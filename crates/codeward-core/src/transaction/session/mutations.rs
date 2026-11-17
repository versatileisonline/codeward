//! Targeted existing-record mutations, with snapshot work kept in Rust.

use super::write::OwnedWrite;
use super::*;
use crate::credentials::{Credential, CredentialSnapshot, MAX_TIMESTAMP, copy_text, valid_id};
use std::time::{SystemTime, UNIX_EPOCH};

impl Session {
    /// Prepare replacement of one existing credential's password from a current
    /// generated draft. No complete snapshot or password input comes from the UI.
    /// This synchronous Rust entry point does not commit; hosts use the worker path.
    pub fn prepare_generated_password<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        id: &str,
        reference: &PasswordDraftRef,
    ) -> Result<OperationRef, TransactionError> {
        let input = self.generated_password_input(worker, caller, request, id, reference)?;
        self.prepare_write(worker, caller, request, input)
    }

    /// Prepare explicit deletion of one existing credential at the selected
    /// generation/revision. Missing records are never recreated; no undo is implied.
    /// The returned operation is pending until the full commit protocol completes.
    pub fn prepare_delete_credential<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        generation: u64,
        request: Request,
        id: &str,
    ) -> Result<OperationRef, TransactionError> {
        let input = self.delete_credential_input(worker, caller, generation, request, id)?;
        self.prepare_write(worker, caller, request, input)
    }

    pub(in crate::transaction) fn delete_credential_input<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        request: Request,
        id: &str,
    ) -> Result<OwnedWrite, TransactionError> {
        self.check_current_access(worker, caller, generation, request.expected_revision)?;
        if !valid_id(id) {
            return Err(TransactionError::InvalidCredentialId);
        }
        let result = copy_text(id).map_err(VaultError::from);
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        Ok(OwnedWrite::DeleteCredential { id: result? })
    }
}

pub(super) fn unix_millis(now: SystemTime) -> Result<u64, TransactionError> {
    let value = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TransactionError::ClockUnavailable)?
        .as_millis();
    let value = u64::try_from(value).map_err(|_| TransactionError::ClockUnavailable)?;
    if value > MAX_TIMESTAMP {
        return Err(TransactionError::ClockUnavailable);
    }
    Ok(value)
}

pub(super) fn changed_snapshot(
    snapshot: &CredentialSnapshot,
    id: &str,
    replacement: Option<(&str, u64)>,
    permit: &Permit,
) -> Result<CredentialSnapshot, TransactionError> {
    if permit.canceled() {
        return Err(TransactionError::Locked);
    }
    let records = snapshot.credentials();
    let selected = records
        .binary_search_by(|record| record.fields().id.cmp(id))
        .map_err(|_| TransactionError::CredentialNotFound)?;
    let mut changed = Vec::new();
    changed
        .try_reserve_exact(records.len() - usize::from(replacement.is_none()))
        .map_err(|_| TransactionError::AllocationFailed)?;
    for (index, record) in records.iter().enumerate() {
        if permit.canceled() {
            return Err(TransactionError::Locked);
        }
        let mut fields = record.fields();
        if index == selected {
            let Some((password, now)) = replacement else {
                continue;
            };
            fields.password = password;
            fields.updated_at = fields.updated_at.max(now);
        }
        changed.push(Credential::new(fields).map_err(VaultError::from)?);
    }
    Ok(CredentialSnapshot::new(changed).map_err(VaultError::from)?)
}

#[cfg(test)]
mod tests;
