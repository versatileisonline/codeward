use hmac::{Hmac, KeyInit, Mac};
use std::sync::Weak;
use zeroize::Zeroizing;

use super::*;
use crate::{
    credentials::CredentialSnapshot,
    crypto::SecretKey,
    vault::{self, AuthenticatedVault, Enrollment, RecoveryAccess, VaultCandidate},
};

enum Access {
    Master(AuthenticatedVault),
    Recovery(RecoveryAccess),
    Enrollment(Enrollment),
}

mod authentication;
mod policy;
#[cfg(test)]
mod tests;
use authentication::Attempt;
pub use authentication::{AuthenticationJob, AuthenticationResult};
use policy::Inactivity;
pub use policy::{ActivityRef, INACTIVITY_TIMEOUT, SessionActivity, SessionEvent};

/// Secret-owning admission context, separate from potentially blocked storage.
///
/// The core checks inactivity before privileged operations. The host must schedule
/// deadline polls, deliver trusted security events and redact its own copies;
/// no background timer, OS adapter, credential replies or draft manager is supplied.
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

    /// Whether this admission context has released its secret access. This is a
    /// snapshot; hosts must poll inactivity before composing a session-state reply.
    pub fn is_locked(&self) -> bool {
        self.access.is_none()
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
        match self.access.as_ref() {
            Some(Access::Enrollment(access)) => Ok(access.recovery_code()?),
            _ => Err(TransactionError::Locked),
        }
    }

    /// Admit an explicitly requested whole-snapshot content edit/deletion.
    /// Identical known requests return their existing reference before encryption.
    /// A new ID after definite failure generates fresh encryption/nonces.
    pub fn prepare_contents<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        snapshot: &CredentialSnapshot,
    ) -> Result<OperationRef, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        if !matches!(self.access, Some(Access::Master(_))) {
            return Err(TransactionError::Locked);
        }
        let encoded = snapshot.encode().map_err(VaultError::from)?;
        let intent = self.intent(b"contents", request, &[&encoded])?;
        drop(encoded);
        if let Some(prior) = self.admission(worker, request, &intent)? {
            return Ok(prior);
        }
        let Some(Access::Master(access)) = self.access.as_ref() else {
            unreachable!()
        };
        let candidate = access.prepare_contents(request.expected_revision, snapshot)?;
        self.submit(worker, request, intent, candidate)
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

    /// Admit a coherent replacement master slot/payload in restricted recovery.
    /// Input copies belong to the caller; no password is retained in request history.
    pub fn prepare_recovery<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        password: &str,
        confirmation: &str,
    ) -> Result<OperationRef, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        if !matches!(self.access, Some(Access::Recovery(_))) {
            return Err(TransactionError::Locked);
        }
        check_password_bounds(password, confirmation)?;
        let intent = self.intent(
            b"recovery",
            request,
            &[password.as_bytes(), confirmation.as_bytes()],
        )?;
        if let Some(prior) = self.admission(worker, request, &intent)? {
            return Ok(prior);
        }
        let Some(Access::Recovery(access)) = self.access.as_ref() else {
            unreachable!()
        };
        let candidate =
            access.prepare_master_password(request.expected_revision, password, confirmation)?;
        self.submit(worker, request, intent, candidate)
    }

    /// Confirm enrollment and admit creation; this returns pending unsaved status,
    /// never Ready/Created. Initial publication must atomically refuse an existing file.
    pub fn prepare_creation<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        password: &str,
        confirmation: &str,
        paper: &str,
    ) -> Result<OperationRef, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        if !matches!(self.access, Some(Access::Enrollment(_))) {
            return Err(TransactionError::Locked);
        }
        check_password_bounds(password, confirmation)?;
        if paper.len() > 256 {
            return Err(VaultError::Recovery(crate::recovery::RecoveryError::InvalidCode).into());
        }
        let intent = self.intent(
            b"creation",
            request,
            &[
                password.as_bytes(),
                confirmation.as_bytes(),
                paper.as_bytes(),
            ],
        )?;
        if let Some(prior) = self.admission(worker, request, &intent)? {
            return Ok(prior);
        }
        let Some(Access::Enrollment(access)) = self.access.as_mut() else {
            unreachable!()
        };
        let candidate = access.prepare(password, confirmation, paper)?;
        self.submit(worker, request, intent, candidate)
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

    fn admission<L: WriterLease>(
        &self,
        worker: &Committer<L>,
        request: Request,
        intent: &[u8; 32],
    ) -> Result<Option<OperationRef>, TransactionError> {
        let prior = worker.prior(request, self.generation, intent)?;
        if prior.is_some() {
            return Ok(prior);
        }
        if self.source != worker.fingerprint {
            return Err(TransactionError::ReloadRequired);
        }
        if request.expected_revision != self.source.map_or(0, |source| source.revision) {
            return Err(VaultError::RevisionConflict.into());
        }
        Ok(None)
    }

    fn submit<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        request: Request,
        intent: [u8; 32],
        candidate: VaultCandidate,
    ) -> Result<OperationRef, TransactionError> {
        let permit = Arc::new(Permit::new());
        let reference =
            worker.submit(request, self.generation, intent, candidate, permit.clone())?;
        self.permit = Some(permit);
        Ok(reference)
    }

    fn intent(
        &self,
        kind: &[u8],
        request: Request,
        fields: &[&[u8]],
    ) -> Result<[u8; 32], TransactionError> {
        // Per-generation random HMAC key avoids a retained offline verifier for
        // low-entropy replacement passwords; only fixed-length digests enter history.
        let key = self.intent_key.as_ref().ok_or(TransactionError::Locked)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(key.bytes()).expect("fixed HMAC key length");
        mac.update(b"Codeward volatile request v1\0");
        mac.update(kind);
        mac.update(&request.expected_revision.to_le_bytes());
        for field in fields {
            mac.update(&(field.len() as u64).to_le_bytes());
            mac.update(field);
        }
        Ok(mac.finalize().into_bytes().into())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.lock();
    }
}

fn check_password_bounds(password: &str, confirmation: &str) -> Result<(), TransactionError> {
    for input in [password, confirmation] {
        if input.is_empty() || input.len() > vault::MAX_MASTER_PASSWORD_BYTES {
            return Err(VaultError::InvalidPassword.into());
        }
    }
    Ok(())
}
