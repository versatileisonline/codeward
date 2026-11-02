use super::*;
use crate::vault::{self, VaultCandidate};

struct Record {
    reference: OperationRef,
    intent: [u8; 32],
    permit: Arc<Permit>,
    status: OperationStatus,
    candidate: Option<Vec<u8>>,
    target: Fingerprint,
}

/// Encrypted-only transaction worker retaining exclusive writer ownership.
///
/// Move this worker to a storage thread while retaining `Session` on the control
/// side. Never hold the session behind a storage-thread lock. Dropping/restarting
/// loses request history; actual destination state then requires explicit review.
pub struct Committer<L: WriterLease> {
    lease: L,
    pub(super) scope: Arc<()>,
    pub(super) caller: CallerId,
    base: Option<Vec<u8>>,
    pub(super) fingerprint: Option<Fingerprint>,
    records: Vec<Record>,
    active: Option<usize>,
    // An observed external change requires reopen/review, not another blind save.
    conflicted: bool,
}

impl<L: WriterLease> Committer<L> {
    /// Acquire exclusive storage and authenticate ordinary access to exact bytes.
    pub fn open_master<S: Storage<Lease = L>>(
        storage: S,
        caller: CallerId,
        password: &str,
    ) -> Result<(Session, Self), TransactionError> {
        let worker = Self::open(storage, caller)?;
        let session = Session::master(&worker, password)?;
        Ok((session, worker))
    }

    /// Acquire storage and authenticate restricted paper recovery, without browsing.
    pub fn open_recovery<S: Storage<Lease = L>>(
        storage: S,
        caller: CallerId,
        paper: &str,
    ) -> Result<(Session, Self), TransactionError> {
        let worker = Self::open(storage, caller)?;
        let session = Session::recovery(&worker, paper)?;
        Ok((session, worker))
    }

    /// Acquire an absent destination and start volatile, uncommitted enrollment.
    pub fn begin_create<S: Storage<Lease = L>>(
        storage: S,
        caller: CallerId,
    ) -> Result<(Session, Self), TransactionError> {
        let worker = Self::open(storage, caller)?;
        if worker.base.is_some() {
            return Err(TransactionError::ReloadRequired);
        }
        let session = Session::enrollment(&worker)?;
        Ok((session, worker))
    }

    fn open<S: Storage<Lease = L>>(storage: S, caller: CallerId) -> Result<Self, TransactionError> {
        let mut lease = storage.acquire()?;
        let base = lease.read_current()?;
        let fingerprint = base.as_deref().map(Fingerprint::of).transpose()?;
        Ok(Self {
            lease,
            scope: Arc::new(()),
            caller,
            base,
            fingerprint,
            records: Vec::new(),
            active: None,
            conflicted: false,
        })
    }

    pub(super) fn base_bytes(&self) -> Result<&[u8], TransactionError> {
        self.base.as_deref().ok_or(TransactionError::ReloadRequired)
    }

    pub(super) fn check_idle(&self) -> Result<(), TransactionError> {
        if self.active.is_some() {
            return Err(TransactionError::Busy);
        }
        if self.conflicted {
            return Err(TransactionError::ReloadRequired);
        }
        Ok(())
    }

    // Deduplicate before randomness/encryption. Session verifies its caller/mode
    // before this method, even for an already completed request.
    pub(super) fn prior(
        &self,
        request: Request,
        generation: u64,
        intent: &[u8; 32],
    ) -> Result<Option<OperationRef>, TransactionError> {
        if let Some(record) = self.records.iter().find(|r| r.reference.id == request.id) {
            if record.reference.generation != generation || record.intent != *intent {
                return Err(TransactionError::RequestReused);
            }
            return Ok(Some(record.reference.clone()));
        }
        self.check_idle()?;
        if self.records.len() == MAX_REQUESTS {
            return Err(TransactionError::HistoryFull);
        }
        Ok(None)
    }

