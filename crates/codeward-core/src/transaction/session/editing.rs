//! Scoped credential forms and worker-side record creation/editing.

use super::*;
use super::{
    mutations::{snapshot_with_change, unix_millis},
    write::OwnedWrite,
};
use crate::credentials::{
    Credential, CredentialError, CredentialInput, CredentialSnapshot, MAX_CREDENTIALS, copy_text,
    valid_id, validate_text,
};
use std::time::SystemTime;

/// Explicit password intent for a single credential form; never a wire payload.
/// No Debug, Clone, Copy or automatic serialization is provided.
///
/// ```compile_fail
/// use codeward_core::transaction::CredentialPassword;
/// let input = CredentialPassword::Provided("synthetic");
/// let _ = serde_json::to_string(&input);
/// ```
pub enum CredentialPassword<'a> {
    /// Preserve an existing password without revealing it; invalid for creation.
    Keep,
    /// Replace with exact entered UTF-8, including an explicit empty password.
    Provided(&'a str),
    /// Use a current scoped generated draft without a plaintext UI round trip.
    Generated(&'a PasswordDraftRef),
}

/// Borrowed single-record input. Rust supplies identity and timestamps.
/// Caller-owned input copies still need cleanup; no form is persisted here.
///
/// ```compile_fail
/// use codeward_core::transaction::CredentialForm;
/// fn no_diagnostics(form: CredentialForm<'_>) { println!("{form:?}"); }
/// ```
/// ```compile_fail
/// use codeward_core::transaction::CredentialForm;
/// fn no_clone(form: CredentialForm<'_>) { let _ = form.clone(); }
/// ```
pub struct CredentialForm<'a> {
    /// Nonempty exact title, at most 512 UTF-8 bytes.
    pub title: &'a str,
    /// Exact username, at most 1,024 UTF-8 bytes; empty is permitted.
    pub username: &'a str,
    /// Keep, entered replacement, or scoped generated-password reference.
    pub password: CredentialPassword<'a>,
    /// Exact URL text, at most 4,096 UTF-8 bytes; never fetched or normalized.
    pub url: &'a str,
    /// Exact notes, at most 65,536 UTF-8 bytes.
    pub notes: &'a str,
}

pub(in crate::transaction) struct OwnedCredentialFields {
    title: Zeroizing<String>,
    username: Zeroizing<String>,
    url: Zeroizing<String>,
    notes: Zeroizing<String>,
    password: Option<Zeroizing<String>>,
}

impl OwnedCredentialFields {
    pub(super) fn validate(&self, creation: bool) -> Result<(), TransactionError> {
        if creation && self.password.is_none() {
            return Err(VaultError::from(CredentialError::InvalidCredential).into());
        }
        validate_text([
            &self.title,
            &self.username,
            self.password.as_ref().map_or("", |p| p.as_str()),
            &self.url,
            &self.notes,
        ])
        .map_err(VaultError::from)?;
        Ok(())
    }

    pub(super) fn intent(&self, field: &mut impl FnMut(&[u8])) {
        field(self.title.as_bytes());
        field(self.username.as_bytes());
        field(self.url.as_bytes());
        field(self.notes.as_bytes());
        field(&[u8::from(self.password.is_some())]);
        if let Some(password) = &self.password {
            field(password.as_bytes());
        }
    }

    fn record(
        &self,
        id: &str,
        created_at: u64,
        updated_at: u64,
        keep: Option<&str>,
    ) -> Result<Credential, TransactionError> {
        let password = self
            .password
            .as_ref()
            .map(|p| p.as_str())
            .or(keep)
            .ok_or(VaultError::from(CredentialError::InvalidCredential))?;
        Ok(Credential::new(CredentialInput {
            id,
            title: &self.title,
            username: &self.username,
            password,
            url: &self.url,
            notes: &self.notes,
            created_at,
            updated_at,
        })
        .map_err(VaultError::from)?)
    }
}

