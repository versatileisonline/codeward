use std::sync::Weak;
use zeroize::Zeroizing;

use super::*;
use crate::{
    crypto::SecretKey,
    vault::{self, AuthenticatedVault, Enrollment, RecoveryAccess},
};

enum Access {
    Master(AuthenticatedVault),
    Recovery(RecoveryAccess),
    Enrollment(Enrollment),
}

mod authentication;
mod listing;
mod policy;
#[cfg(test)]
mod tests;
pub(super) mod write;
use authentication::Attempt;
pub use authentication::{AuthenticationJob, AuthenticationResult};
pub use listing::{CredentialCursor, CredentialPage, CredentialSummary, MAX_CREDENTIAL_SUMMARIES};
use policy::Inactivity;
pub use policy::{ActivityRef, INACTIVITY_TIMEOUT, SessionActivity, SessionEvent};

/// Secret-owning admission context, separate from potentially blocked storage.
///
/// The core checks inactivity before privileged operations. The host must schedule
/// deadline polls, deliver trusted security events and redact its own copies;
/// no background timer, OS adapter, IPC or draft manager is supplied.
/// After a commit, refresh a current content update or explicitly reload before
/// preparing a new mutation. The old snapshot never stands in for a new revision.
pub struct Session {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
    exhausted: bool,
    source: Option<Fingerprint>,
    access: Option<Access>,
    intent_key: Option<SecretKey>,
    permit: Option<Arc<Permit>>,
    preparation: Option<Arc<Permit>>,
    authentication: Option<Arc<Attempt>>,
    authentication_job: Weak<()>,
    inactivity: Inactivity,
}

impl Session {
    fn new<L: WriterLease>(
        worker: &Committer<L>,
        access: Option<Access>,
    ) -> Result<Self, TransactionError> {
        let inactivity = Inactivity::new(std::time::Instant::now(), access.is_some())?;
        Ok(Self {
            scope: worker.scope.clone(),
            caller: worker.caller,
            generation: 1,
            exhausted: false,
            source: worker.fingerprint,
            intent_key: if access.is_some() {
                Some(SecretKey::generate().map_err(VaultError::from)?)
            } else {
                None
            },
            access,
            permit: None,
            preparation: None,
            authentication: None,
            authentication_job: Weak::new(),
            inactivity,
        })
    }

    pub(super) fn master<L: WriterLease>(
        worker: &Committer<L>,
        password: &str,
    ) -> Result<Self, TransactionError> {
        Self::new(
            worker,
            Some(Access::Master(vault::unlock_with_master(
                worker.base_bytes()?,
                password,
            )?)),
        )
    }

    pub(super) fn recovery<L: WriterLease>(
        worker: &Committer<L>,
        paper: &str,
    ) -> Result<Self, TransactionError> {
        Self::new(
            worker,
            Some(Access::Recovery(vault::begin_recovery(
                worker.base_bytes()?,
                paper,
            )?)),
        )
    }

    pub(super) fn enrollment<L: WriterLease>(
        worker: &Committer<L>,
    ) -> Result<Self, TransactionError> {
        Self::new(worker, Some(Access::Enrollment(Enrollment::begin()?)))
    }

    pub(super) fn locked<L: WriterLease>(worker: &Committer<L>) -> Result<Self, TransactionError> {
        Self::new(worker, None)
    }