    pub(super) fn submit(
        &mut self,
        request: Request,
        generation: u64,
        intent: [u8; 32],
        candidate: VaultCandidate,
        permit: Arc<Permit>,
    ) -> Result<OperationRef, TransactionError> {
        self.check_idle()?;
        if self.records.len() == MAX_REQUESTS {
            return Err(TransactionError::HistoryFull);
        }
        let target = Fingerprint::of(candidate.as_bytes())?;
        let expected = self.fingerprint.map_or(0, |base| base.revision);
        if request.expected_revision != expected
            || expected.checked_add(1) != Some(target.revision)
            || self
                .fingerprint
                .is_some_and(|base| base.vault_id != target.vault_id)
        {
            return Err(VaultError::RevisionConflict.into());
        }
        self.records
            .try_reserve(1)
            .map_err(|_| TransactionError::AllocationFailed)?;
        let reference = OperationRef {
            scope: self.scope.clone(),
            caller: self.caller,
            generation,
            id: request.id,
        };
        self.active = Some(self.records.len());
        self.records.push(Record {
            reference: reference.clone(),
            intent,
            permit,
            status: OperationStatus::Pending(Phase::Prepared),
            candidate: Some(candidate.into_bytes()),
            target,
        });
        Ok(reference)
    }

    /// Return only safe status to the original trusted caller, including after lock.
    /// References from a closed/reopened worker are unavailable, never replayable.
    pub fn status(
        &self,
        caller: CallerId,
        reference: &OperationRef,
    ) -> Result<OperationStatus, TransactionError> {
        let index = self.index(caller, reference)?;
        Ok(index.map_or(OperationStatus::Unavailable, |i| {
            let record = &self.records[i];
            if matches!(record.status, OperationStatus::Pending(_)) && record.permit.canceled() {
                OperationStatus::Canceled
            } else {
                record.status
            }
        }))
    }

    fn index(
        &self,
        caller: CallerId,
        reference: &OperationRef,
    ) -> Result<Option<usize>, TransactionError> {
        if caller != self.caller || reference.caller != caller {
            return Err(TransactionError::Unauthorized);
        }
        if !Arc::ptr_eq(&self.scope, &reference.scope) {
            return Ok(None);
        }
        Ok(self.records.iter().position(|r| {
            r.reference.id == reference.id && r.reference.generation == reference.generation
        }))
    }

    /// Execute at most one storage/admission stage. This is an internal worker
    /// operation, never a UI command. It cannot return plaintext or reopen access.
    pub fn advance(&mut self) -> Option<OperationStatus> {
        let i = self.active?;
        if self.records[i].permit.canceled() {
            self.finish(i, OperationStatus::Canceled);
            return Some(OperationStatus::Canceled);
        }
        let status = match self.records[i].status {
            OperationStatus::Pending(Phase::Prepared) => {
                match self.lease.stage(
                    self.records[i]
                        .candidate
                        .as_deref()
                        .expect("active candidate"),
                ) {
                    Ok(()) => OperationStatus::Pending(Phase::Staged),
                    Err(error) => OperationStatus::Failed(error),
                }
            }
            OperationStatus::Pending(Phase::Staged) => match self.lease.read_current() {
                Err(error) => OperationStatus::Failed(error),
                Ok(current) => {
                    let matches = match current.as_deref() {
                        None => self.fingerprint.is_none(),
                        Some(bytes) => {
                            Fingerprint::of(bytes).ok() == self.fingerprint
                                && self.fingerprint.is_some()
                        }
                    };
                    if !matches {
                        self.conflicted = true;
                        OperationStatus::Conflict
                    } else if self.records[i].permit.admit() {
                        OperationStatus::Pending(Phase::Admitted)
                    } else {
                        OperationStatus::Canceled
                    }
                }
            },
            OperationStatus::Pending(Phase::Admitted) => {
                let mode = if self.base.is_none() {
                    PublishMode::Create
                } else {
                    PublishMode::Replace
                };
                match self.lease.publish(mode) {
                    Ok(()) => OperationStatus::Pending(Phase::Published),
                    Err(PublishError::NotPublished) => OperationStatus::NotPublished,
                    Err(PublishError::OutcomeUnknown) => OperationStatus::OutcomeUnknown,
                }
            }
            OperationStatus::Pending(Phase::Published) => match self.lease.persist() {
                Ok(()) => OperationStatus::Committed {
                    revision: self.records[i].target.revision,
                },
                Err(_) => OperationStatus::OutcomeUnknown,
            },
            OperationStatus::OutcomeUnknown => return Some(OperationStatus::OutcomeUnknown),
            _ => unreachable!("only active transactions advance"),
        };
        // Lock may have won while a prepublication storage call was blocked.
        let status = if self.records[i].permit.canceled() {
            OperationStatus::Canceled
        } else {
            status
        };
        self.records[i].status = status;
        if !matches!(
            status,
            OperationStatus::Pending(_) | OperationStatus::OutcomeUnknown
        ) {
            self.finish(i, status);
        }
        Some(status)
    }

