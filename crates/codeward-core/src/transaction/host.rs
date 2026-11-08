//! Headless control-side orchestration for one opened writer and trusted caller.

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Instant,
};

use zeroize::Zeroizing;

use super::*;
use crate::{
    credentials::CredentialSnapshot, recovery::RecoveryError, vault::MAX_MASTER_PASSWORD_BYTES,
};

/// Borrowed authentication input owned by the trusted caller.
pub enum AccessProof<'a> {
    /// Ordinary master-password authentication.
    Master(&'a str),
    /// Restricted paper-code authentication.
    Paper(&'a str),
}

/// Controlled failures from the headless host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostError {
    /// A session or storage operation refused the request.
    Transaction(TransactionError),
    /// A worker failed to start or return. A lost writer requires reopening.
    WorkerUnavailable,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transaction(error) => error.fmt(f),
            Self::WorkerUnavailable => f.write_str("headless worker unavailable"),
        }
    }
}

impl Error for HostError {}

impl From<TransactionError> for HostError {
    fn from(error: TransactionError) -> Self {
        Self::Transaction(error)
    }
}

/// Targeted worker result for the trusted caller. Contains no credential data.
pub enum HostCompletion {
    /// The current authentication completed. Revoked results are discarded.
    Authentication(Result<(), TransactionError>),
    /// A commit or reconciliation completed without granting access by itself.
    Transaction {
        /// Original operation reference, also usable for status lookup.
        operation: OperationRef,
        /// Proven worker outcome, or the error from a reconciliation attempt.
        outcome: Result<OperationStatus, TransactionError>,
        /// Snapshot adoption for a successful ordinary save. An error here does
        /// not undo or change the independent commit outcome.
        refresh: Option<Result<(), TransactionError>>,
    },
}

struct FinishedWrite<L: WriterLease> {
    worker: Committer<L>,
    operation: OperationRef,
    outcome: Result<OperationStatus, TransactionError>,
    refresh: bool,
}

enum Pending<L: WriterLease> {
    Authentication(Receiver<AuthenticationResult>),
    Write(Receiver<FinishedWrite<L>>),
}

/// One headless vault opened under a trusted Rust caller. Keep it on the
/// control thread. Workers never own the live session, so lock can revoke
/// access while authentication or storage is blocked.
///
/// Poll for worker results and schedule `inactivity_deadline` even when idle.
/// The control loop supplies wakeups and trusted events. Open and candidate
/// preparation are synchronous. The host accepts explicit save snapshots and
/// holds no drafts.
pub struct HeadlessVault<L: WriterLease + Send + 'static> {
    // Drop revokes access before releasing the worker or dropping its receiver.
    session: Session,
    caller: CallerId,
    pending: Option<Pending<L>>,
    worker: Option<Committer<L>>,
}