impl Session {
    /// Prepare one new credential. Rust assigns its ID and timestamps on the worker.
    /// Only a committed operation acknowledges saving; list after refresh for IDs.
    pub fn prepare_create_credential<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        generation: u64,
        request: Request,
        form: CredentialForm<'_>,
    ) -> Result<OperationRef, TransactionError> {
        let input = self.credential_form_input(worker, caller, generation, request, None, form)?;
        self.prepare_write(worker, caller, request, input)
    }

    /// Prepare full editing of one existing record; never insert a missing ID.
    /// Preserve identity/creation time; Keep avoids revealing the existing password.
    pub fn prepare_update_credential<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        generation: u64,
        request: Request,
        id: &str,
        form: CredentialForm<'_>,
    ) -> Result<OperationRef, TransactionError> {
        let input =
            self.credential_form_input(worker, caller, generation, request, Some(id), form)?;
        self.prepare_write(worker, caller, request, input)
    }

    pub(in crate::transaction) fn credential_form_input<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        request: Request,
        id: Option<&str>,
        form: CredentialForm<'_>,
    ) -> Result<OwnedWrite, TransactionError> {
        self.check_current_access(worker, caller, generation, request.expected_revision)?;
        if id.is_some_and(|id| !valid_id(id)) {
            return Err(TransactionError::InvalidCredentialId);
        }
        if let CredentialPassword::Generated(reference) = &form.password {
            self.validate_password_draft(worker, caller, reference)?;
        }
        let password = match &form.password {
            CredentialPassword::Keep if id.is_none() => {
                return Err(VaultError::from(CredentialError::InvalidCredential).into());
            }
            CredentialPassword::Provided(value) => *value,
            _ => "",
        };
        validate_text([form.title, form.username, password, form.url, form.notes])
            .map_err(VaultError::from)?;
        let result = (|| {
            let password = match form.password {
                CredentialPassword::Keep => None,
                CredentialPassword::Provided(value) => {
                    Some(copy_text(value).map_err(VaultError::from)?)
                }
                CredentialPassword::Generated(reference) => {
                    Some(self.copy_password_draft(worker, caller, reference)?)
                }
            };
            let fields = OwnedCredentialFields {
                title: copy_text(form.title).map_err(VaultError::from)?,
                username: copy_text(form.username).map_err(VaultError::from)?,
                url: copy_text(form.url).map_err(VaultError::from)?,
                notes: copy_text(form.notes).map_err(VaultError::from)?,
                password,
            };
            Ok(if let Some(id) = id {
                OwnedWrite::UpdateCredential {
                    id: copy_text(id).map_err(VaultError::from)?,
                    fields,
                }
            } else {
                OwnedWrite::CreateCredential(fields)
            })
        })();
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        result
    }
}

pub(super) fn created_snapshot(
    snapshot: &CredentialSnapshot,
    fields: &OwnedCredentialFields,
    permit: &Permit,
) -> Result<CredentialSnapshot, TransactionError> {
    created_snapshot_with(
        snapshot,
        fields,
        permit,
        || Ok(*crate::crypto::random_secret::<16>().map_err(VaultError::from)?),
        || unix_millis(SystemTime::now()),
    )
}

fn created_snapshot_with(
    snapshot: &CredentialSnapshot,
    fields: &OwnedCredentialFields,
    permit: &Permit,
    mut next_id: impl FnMut() -> Result<[u8; 16], TransactionError>,
    now: impl FnOnce() -> Result<u64, TransactionError>,
) -> Result<CredentialSnapshot, TransactionError> {
    if permit.canceled() {
        return Err(TransactionError::Locked);
    }
    if snapshot.credentials().len() >= MAX_CREDENTIALS {
        return Err(VaultError::from(CredentialError::LimitExceeded).into());
    }
    let mut selected = None;
    for _ in 0..8 {
        if permit.canceled() {
            return Err(TransactionError::Locked);
        }
        let random = next_id()?;
        let mut encoded = [0u8; 32];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (i, byte) in random.into_iter().enumerate() {
            encoded[i * 2] = HEX[usize::from(byte >> 4)];
            encoded[i * 2 + 1] = HEX[usize::from(byte & 15)];
        }
        let id = std::str::from_utf8(&encoded).expect("hex alphabet is ASCII");
        if snapshot
            .credentials()
            .binary_search_by(|r| r.fields().id.cmp(id))
            .is_err()
        {
            selected = Some(encoded);
            break;
        }
    }
    let id = selected.ok_or(VaultError::from(CredentialError::DuplicateId))?;
    if permit.canceled() {
        return Err(TransactionError::Locked);
    }
    let now = now()?;
    let record = fields.record(
        std::str::from_utf8(&id).expect("hex alphabet is ASCII"),
        now,
        now,
        None,
    )?;
    snapshot_with_change(snapshot, None, Some(record), permit)
}

pub(super) fn updated_snapshot(
    snapshot: &CredentialSnapshot,
    id: &str,
    fields: &OwnedCredentialFields,
    permit: &Permit,
) -> Result<CredentialSnapshot, TransactionError> {
    if permit.canceled() {
        return Err(TransactionError::Locked);
    }
    let selected = snapshot
        .credentials()
        .binary_search_by(|r| r.fields().id.cmp(id))
        .map_err(|_| TransactionError::CredentialNotFound)?;
    let previous = snapshot.credentials()[selected].fields();
    let now = unix_millis(SystemTime::now())?;
    let record = fields.record(
        id,
        previous.created_at,
        previous.updated_at.max(now),
        Some(previous.password),
    )?;
    snapshot_with_change(snapshot, Some(selected), Some(record), permit)
}

#[cfg(test)]
mod tests;
