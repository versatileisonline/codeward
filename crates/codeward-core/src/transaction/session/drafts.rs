//! Session-owned volatile generated passwords.

use std::time::Instant;

use super::*;
use crate::{
    credentials::copy_text,
    password::{GeneratedPassword, GenerationError, PasswordOptions, generate_password},
};

pub(super) struct PasswordDraft {
    identity: Arc<()>,
    password: GeneratedPassword,
}

/// Opaque reference to one volatile password in one session and revision.
///
/// This contains no password and is not authority without current session checks.
/// It is deliberately not Debug, Clone, Copy or automatically serializable.
///
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftRef;
/// fn no_diagnostics(reference: PasswordDraftRef) { println!("{reference:?}"); }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftRef;
/// fn no_copy(reference: PasswordDraftRef) { let _ = reference.clone(); }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftRef;
/// fn no_wire(reference: PasswordDraftRef) { let _ = serde_json::to_string(&reference); }
/// ```
pub struct PasswordDraftRef {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
    source: Fingerprint,
    identity: Arc<()>,
}

impl PasswordDraftRef {
    /// Session generation that created the draft, for targeted reply correlation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Authenticated committed revision against which generation was authorized.
    pub fn revision(&self) -> u64 {
        self.source.revision
    }
}

/// Explicit plaintext preview of the current generated draft, cleared on drop.
///
/// The trusted host retains the original reference/caller/opened context, rechecks
/// it before queued delivery, and clears its own copies at deadline, lock,
/// navigation or focus loss. This result does not supply an erasure timer.
/// No diagnostic, copying or automatic serialization traits are provided.
///
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftReveal;
/// fn no_diagnostics(reveal: PasswordDraftReveal) { println!("{reveal:?}"); }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftReveal;
/// fn no_copy(reveal: PasswordDraftReveal) { let _ = reveal.clone(); }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordDraftReveal;
/// fn no_wire(reveal: PasswordDraftReveal) { let _ = serde_json::to_string(&reveal); }
/// ```
pub struct PasswordDraftReveal {
    /// Authorizing generation; metadata alone is not authority.
    pub generation: u64,
    /// Authenticated revision associated with the draft.
    pub revision: u64,
    /// Generated password only, with no record fields or keys.
    pub password: Zeroizing<String>,
    /// Upper display bound, capped by current inactivity and 30 seconds.
    pub expires_at: Instant,
}

impl Session {
    /// Generate and retain one unsaved password with ordinary current access.
    /// Successful generation replaces the previous draft. Failure preserves it
    /// unless inactivity revokes the session. No storage or idle refresh occurs.
    pub fn generate_password_draft<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
        options: PasswordOptions,
    ) -> Result<PasswordDraftRef, TransactionError> {
        self.generate_draft_with(worker, caller, generation, expected_revision, || {
            generate_password(options)
        })
    }

    /// Explicitly reveal only the currently referenced generated password.
    /// The caller enforces delivery/display lifetime on already-returned copies.
    pub fn reveal_password_draft<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &PasswordDraftRef,
    ) -> Result<PasswordDraftReveal, TransactionError> {
        let Some(limit) = Instant::now().checked_add(PASSWORD_REVEAL_DURATION) else {
            self.lock();
            return Err(TransactionError::ClockUnavailable);
        };
        self.validate_password_draft(worker, caller, reference)?;
        let draft = self.password_draft.as_ref().expect("checked current draft");
        let password = copy_text(draft.password.as_str()).map_err(VaultError::from)?;
        if self.poll_inactivity() || Instant::now() >= limit {
            return Err(TransactionError::Locked);
        }
        let idle = self.inactivity_deadline().ok_or(TransactionError::Locked)?;
        Ok(PasswordDraftReveal {
            generation: reference.generation,
            revision: reference.source.revision,
            password,
            expires_at: limit.min(idle),
        })
    }

    /// Discard only the current referenced draft. This never undoes a save or
    /// resolves an uncertain write, and does not refresh inactivity.
    pub fn discard_password_draft<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &PasswordDraftRef,
    ) -> Result<(), TransactionError> {
        self.validate_password_draft(worker, caller, reference)?;
        self.password_draft = None;
        Ok(())
    }

    fn generate_draft_with<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
        generate: impl FnOnce() -> Result<GeneratedPassword, GenerationError>,
    ) -> Result<PasswordDraftRef, TransactionError> {
        let source = self.check_draft_access(worker, caller, generation, expected_revision)?;
        let result = generate();
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        let password = result.map_err(TransactionError::PasswordGeneration)?;
        let identity = Arc::new(());
        let reference = PasswordDraftRef {
            scope: self.scope.clone(),
            caller,
            generation,
            source,
            identity: identity.clone(),
        };
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        self.password_draft = Some(PasswordDraft { identity, password });
        Ok(reference)
    }

    fn check_draft_access<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
    ) -> Result<Fingerprint, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        if generation != self.generation || !matches!(self.access, Some(Access::Master(_))) {
            return Err(TransactionError::Locked);
        }
        worker.check_idle()?;
        let source = self.source.ok_or(TransactionError::ReloadRequired)?;
        if Some(source) != worker.fingerprint {
            return Err(TransactionError::ReloadRequired);
        }
        if expected_revision != source.revision {
            return Err(VaultError::RevisionConflict.into());
        }
        Ok(source)
    }

    /// Recheck current draft scope/state without returning another plaintext copy.
    /// Before delivering a queued preview, also check its original `expires_at`.
    pub fn validate_password_draft<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        reference: &PasswordDraftRef,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        if reference.caller != caller || !Arc::ptr_eq(&reference.scope, &self.scope) {
            return Err(TransactionError::Unauthorized);
        }
        let source = self.check_draft_access(
            worker,
            caller,
            reference.generation,
            reference.source.revision,
        )?;
        if source != reference.source
            || !self
                .password_draft
                .as_ref()
                .is_some_and(|draft| Arc::ptr_eq(&draft.identity, &reference.identity))
        {
            return Err(TransactionError::StalePasswordDraft);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