impl<L: WriterLease + Send + 'static> HeadlessVault<L> {
    /// Open an existing destination locked and retain writer exclusion.
    pub fn open<S: Storage<Lease = L>>(storage: S, caller: CallerId) -> Result<Self, HostError> {
        let (session, worker) = Committer::open_locked(storage, caller)?;
        Ok(Self::new(session, worker, caller))
    }

    /// Start volatile enrollment for an absent destination.
    pub fn begin_create<S: Storage<Lease = L>>(
        storage: S,
        caller: CallerId,
    ) -> Result<Self, HostError> {
        let (session, worker) = Committer::begin_create(storage, caller)?;
        Ok(Self::new(session, worker, caller))
    }

    fn new(session: Session, worker: Committer<L>, caller: CallerId) -> Self {
        Self {
            session,
            caller,
            worker: Some(worker),
            pending: None,
        }
    }

    /// Authenticate on a worker. Only a current, unrevoked completion can grant
    /// access. A paper code opens restricted recovery.
    pub fn begin_authentication(&mut self, proof: AccessProof<'_>) -> Result<(), HostError> {
        let (session, worker, caller) = self.idle()?;
        let job = match proof {
            AccessProof::Master(password) => {
                session.begin_master_authentication(worker, caller, password)?
            }
            AccessProof::Paper(paper) => {
                session.begin_recovery_authentication(worker, caller, paper)?
            }
        };
        self.pending = Some(Pending::Authentication(self.spawn(move || job.run())?));
        Ok(())
    }

    /// Display the temporary paper code during enrollment.
    pub fn recovery_code(&mut self) -> Result<Zeroizing<String>, HostError> {
        self.poll_deadline();
        let code = self.session.recovery_code(self.caller);
        self.discard_revoked_authentication();
        Ok(code?)
    }

    /// Restart an uncommitted enrollment with a fresh code.
    pub fn restart_enrollment(&mut self) -> Result<(), HostError> {
        let (session, worker, caller) = self.idle()?;
        Ok(session.restart_enrollment(worker, caller)?)
    }

    /// Confirm enrollment and dispatch creation. Only a committed result is ready.
    pub fn create(
        &mut self,
        request: Request,
        password: &str,
        confirmation: &str,
        paper: &str,
    ) -> Result<OperationRef, HostError> {
        let (session, worker, caller) = self.idle()?;
        let operation =
            session.prepare_creation(worker, caller, request, password, confirmation, paper)?;
        self.dispatch_prepared(operation, false)
    }

    /// Dispatch an explicit content save. A duplicate completed request returns
    /// its known reference without starting new work or emitting a new result.
    pub fn save(
        &mut self,
        request: Request,
        snapshot: &CredentialSnapshot,
    ) -> Result<OperationRef, HostError> {
        let (session, worker, caller) = self.idle()?;
        let operation = session.prepare_contents(worker, caller, request, snapshot)?;
        self.dispatch_prepared(operation, true)
    }

    /// Dispatch replacement of the master password from restricted recovery.
    pub fn replace_master_password(
        &mut self,
        request: Request,
        password: &str,
        confirmation: &str,
    ) -> Result<OperationRef, HostError> {
        let (session, worker, caller) = self.idle()?;
        let operation =
            session.prepare_recovery(worker, caller, request, password, confirmation)?;
        self.dispatch_prepared(operation, false)
    }

    /// Reconcile the current uncertain operation using fresh proof. Successful
    /// reconciliation reports the proven state. A changed base needs fresh
    /// authentication before further ordinary access.
    pub fn reconcile(
        &mut self,
        operation: &OperationRef,
        proof: AccessProof<'_>,
    ) -> Result<(), HostError> {
        let (_, worker, caller) = self.idle()?;
        if worker.status(caller, operation)? != OperationStatus::OutcomeUnknown {
            return Err(TransactionError::OutcomeUnknown.into());
        }
        let (input, paper) = match proof {
            AccessProof::Master(password) => {
                if password.is_empty() || password.len() > MAX_MASTER_PASSWORD_BYTES {
                    return Err(TransactionError::Vault(VaultError::InvalidPassword).into());
                }
                (password, false)
            }
            AccessProof::Paper(code) => {
                if code.len() > 256 {
                    return Err(TransactionError::Vault(VaultError::Recovery(
                        RecoveryError::InvalidCode,
                    ))
                    .into());
                }
                (code, true)
            }
        };
        let mut owned = Zeroizing::new(String::new());
        owned
            .try_reserve_exact(input.len())
            .map_err(|_| TransactionError::AllocationFailed)?;
        owned.push_str(input);
        self.dispatch_write(
            operation.clone(),
            false,
            move |worker, caller, operation| {
                if paper {
                    worker.reconcile_recovery(caller, operation, &owned)
                } else {
                    worker.reconcile_master(caller, operation, &owned)
                }
            },
        )
    }

    /// Look up an operation after the writer returns, including after lock.
    pub fn status(&mut self, operation: &OperationRef) -> Result<OperationStatus, HostError> {
        let (_, worker, caller) = self.idle()?;
        Ok(worker.status(caller, operation)?)
    }

    /// List only current ordinary access. The trusted caller owns delivery scope.
    pub fn list_credentials(
        &mut self,
        generation: u64,
        limit: usize,
        cursor: Option<&CredentialCursor>,
    ) -> Result<CredentialPage, HostError> {
        let (session, worker, caller) = self.idle()?;
        Ok(session.list_credentials(worker, caller, generation, limit, cursor)?)
    }

    /// Current scoped generation. Poll before composing an access-state reply.
    pub fn generation(&self) -> u64 {
        self.session.generation()
    }

    /// Check expiry and whether the session has released its access state.
    pub fn is_locked(&mut self) -> bool {
        self.poll_deadline();
        self.session.is_locked()
    }

    /// Monotonic deadline the control loop should schedule, even without work.
    pub fn inactivity_deadline(&self) -> Option<Instant> {
        self.session.inactivity_deadline()
    }

    /// Whether a worker result is outstanding. An already revoked authentication
    /// can still be finishing its KDF without a result to poll.
    pub fn has_pending_work(&self) -> bool {
        self.pending.is_some()
    }

    /// Obtain a scoped interaction reference for the current session.
    pub fn activity_reference(&mut self) -> Result<ActivityRef, HostError> {
        self.poll_deadline();
        let reference = self.session.activity_reference(self.caller);
        self.discard_revoked_authentication();
        Ok(reference?)
    }

    /// Record a scoped interaction without requiring the storage worker.
    pub fn record_activity(
        &mut self,
        reference: &ActivityRef,
        activity: SessionActivity,
    ) -> Result<(), HostError> {
        self.poll_deadline();
        let result = self
            .session
            .record_activity(self.caller, reference, activity);
        self.discard_revoked_authentication();
        Ok(result?)
    }

    /// Deliver a trusted event. Security events revoke access immediately.
    pub fn observe_event(&mut self, event: SessionEvent) -> bool {
        let revoked = self.session.observe_event(event);
        if revoked {
            self.discard_authentication();
        }
        revoked
    }

    /// Revoke access without waiting for authentication or a storage call.
    pub fn lock(&mut self) {
        self.session.lock();
        self.discard_authentication();
    }

    /// Revoke and close without joining an in-flight worker. An admitted commit retains
    /// its worker and lease until it resolves. Reopening checks the actual file.
    pub fn close(mut self) {
        self.lock();
    }

    /// Check expiry and consume at most one worker result without blocking on it.
    pub fn poll(&mut self) -> Result<Option<HostCompletion>, HostError> {
        self.poll_deadline();
        let completion = match &self.pending {
            Some(Pending::Authentication(receiver)) => receiver.try_recv().map(|result| {
                let worker = self.worker.as_ref().expect("authentication retains writer");
                HostCompletion::Authentication(self.session.complete_authentication(
                    worker,
                    self.caller,
                    result,
                ))
            }),
            Some(Pending::Write(receiver)) => receiver.try_recv().map(|done| {
                let refresh = (done.refresh
                    && matches!(done.outcome, Ok(OperationStatus::Committed { .. })))
                .then(|| {
                    self.session.refresh_committed_contents(
                        &done.worker,
                        self.caller,
                        &done.operation,
                    )
                });
                self.worker = Some(done.worker);
                HostCompletion::Transaction {
                    operation: done.operation,
                    outcome: done.outcome,
                    refresh,
                }
            }),
            None => return Ok(None),
        };
        match completion {
            Ok(completion) => {
                self.pending = None;
                Ok(Some(completion))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.lock();
                Err(HostError::WorkerUnavailable)
            }
        }
    }

    fn poll_deadline(&mut self) {
        self.session.poll_inactivity();
        self.discard_revoked_authentication();
    }

    fn discard_revoked_authentication(&mut self) {
        if self.session.inactivity_deadline().is_none() {
            self.discard_authentication();
        }
    }

    fn discard_authentication(&mut self) {
        if matches!(self.pending, Some(Pending::Authentication(_))) {
            self.pending = None;
        }
    }

    fn idle(&mut self) -> Result<(&mut Session, &mut Committer<L>, CallerId), HostError> {
        self.poll_deadline();
        if self.pending.is_some() {
            return Err(TransactionError::Busy.into());
        }
        let worker = self.worker.as_mut().ok_or(HostError::WorkerUnavailable)?;
        Ok((&mut self.session, worker, self.caller))
    }

    fn dispatch_prepared(
        &mut self,
        operation: OperationRef,
        refresh: bool,
    ) -> Result<OperationRef, HostError> {
        let worker = self.worker.as_ref().expect("preparation retained writer");
        if matches!(
            worker.status(self.caller, &operation)?,
            OperationStatus::Pending(_)
        ) {
            // Preparation may cross the deadline. Still drain the prepared
            // operation so the worker clears its canceled active record.
            self.poll_deadline();
            self.dispatch_write(operation.clone(), refresh, |worker, _, _| {
                loop {
                    match worker.advance() {
                        Some(OperationStatus::Pending(_)) => continue,
                        Some(status) => return Ok(status),
                        None => return Err(TransactionError::OutcomeUnknown),
                    }
                }
            })?;
        }
        Ok(operation)
    }

    fn dispatch_write(
        &mut self,
        operation: OperationRef,
        refresh: bool,
        run: impl FnOnce(
            &mut Committer<L>,
            CallerId,
            &OperationRef,
        ) -> Result<OperationStatus, TransactionError>
        + Send
        + 'static,
    ) -> Result<(), HostError> {
        let mut worker = self.worker.take().expect("dispatch requires idle writer");
        let caller = self.caller;
        self.pending = Some(Pending::Write(self.spawn(move || {
            let outcome = run(&mut worker, caller, &operation);
            FinishedWrite {
                worker,
                operation,
                outcome,
                refresh,
            }
        })?));
        Ok(())
    }

    fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<Receiver<T>, HostError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        if thread::Builder::new()
            .spawn(move || {
                let _ = sender.send(work());
            })
            .is_err()
        {
            self.lock();
            return Err(HostError::WorkerUnavailable);
        }
        Ok(receiver)
    }
}

#[cfg(test)]
mod tests;
