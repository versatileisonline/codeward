use hmac::{Hmac, KeyInit, Mac};
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

/// Secret-owning admission context, separate from potentially blocked storage.
///
/// This bounded layer exposes no credential replies, timers, OS events or draft
/// manager. The host must call lock on security events and redact its own copies.
/// After a commit, explicitly reload/authenticate before preparing a new mutation;
/// the old authenticated snapshot is never silently treated as the new revision.
pub struct Session {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
    exhausted: bool,
    source: Option<Fingerprint>,
    access: Option<Access>,
    intent_key: Option<SecretKey>,
    permit: Option<Arc<Permit>>,
}

impl Session {
    fn new<L: WriterLease>(
        worker: &Committer<L>,
        access: Access,
    ) -> Result<Self, TransactionError> {
        Ok(Self {
            scope: worker.scope.clone(),
            caller: worker.caller,
            generation: 1,
            exhausted: false,
            source: worker.fingerprint,
            access: Some(access),
            intent_key: Some(SecretKey::generate().map_err(VaultError::from)?),
            permit: None,
        })
    }

    pub(super) fn master<L: WriterLease>(
        worker: &Committer<L>,
        password: &str,
    ) -> Result<Self, TransactionError> {
        Self::new(
            worker,
            Access::Master(vault::unlock_with_master(worker.base_bytes()?, password)?),
        )
    }

    pub(super) fn recovery<L: WriterLease>(
        worker: &Committer<L>,
        paper: &str,
    ) -> Result<Self, TransactionError> {
        Self::new(
            worker,
            Access::Recovery(vault::begin_recovery(worker.base_bytes()?, paper)?),
        )
    }

    pub(super) fn enrollment<L: WriterLease>(
        worker: &Committer<L>,
    ) -> Result<Self, TransactionError> {
        Self::new(worker, Access::Enrollment(Enrollment::begin()?))
    }

    /// Current generation for targeted core/host response scoping.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Revoke access immediately, cancel a not-yet-admitted candidate and release
    /// owned keys/records/enrollment. Already admitted publication still resolves.
    /// This needs no storage-thread mutex and never waits for a storage operation.
    pub fn lock(&mut self) {
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

    /// Whether this admission context has released its secret access.
    pub fn is_locked(&self) -> bool {
        self.access.is_none()
    }

    /// Explicitly authenticate the worker's known committed base in a fresh
    /// generation. Does not reconcile an unknown outcome or restore canceled work.
    pub fn reload_master<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        password: &str,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        worker.check_idle()?;
        self.lock();
        if self.exhausted {
            return Err(TransactionError::GenerationExhausted);
        }
        let access = vault::unlock_with_master(worker.base_bytes()?, password)?;
        self.install(worker, Access::Master(access))
    }

    /// Fresh restricted authentication after lock or a completed recovery commit.
    pub fn reload_recovery<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        paper: &str,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        worker.check_idle()?;
        self.lock();
        if self.exhausted {
            return Err(TransactionError::GenerationExhausted);
        }
        let access = vault::begin_recovery(worker.base_bytes()?, paper)?;
        self.install(worker, Access::Recovery(access))
    }

    /// Restart canceled/failed uncommitted enrollment with a newly transcribed code.
    pub fn restart_enrollment<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        worker.check_idle()?;
        if worker.fingerprint.is_some() {
            return Err(TransactionError::ReloadRequired);
        }
        self.lock();
        if self.exhausted {
            return Err(TransactionError::GenerationExhausted);
        }
        self.install(worker, Access::Enrollment(Enrollment::begin()?))
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
    pub fn recovery_code(&self, caller: CallerId) -> Result<Zeroizing<String>, TransactionError> {
        if caller != self.caller {
            return Err(TransactionError::Unauthorized);
        }
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