    /// Current generation for targeted core/host response scoping.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Revoke access immediately, cancel a not-yet-admitted candidate and release
    /// owned keys/records/enrollment. Already admitted publication still resolves.
    /// This needs no storage-thread mutex and never waits for a storage operation.
    pub fn lock(&mut self) {
        self.inactivity.disarm();
        if let Some(attempt) = self.authentication.take() {
            attempt.cancel();
        }
        if let Some(permit) = self.preparation.take() {
            permit.cancel();
        }
        if let Some(permit) = self.permit.take() {
            permit.cancel();
        }
        self.access = None;
        self.intent_key = None;
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            self.exhausted = true;
        }
    }

    /// Whether this admission context has revoked access. Preparation retains
    /// the current generation while a worker temporarily owns its access state.
    /// Hosts must poll inactivity before composing a session-state reply.
    pub fn is_locked(&self) -> bool {
        self.access.is_none() && self.preparation.is_none()
    }

    /// Explicitly authenticate the worker's known committed base in a fresh
    /// generation. Does not reconcile an unknown outcome or restore canceled work.
    /// This convenience call blocks; hosts needing prompt lock use the split-phase
    /// authentication job instead of holding the session while deriving.
    pub fn reload_master<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        password: &str,
    ) -> Result<(), TransactionError> {
        let job = self.begin_master_authentication(worker, caller, password)?;
        self.complete_authentication(worker, caller, job.run())
    }

    /// Fresh restricted authentication after lock or a completed recovery commit.
    pub fn reload_recovery<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        paper: &str,
    ) -> Result<(), TransactionError> {
        let job = self.begin_recovery_authentication(worker, caller, paper)?;
        self.complete_authentication(worker, caller, job.run())
    }

    /// Restart canceled/failed uncommitted enrollment with a newly transcribed code.
    pub fn restart_enrollment<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        worker.check_idle()?;
        if worker.fingerprint.is_some() {
            return Err(TransactionError::ReloadRequired);
        }
        self.lock();
        if self.exhausted {
            return Err(TransactionError::GenerationExhausted);
        }
        let access = Access::Enrollment(Enrollment::begin()?);
        self.install(worker, access)?;
        self.start_inactivity_at(std::time::Instant::now())
    }

    fn install<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        access: Access,
    ) -> Result<(), TransactionError> {
        self.intent_key = Some(SecretKey::generate().map_err(VaultError::from)?);
        self.source = worker.fingerprint;
        self.access = Some(access);
        Ok(())
    }

    /// Enrollment-only temporary code display, scoped to the owning trusted caller.
    pub fn recovery_code(
        &mut self,
        caller: CallerId,
    ) -> Result<Zeroizing<String>, TransactionError> {
        if caller != self.caller {
            return Err(TransactionError::Unauthorized);
        }
        self.poll_inactivity();
        if self.preparation.is_some() {
            return Err(TransactionError::Busy);
        }
        match self.access.as_ref() {
            Some(Access::Enrollment(access)) => Ok(access.recovery_code()?),
            _ => Err(TransactionError::Locked),
        }
    }

    /// Refresh ordinary access after this generation's content commit. The trusted
    /// host calls this with the returned operation reference once storage resolves.
    /// Reuses the current data key without refreshing inactivity or returning
    /// credentials. Repeating the refresh of the current commit is harmless.
    pub fn refresh_committed_contents<L: WriterLease>(
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
        let Some(Access::Master(access)) = self.access.as_mut() else {
            return Err(TransactionError::Locked);
        };
        if reference.generation != self.generation {
            return Err(TransactionError::Locked);
        }
        worker.check_idle()?;
        let Some(committed) = worker.fingerprint else {
            return Err(TransactionError::ReloadRequired);
        };
        // Only this generation's master access can have submitted this operation.
        // The worker advances the revision only for a barrier-completed commit.
        let status = OperationStatus::Committed {
            revision: committed.revision,
        };
        if worker.status(caller, reference)? != status {
            return Err(TransactionError::ReloadRequired);
        }
        if self.source == Some(committed) {
            return Ok(());
        }
        let result = access.refresh_contents(worker.base_bytes()?);
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        result?;
        self.source = Some(committed);
        Ok(())
    }

    fn check_scope<L: WriterLease>(
        &self,
        worker: &Committer<L>,
        caller: CallerId,
    ) -> Result<(), TransactionError> {
        if caller != self.caller
            || caller != worker.caller
            || !Arc::ptr_eq(&self.scope, &worker.scope)
        {
            return Err(TransactionError::Unauthorized);
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.lock();
    }
}

pub(super) fn check_password_bounds(
    password: &str,
    confirmation: &str,
) -> Result<(), TransactionError> {
    for input in [password, confirmation] {
        if input.is_empty() || input.len() > vault::MAX_MASTER_PASSWORD_BYTES {
            return Err(VaultError::InvalidPassword.into());
        }
    }
    Ok(())
}