    fn finish(&mut self, i: usize, status: OperationStatus) {
        if matches!(status, OperationStatus::Committed { .. }) {
            self.base = self.records[i].candidate.take();
            self.fingerprint = Some(self.records[i].target);
        } else {
            self.records[i].candidate = None;
        }
        self.records[i].status = status;
        self.active = None;
    }

    /// Reopen actual bytes under the retained lease and freshly authenticate the
    /// known base/candidate through master access. Any failure stays unresolved.
    /// This does not unlock a Session or return credential data.
    pub fn reconcile_master(
        &mut self,
        caller: CallerId,
        reference: &OperationRef,
        password: &str,
    ) -> Result<OperationStatus, TransactionError> {
        self.reconcile(caller, reference, |bytes| {
            vault::unlock_with_master(bytes, password).map(|access| access.lock())
        })
    }

    /// Restricted paper authentication for reconciliation; keys are dropped before
    /// barriers and never retained merely to answer later status requests.
    pub fn reconcile_recovery(
        &mut self,
        caller: CallerId,
        reference: &OperationRef,
        paper: &str,
    ) -> Result<OperationStatus, TransactionError> {
        self.reconcile(caller, reference, |bytes| {
            vault::begin_recovery(bytes, paper).map(drop)
        })
    }

    fn reconcile(
        &mut self,
        caller: CallerId,
        reference: &OperationRef,
        authenticate: impl FnOnce(&[u8]) -> Result<(), VaultError>,
    ) -> Result<OperationStatus, TransactionError> {
        let i = self
            .index(caller, reference)?
            .ok_or(TransactionError::OutcomeUnknown)?;
        if self.records[i].status != OperationStatus::OutcomeUnknown {
            return self.status(caller, reference);
        }
        let current = self.lease.read_current()?;
        let is_candidate = current.as_deref() == self.records[i].candidate.as_deref();
        let is_base = current.as_deref() == self.base.as_deref();
        if !is_candidate && !is_base {
            return Err(TransactionError::OutcomeUnknown);
        }
        if let Some(bytes) = current.as_deref() {
            // Authenticate + schema validate; byte equality alone is not success.
            authenticate(bytes)?;
        }
        // Includes absence after uncertain initial creation. Adapter must establish
        // the declared namespace barrier; read-back/absence alone is not durable.
        self.lease.persist()?;
        let status = if is_candidate {
            OperationStatus::Committed {
                revision: self.records[i].target.revision,
            }
        } else {
            OperationStatus::Unchanged
        };
        self.finish(i, status);
        Ok(status)
    }

    /// Authenticated baseline loaded on open, or the last barrier-completed update.
    /// Pending/unknown candidates never advance it. Loading visible bytes after
    /// restart does not prove a previous process's barriers or historical outcome.
    pub fn committed_revision(&self) -> Option<u64> {
        self.fingerprint.map(|base| base.revision)
    }
}
