//! Control-thread enrollment bridge. Storage and cryptography remain in the core.
use std::time::{Duration, Instant};

use codeward_core::{
    transaction::{
        AccessProof, CallerId, ControlError, HeadlessControlLoop, HeadlessEventSource,
        HostCompletion, HostError, OperationStatus, Request, RequestId, Storage, TransactionError,
        VaultOpenMode, WriterLease,
    },
    vault::VaultError,
};
use serde::{Serialize, Serializer};
use zeroize::Zeroizing;

pub(crate) mod credentials;
pub(crate) mod recovery;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Idle,
    Opening,
    Recovery,
    Saving,
    Created,
    Locked,
    Failed,
    Existing,
    Missing,
    Unlocking,
    Recovering,
    RecoveryReady,
    RecoverySaving,
    RecoverySaved,
    RecoveryUnknown,
    RecoveryChecking,
    RecoveryFailed,
    Unlocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupError {
    InvalidInput,
    InvalidCode,
    WrongPassword,
    Busy,
    Storage,
    Unavailable,
    Stale,
}

#[derive(Clone, Copy, Serialize)]
pub(super) struct Status {
    pub epoch: u64,
    pub state: State,
    pub error: Option<SetupError>,
    pub revocation: u64,
    pub close_requested: bool,
}

#[derive(Serialize)]
pub(super) struct CodeReply {
    pub epoch: u64,
    #[serde(serialize_with = "serialize_secret")]
    code: Zeroizing<String>,
}
fn serialize_secret<S: Serializer>(
    secret: &Zeroizing<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    secret.as_str().serialize(serializer)
}

