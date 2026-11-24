//! Restricted paper authentication and authoritative replacement outcomes.
use super::*;
use codeward_core::transaction::OperationRef;
use serde::Deserialize;
use zeroize::Zeroize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PaperInput {
    pub epoch: u64,
    pub revocation: u64,
    pub paper: String,
}
impl Drop for PaperInput {
    fn drop(&mut self) {
        self.paper.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplacementInput {
    pub epoch: u64,
    pub revocation: u64,
    pub password: String,
    pub confirmation: String,
}
impl Drop for ReplacementInput {
    fn drop(&mut self) {
        self.password.zeroize();
        self.confirmation.zeroize();
    }
}
#[derive(Default)]
pub(super) struct Recovery {
    pub(super) revision: Option<u64>,
    operation: Option<OperationRef>,
}
impl Recovery {
    pub(super) fn resumable(&self, state: State) -> bool {
        matches!(
            state,
            State::RecoverySaving | State::RecoveryUnknown | State::RecoveryChecking
        )
    }
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    fn recovery_scope(&mut self, epoch: u64, revocation: u64) -> Result<(), SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if !self.existing || self.status.revocation != revocation {
            return Err(SetupError::Stale);
        }
        Ok(())
    }
    pub(crate) fn begin_recovery(
        &mut self,
        epoch: u64,
        revocation: u64,
        paper: &str,
    ) -> Result<Status, SetupError> {
        self.recovery_scope(epoch, revocation)?;
        if self.pending
            || self.uncertain
            || !matches!(self.status.state, State::Locked | State::RecoveryFailed)
        {
            return Err(SetupError::Stale);
        }
        if paper.is_empty() || paper.len() > 256 {
            return Err(SetupError::InvalidCode);
        }
        self.recovery = Recovery::default();
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.begin_authentication(AccessProof::Paper(paper)));
        result.map_err(|error| self.recovery_error(error, false))?;
        self.pending = true;
        self.set(State::Recovering, None);
        Ok(self.status)
    }
    pub(crate) fn complete_recovery(
        &mut self,
        epoch: u64,
        revocation: u64,
        password: &str,
        confirmation: &str,
    ) -> Result<Status, SetupError> {
        self.recovery_scope(epoch, revocation)?;
        if self.pending || self.uncertain || self.status.state != State::RecoveryReady {
            return Err(SetupError::Stale);
        }
        if password.is_empty() || password.len() > 4096 || password != confirmation {
            return Err(SetupError::InvalidInput);
        }
        let revision = self.recovery.revision.ok_or(SetupError::Stale)?;
        let request = self.credential_request(revision)?;
        let mut queued = false;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                host.replace_master_password(request, password, confirmation)?;
                queued = true;
                Ok(())
            });
        if queued {
            self.pending = true;
            self.recovery.revision = None;
            self.set(State::RecoverySaving, None);
        }
        if let Err(error) = result {
            if queued {
                // The core may have admitted a write before its second event
                // check revoked access. Keep polling its authoritative outcome.
                self.status.revocation = self.status.revocation.saturating_add(1);
                return Err(SetupError::Stale);
            }
            return Err(self.recovery_error(error, false));
        }
        Ok(self.status)
    }
    pub(crate) fn reconcile_recovery(
        &mut self,
        epoch: u64,
        revocation: u64,
        paper: &str,
    ) -> Result<Status, SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if self.status.state == State::RecoveryUnknown
            && self.status.error == Some(SetupError::Unavailable)
        {
            return Err(SetupError::Unavailable);
        }
        self.recovery_scope(epoch, revocation)?;
        if self.pending || !self.uncertain || self.status.state != State::RecoveryUnknown {
            return Err(SetupError::Stale);
        }
        if paper.is_empty() || paper.len() > 256 {
            return Err(SetupError::InvalidCode);
        }
        let operation = self
            .recovery
            .operation
            .as_ref()
            .ok_or(SetupError::Unavailable)?
            .clone();
        let mut queued = false;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                host.reconcile(&operation, AccessProof::Paper(paper))?;
                queued = true;
                Ok(())
            });
        if queued {
            self.pending = true;
            self.set(State::RecoveryChecking, None);
        }
        if let Err(error) = result {
            if queued {
                self.status.revocation = self.status.revocation.saturating_add(1);
                return Err(SetupError::Stale);
            }
            return Err(self.recovery_error(error, true));
        }
        Ok(self.status)
    }
    fn recovery_error(&mut self, error: ControlError, unknown: bool) -> SetupError {
        let invalid = matches!(
            error,
            ControlError::Host(HostError::Transaction(
                TransactionError::Vault(VaultError::Recovery(_))
                    | TransactionError::Vault(VaultError::Crypto(
                        codeward_core::crypto::CryptoError::AuthenticationFailed
                    ))
            ))
        );
        self.recovery.revision = None;
        self.status.revocation = self.status.revocation.saturating_add(1);
        if let Some(control) = &mut self.control {
            control.lock();
        }
        let controlled = if invalid {
            SetupError::InvalidCode
        } else if matches!(
            error,
            ControlError::Revoked
                | ControlError::Host(HostError::Transaction(TransactionError::Locked))
        ) {
            SetupError::Stale
        } else {
            SetupError::Unavailable
        };
        self.set(
            if unknown {
                State::RecoveryUnknown
            } else {
                State::Locked
            },
            Some(controlled),
        );
        controlled
    }
    pub(super) fn recovery_completion(
        &mut self,
        completion: &HostCompletion,
        revoked: bool,
    ) -> bool {
        if self.status.state == State::Recovering {
            let HostCompletion::Authentication(outcome) = completion else {
                return false;
            };
            self.pending = false;
            if matches!(outcome, Ok(())) && !revoked {
                let result = self
                    .control
                    .as_mut()
                    .unwrap()
                    .command(|host| host.recovery_revision());
                match result {
                    Ok(revision) => {
                        self.recovery.revision = Some(revision);
                        self.set(State::RecoveryReady, None);
                    }
                    Err(error) => {
                        self.recovery_error(error, false);
                    }
                }
            } else {
                let error = match outcome {
                    Err(TransactionError::Vault(VaultError::Recovery(_)))
                    | Err(TransactionError::Vault(VaultError::Crypto(
                        codeward_core::crypto::CryptoError::AuthenticationFailed,
                    ))) => Some(SetupError::InvalidCode),
                    _ if revoked => None,
                    _ => Some(SetupError::Unavailable),
                };
                self.set(State::Locked, error);
            }
            return true;
        }
        if !matches!(
            self.status.state,
            State::RecoverySaving | State::RecoveryChecking
        ) {
            return false;
        }
        let checking = self.status.state == State::RecoveryChecking;
        let (state, error) = match completion {
            HostCompletion::PreparationFailed { .. } => {
                (State::RecoveryFailed, Some(SetupError::Storage))
            }
            HostCompletion::Transaction {
                operation, outcome, ..
            } => {
                self.recovery.operation = Some(operation.clone());
                match outcome {
                    Ok(OperationStatus::Committed { .. }) => (State::RecoverySaved, None),
                    Ok(OperationStatus::OutcomeUnknown) | Err(TransactionError::OutcomeUnknown) => {
                        (State::RecoveryUnknown, None)
                    }
                    Err(TransactionError::Vault(VaultError::Recovery(_)))
                    | Err(TransactionError::Vault(VaultError::Crypto(
                        codeward_core::crypto::CryptoError::AuthenticationFailed,
                    ))) if checking => (State::RecoveryUnknown, Some(SetupError::InvalidCode)),
                    Err(_) if checking => (State::RecoveryUnknown, Some(SetupError::Storage)),
                    _ => (State::RecoveryFailed, Some(SetupError::Storage)),
                }
            }
            _ => return false,
        };
        self.pending = false;
        self.uncertain = state == State::RecoveryUnknown;
        self.recovery.revision = None;
        self.status.revocation = self.status.revocation.saturating_add(1);
        if let Some(control) = &mut self.control {
            control.lock();
        }
        self.set(state, error);
        true
    }
}
