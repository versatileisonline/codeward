//! Decode the exact committed snapshot while retaining revocation on the control side.

use super::*;

pub(in crate::transaction) struct RefreshJob {
    access: AuthenticatedVault,
    bytes: Arc<Vec<u8>>,
    target: Fingerprint,
    permit: Arc<Permit>,
}

pub(in crate::transaction) struct RefreshResult {
    access: AuthenticatedVault,
    target: Fingerprint,
    permit: Arc<Permit>,
    outcome: Result<(), VaultError>,
}

impl RefreshJob {
    pub(in crate::transaction) fn run(self) -> Result<RefreshResult, TransactionError> {
        let Self {
            mut access,
            bytes,
            target,
            permit,
        } = self;
        if permit.canceled() {
            return Err(TransactionError::Locked);
        }
        // The old snapshot is released on the worker when decoding succeeds.
        let outcome = access.refresh_contents(&bytes);
        if permit.canceled() {
            return Err(TransactionError::Locked);
        }
        Ok(RefreshResult {
            access,
            target,
            permit,
            outcome,
        })
    }
}

impl Session {
    /// Synchronously refresh ordinary access after this generation's content
    /// commit. Reuses the current data key without refreshing inactivity or
    /// returning credentials. Repeating the current refresh is harmless.
    /// The headless host runs the same work on a worker.
    pub fn refresh_committed_contents<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &OperationRef,
    ) -> Result<(), TransactionError> {
        if let Some(job) = self.begin_refresh(worker, caller, reference)? {
            let result = job.run()?;
            self.finish_refresh(worker, caller, reference, result)?;
        }
        Ok(())
    }

    pub(in crate::transaction) fn begin_refresh<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &OperationRef,
    ) -> Result<Option<RefreshJob>, TransactionError> {
        self.check_refresh_reference(worker, caller, reference)?;
        if !matches!(self.access, Some(Access::Master(_))) {
            return Err(TransactionError::Locked);
        }
        let target = committed_target(worker, caller, reference)?;
        if self.source == Some(target) {
            return Ok(None);
        }
        let bytes = worker.shared_base()?;
        let permit = Arc::new(Permit::new());
        let Some(Access::Master(access)) = self.access.take() else {
            unreachable!("refresh requires ordinary access");
        };
        self.worker_access = Some(permit.clone());
        Ok(Some(RefreshJob {
            access,
            bytes,
            target,
            permit,
        }))
    }

    pub(in crate::transaction) fn finish_refresh<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &OperationRef,
        result: RefreshResult,
    ) -> Result<(), TransactionError> {
        self.check_refresh_reference(worker, caller, reference)?;
        if !self
            .worker_access
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &result.permit))
        {
            return Err(TransactionError::Locked);
        }
        let target = committed_target(worker, caller, reference)
            .and_then(|target| {
                (target == result.target)
                    .then_some(target)
                    .ok_or(TransactionError::ReloadRequired)
            })
            .inspect_err(|_| self.lock())?;
        self.worker_access = None;
        self.access = Some(Access::Master(result.access));
        result.outcome?;
        self.source = Some(target);
        Ok(())
    }

    fn check_refresh_reference<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &OperationRef,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        if reference.caller != caller || !Arc::ptr_eq(&self.scope, &reference.scope) {
            return Err(TransactionError::Unauthorized);
        }
        self.poll_inactivity();
        if reference.generation != self.generation {
            return Err(TransactionError::Locked);
        }
        Ok(())
    }
}

fn committed_target<L: WriterLease>(
    worker: &Committer<L>,
    caller: CallerId,
    reference: &OperationRef,
) -> Result<Fingerprint, TransactionError> {
    worker.check_idle()?;
    let target = worker.fingerprint.ok_or(TransactionError::ReloadRequired)?;
    let committed = OperationStatus::Committed {
        revision: target.revision,
    };
    if worker.status(caller, reference)? != committed {
        return Err(TransactionError::ReloadRequired);
    }
    Ok(target)
}