pub(super) struct Setup<L: WriterLease + Send + 'static, E: HeadlessEventSource> {
    control: Option<HeadlessControlLoop<L, E>>,
    password: Option<Zeroizing<String>>,
    status: Status,
    pending: bool,
    uncertain: bool,
    next_request: u64,
    existing: bool,
    credentials: credentials::Credentials,
    dirty: bool,
    recovery: recovery::Recovery,
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    pub fn new() -> Self {
        Self {
            control: None,
            password: None,
            status: Status {
                epoch: 0,
                state: State::Idle,
                error: None,
                revocation: 0,
                close_requested: false,
            },
            pending: false,
            uncertain: false,
            next_request: 0,
            existing: false,
            credentials: credentials::Credentials::default(),
            dirty: false,
            recovery: recovery::Recovery::default(),
        }
    }
    pub fn begin<S: Storage<Lease = L>>(
        &mut self,
        password: &str,
        confirmation: &str,
        source: E,
        storage: impl FnOnce() -> Result<S, TransactionError> + Send + 'static,
    ) -> Result<Status, SetupError> {
        self.advance();
        if self.pending || self.uncertain || self.status.state == State::Recovery {
            return Err(SetupError::Busy);
        }
        if password.is_empty() || password.len() > 4096 || password != confirmation {
            return Err(SetupError::InvalidInput);
        }
        self.status.epoch = self
            .status
            .epoch
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        if let Some(control) = self.control.take() {
            control.close();
        }
        self.credentials = credentials::Credentials::default();
        self.recovery = recovery::Recovery::default();
        self.existing = false;
        self.password = Some(Zeroizing::new(password.to_owned()));
        let result = HeadlessControlLoop::begin_open(
            source,
            CallerId(2),
            VaultOpenMode::Enrollment,
            storage,
        );
        match result {
            Ok(control) => {
                self.control = Some(control);
                self.pending = true;
                self.set(State::Opening, None);
            }
            Err(_) => {
                self.password.take();
                self.set(State::Locked, Some(SetupError::Unavailable));
            }
        }
        Ok(self.status)
    }
    pub fn open_existing<S: Storage<Lease = L>>(
        &mut self,
        source: E,
        storage: impl FnOnce() -> Result<S, TransactionError> + Send + 'static,
    ) -> Result<Status, SetupError> {
        self.advance();
        if self.recovery.resumable(self.status.state)
            || matches!(
                self.status.state,
                State::RecoverySaved | State::RecoveryFailed
            )
        {
            return Ok(self.status);
        }
        if self.pending || self.uncertain || self.status.state == State::Recovery {
            return Err(SetupError::Busy);
        }
        self.status.epoch = self
            .status
            .epoch
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        if let Some(control) = self.control.take() {
            control.close();
        }
        self.password.take();
        self.credentials = credentials::Credentials::default();
        self.recovery = recovery::Recovery::default();
        self.existing = true;
        match HeadlessControlLoop::begin_open(source, CallerId(2), VaultOpenMode::Existing, storage)
        {
            Ok(control) => {
                self.control = Some(control);
                self.pending = true;
                self.set(State::Opening, None);
            }
            Err(_) => self.set(State::Failed, Some(SetupError::Unavailable)),
        }
        Ok(self.status)
    }
    pub fn unlock(
        &mut self,
        epoch: u64,
        revocation: u64,
        password: &str,
    ) -> Result<Status, SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if !self.existing
            || self.status.state != State::Locked
            || self.pending
            || revocation != self.status.revocation
        {
            return Err(SetupError::Stale);
        }
        if password.is_empty() || password.len() > 4096 {
            return Err(SetupError::InvalidInput);
        }
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.begin_authentication(AccessProof::Master(password)));
        if let Err(error) = result {
            // Authentication is revoked/discarded by the core at a failed
            // command boundary. It cannot become an active session later.
            self.pending = false;
            return Err(self.command_error(error));
        }
        self.pending = true;
        self.set(State::Unlocking, None);
        Ok(self.status)
    }
    pub fn lock_existing(&mut self, epoch: u64) -> Result<Status, SetupError> {
        self.check_epoch(epoch)?;
        if !self.existing
            || !matches!(
                self.status.state,
                State::Locked
                    | State::Unlocking
                    | State::Unlocked
                    | State::Recovering
                    | State::RecoveryReady
                    | State::RecoverySaved
                    | State::RecoveryFailed
            )
        {
            return Err(SetupError::Stale);
        }
        self.control.as_mut().ok_or(SetupError::Unavailable)?.lock();
        self.pending = false;
        self.recovery.revision = None;
        self.status.revocation = self.status.revocation.saturating_add(1);
        self.set(State::Locked, None);
        Ok(self.status)
    }
    pub fn status(&mut self) -> Status {
        self.advance();
        self.status
    }
    pub fn next_tick(&self) -> Option<Instant> {
        self.control
            .as_ref()
            .map(|c| Instant::now() + c.next_poll_in(Duration::from_millis(250)))
    }
    pub fn code(&mut self, epoch: u64) -> Result<CodeReply, SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if self.status.state != State::Recovery {
            return Err(SetupError::Stale);
        }
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.recovery_code());
        let code = result.map_err(|error| self.command_error(error))?;
        Ok(CodeReply { epoch, code })
    }
    pub fn confirm(&mut self, epoch: u64, paper: &str) -> Result<Status, SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if self.status.state != State::Recovery {
            return Err(SetupError::Busy);
        }
        if paper.len() > 256 {
            return Err(SetupError::InvalidCode);
        }
        let password = self.password.as_ref().ok_or(SetupError::Stale)?;
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        let mut id = [0; 16];
        id[..8].copy_from_slice(&epoch.to_le_bytes());
        id[8..].copy_from_slice(&self.next_request.to_le_bytes());
        let mut queued = false;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                host.create(
                    Request {
                        id: RequestId(id),
                        expected_revision: 0,
                    },
                    password,
                    password,
                    paper,
                )?;
                queued = true;
                Ok(())
            });
        self.pending = queued;
        result.map_err(|error| self.command_error(error))?;
        self.set(State::Saving, None);
        Ok(self.status)
    }
    pub fn cancel(&mut self, epoch: u64) -> Result<Status, SetupError> {
        self.check_epoch(epoch)?;
        self.status.revocation = self.status.revocation.saturating_add(1);
        if matches!(
            self.status.state,
            State::Opening | State::Unlocking | State::Recovering
        ) {
            self.pending = false;
        }
        if let Some(control) = &mut self.control {
            control.lock();
        }
        self.password.take();
        self.recovery.revision = None;
        if !self.recovery.resumable(self.status.state)
            && !self.uncertain
            && self.status.state != State::Created
        {
            self.set(State::Locked, None);
        }
        if self.status.state == State::Locked
            && !self.pending
            && let Some(control) = self.control.take()
        {
            control.close();
        }
        Ok(self.status)
    }
    fn command_error(&mut self, error: ControlError) -> SetupError {
        if matches!(
            error,
            ControlError::Host(HostError::Transaction(TransactionError::Vault(
                VaultError::Recovery(_)
            )))
        ) {
            self.set(State::Recovery, Some(SetupError::InvalidCode));
            return SetupError::InvalidCode;
        }
        self.password.take();
        self.status.revocation = self.status.revocation.saturating_add(1);
        if let Some(control) = &mut self.control {
            control.lock();
        }
        if matches!(
            error,
            ControlError::Revoked
                | ControlError::EventSourceUnavailable
                | ControlError::Host(HostError::Transaction(TransactionError::Locked))
        ) {
            self.set(State::Locked, None);
            SetupError::Stale
        } else {
            self.set(State::Failed, Some(SetupError::Unavailable));
            SetupError::Unavailable
        }
    }
    fn check_epoch(&self, epoch: u64) -> Result<(), SetupError> {
        if epoch != self.status.epoch {
            Err(SetupError::Stale)
        } else {
            Ok(())
        }
    }
    fn set(&mut self, state: State, error: Option<SetupError>) {
        self.status.state = state;
        if state != State::Unlocked {
            self.credentials.clear_draft();
            self.credentials.clear_read_access();
            self.dirty = false;
            self.status.close_requested = false;
        }
        self.status.error = error;
    }
    pub fn advance(&mut self) {
        let Some(control) = &mut self.control else {
            return;
        };
        let update = match control.step(Duration::ZERO) {
            Ok(update) => update,
            Err(_) => {
                self.password.take();
                self.pending = false;
                let recovery_write = self.recovery.resumable(self.status.state);
                if recovery_write {
                    if let Some(control) = &mut self.control {
                        control.lock();
                    }
                    self.status.revocation = self.status.revocation.saturating_add(1);
                    self.recovery.revision = None;
                    self.uncertain = true;
                    self.set(State::RecoveryUnknown, Some(SetupError::Unavailable));
                    return;
                }
                let writing = self.credentials.fail_pending();
                self.uncertain = !self.existing || writing;
                self.set(
                    if self.existing && !writing {
                        State::Failed
                    } else {
                        State::Unknown
                    },
                    Some(SetupError::Unavailable),
                );
                return;
            }
        };
        if update.revoked || !update.source_available {
            self.status.revocation = self.status.revocation.saturating_add(1);
            self.password.take();
            let opening_revoked = self.status.state == State::Opening;
            if matches!(
                self.status.state,
                State::Opening | State::Unlocking | State::Recovering
            ) {
                self.pending = false;
            }
            self.recovery.revision = None;
            if !update.source_available && self.status.state == State::RecoveryUnknown {
                self.status.error = Some(SetupError::Unavailable);
            }
            if !self.recovery.resumable(self.status.state)
                && !self.uncertain
                && !matches!(
                    self.status.state,
                    State::Created | State::RecoverySaved | State::RecoveryFailed
                )
            {
                self.set(
                    if self.existing && (!update.source_available || opening_revoked) {
                        State::Failed
                    } else {
                        State::Locked
                    },
                    None,
                );
            }
        }
        if let Some(opening) = update.opening {
            self.pending = false;
            match opening {
                Ok(()) => self.set(
                    if self.existing {
                        State::Locked
                    } else {
                        State::Recovery
                    },
                    None,
                ),
                Err(HostError::Transaction(TransactionError::ReloadRequired)) => {
                    self.password.take();
                    self.set(
                        if self.existing {
                            State::Missing
                        } else {
                            State::Existing
                        },
                        None,
                    );
                }
                Err(_) => {
                    self.password.take();
                    self.set(State::Failed, Some(SetupError::Storage));
                }
            }
        }
        if let Some(completion) = update.completion {
            if self.recovery_completion(&completion, update.revoked || !update.source_available) {
                if self.status.state == State::RecoveryUnknown && !update.source_available {
                    self.status.error = Some(SetupError::Unavailable);
                }
                return;
            }
            if self.credential_completion(&completion) {
                return;
            }
            match completion {
                HostCompletion::Authentication(outcome) if self.existing => {
                    self.pending = false;
                    match outcome {
                        Ok(()) if !update.revoked && update.source_available => {
                            self.set(State::Unlocked, None)
                        }
                        Err(TransactionError::Vault(VaultError::Crypto(
                            codeward_core::crypto::CryptoError::AuthenticationFailed,
                        ))) => self.set(State::Locked, Some(SetupError::WrongPassword)),
                        _ => self.set(State::Locked, Some(SetupError::Unavailable)),
                    }
                }
                HostCompletion::PreparationFailed {
                    error: TransactionError::Vault(VaultError::Recovery(_)),
                    ..
                } if self.password.is_some() && self.status.state == State::Saving => {
                    // Invalid transcription does not consume enrollment. Retrying
                    // preparation before an operation exists is explicit and safe.
                    self.pending = false;
                    self.set(State::Recovery, Some(SetupError::InvalidCode));
                }
                HostCompletion::Transaction {
                    outcome: Ok(OperationStatus::Committed { .. }),
                    ..
                } => {
                    self.password.take();
                    self.pending = false;
                    self.set(State::Created, None);
                    if let Some(control) = &mut self.control {
                        control.lock();
                    }
                }
                HostCompletion::Transaction {
                    outcome: Ok(OperationStatus::OutcomeUnknown),
                    ..
                }
                | HostCompletion::Transaction {
                    outcome: Err(TransactionError::OutcomeUnknown),
                    ..
                } => {
                    self.password.take();
                    self.pending = false;
                    self.uncertain = true;
                    self.set(State::Unknown, None);
                }
                _ => {
                    self.password.take();
                    self.pending = false;
                    self.set(State::Failed, Some(SetupError::Storage));
                }
            }
        }
        if matches!(
            self.status.state,
            State::Created | State::Failed | State::Unknown
        ) && let Some(control) = &mut self.control
        {
            control.lock();
        }
    }
}

#[cfg(test)]
mod tests;
