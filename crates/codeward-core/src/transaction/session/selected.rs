use std::time::{Duration, Instant};

use super::*;
use crate::credentials::{CredentialInput, copy_text, valid_id};

/// Maximum display interval for an explicitly requested password reveal.
/// Hosts must clear their own copies at this deadline and on lock/navigation/focus loss.
pub const PASSWORD_REVEAL_DURATION: Duration = Duration::from_secs(30);

/// Fields of exactly one selected record, without its password.
///
/// Owned text clears on drop. Hosts retain caller/opened-vault context and recheck
/// generation/revision before delivering queued replies. These fields are not an
/// authorization token. No diagnostic, copying or serialization traits are provided.
///
/// ```compile_fail
/// use codeward_core::transaction::CredentialDetails;
/// fn no_password_in_details(details: CredentialDetails) {
///     let _password = details.password;
/// }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::CredentialDetails;
/// fn no_diagnostics(details: CredentialDetails) {
///     println!("{details:?}");
/// }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::CredentialDetails;
/// fn no_implicit_copy(details: CredentialDetails) {
///     let _copy = details.clone();
/// }
/// ```
pub struct CredentialDetails {
    /// Authorizing session generation; compare again before delivery.
    pub generation: u64,
    /// Authenticated committed revision read.
    pub revision: u64,
    /// Selected record identifier.
    pub id: Zeroizing<String>,
    /// Exact title, at most 512 UTF-8 bytes.
    pub title: Zeroizing<String>,
    /// Exact username, at most 1,024 UTF-8 bytes.
    pub username: Zeroizing<String>,
    /// Exact URL text, at most 4,096 UTF-8 bytes; never fetched.
    pub url: Zeroizing<String>,
    /// Selected record's notes, at most 65,536 UTF-8 bytes.
    pub notes: Zeroizing<String>,
    /// Creation time in Unix milliseconds.
    pub created_at: u64,
    /// Last update time in Unix milliseconds.
    pub updated_at: u64,
}

/// Explicit password reply for one selected record, owned only by trusted Rust.
///
/// `expires_at` is a delivery/display limit, not an erasure mechanism or timer.
/// Returned copies cannot be revoked by locking the session. The host must recheck
/// caller/opened-vault/generation/revision and time before delivery, and clear its
/// copies at deadline, lock, navigation or focus loss. No clipboard API is implied.
///
/// ```compile_fail
/// use codeward_core::transaction::PasswordReveal;
/// fn no_diagnostics(reveal: PasswordReveal) {
///     println!("{reveal:?}");
/// }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordReveal;
/// fn no_implicit_copy(reveal: PasswordReveal) {
///     let _copy = reveal.clone();
/// }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::PasswordReveal;
/// fn no_automatic_wire_payload(reveal: PasswordReveal) {
///     let _json = serde_json::to_string(&reveal);
/// }
/// ```
pub struct PasswordReveal {
    /// Authorizing session generation; compare again before delivery.
    pub generation: u64,
    /// Authenticated committed revision read.
    pub revision: u64,
    /// Selected record identifier.
    pub id: Zeroizing<String>,
    /// Exact selected password, at most 4,096 UTF-8 bytes; empty is permitted.
    pub password: Zeroizing<String>,
    /// Monotonic upper display bound, capped by the current inactivity deadline.
    pub expires_at: Instant,
}

impl Session {
    /// Read only non-password details of a selected record in ordinary access.
    /// Pass the current list/selection revision; stale selections are rejected.
    /// Pending/uncertain/conflicted writes and stale snapshots cannot supply data.
    /// Reading neither touches storage nor extends inactivity.
    pub fn get_credential_details<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
        id: &str,
    ) -> Result<CredentialDetails, TransactionError> {
        let (fields, revision) =
            self.selected_record(worker, caller, generation, expected_revision, id)?;
        let result = CredentialDetails {
            generation,
            revision,
            id: copy_text(fields.id).map_err(VaultError::from)?,
            title: copy_text(fields.title).map_err(VaultError::from)?,
            username: copy_text(fields.username).map_err(VaultError::from)?,
            url: copy_text(fields.url).map_err(VaultError::from)?,
            notes: copy_text(fields.notes).map_err(VaultError::from)?,
            created_at: fields.created_at,
            updated_at: fields.updated_at,
        };
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        Ok(result)
    }

    /// Explicitly reveal one current selected password to a trusted Rust caller.
    /// The host enforces returned-copy lifetime and presentation rules; the core
    /// cannot erase a copy already delivered. Reading does not refresh inactivity.
    pub fn reveal_password<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
        id: &str,
    ) -> Result<PasswordReveal, TransactionError> {
        let requested_at = Instant::now();
        let Some(limit) = requested_at.checked_add(PASSWORD_REVEAL_DURATION) else {
            self.lock();
            return Err(TransactionError::ClockUnavailable);
        };
        let (fields, revision) =
            self.selected_record(worker, caller, generation, expected_revision, id)?;
        let id = copy_text(fields.id).map_err(VaultError::from)?;
        let password = copy_text(fields.password).map_err(VaultError::from)?;
        if self.poll_inactivity() || Instant::now() >= limit {
            return Err(TransactionError::Locked);
        }
        let idle = self.inactivity_deadline().ok_or(TransactionError::Locked)?;
        Ok(PasswordReveal {
            generation,
            revision,
            id,
            password,
            expires_at: limit.min(idle),
        })
    }

    fn selected_record<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        expected_revision: u64,
        id: &str,
    ) -> Result<(CredentialInput<'_>, u64), TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        let Some(Access::Master(access)) = self.access.as_ref() else {
            return Err(TransactionError::Locked);
        };
        if generation != self.generation {
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
        if !valid_id(id) {
            return Err(TransactionError::InvalidCredentialId);
        }
        let records = access.snapshot().credentials();
        let index = records
            .binary_search_by(|record| record.fields().id.cmp(id))
            .map_err(|_| TransactionError::CredentialNotFound)?;
        Ok((records[index].fields(), source.revision))
    }
}

#[cfg(test)]
mod tests;
