//! Targeted credential replies and operation tracking for the desktop caller.
use super::*;
use codeward_core::transaction::{
    CredentialCursor, CredentialForm, CredentialPassword, PasswordDraftRef,
};
use serde::Deserialize;
use zeroize::Zeroize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListInput {
    pub epoch: u64,
    pub revocation: u64,
    pub cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selection {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Password {
    pub(super) mode: String,
    pub(super) value: Option<String>,
    pub(super) draft: Option<String>,
}
impl Drop for Password {
    fn drop(&mut self) {
        if let Some(value) = &mut self.value {
            value.zeroize();
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Form {
    pub(super) title: String,
    pub(super) username: String,
    pub(super) url: String,
    pub(super) notes: String,
    pub(super) password: Password,
}
impl Drop for Form {
    fn drop(&mut self) {
        self.title.zeroize();
        self.username.zeroize();
        self.url.zeroize();
        self.notes.zeroize();
    }
}
impl Form {
    fn borrowed<'a>(
        &'a self,
        generated: Option<&'a PasswordDraftRef>,
    ) -> Result<CredentialForm<'a>, SetupError> {
        if self.title.is_empty()
            || self.title.len() > 512
            || self.username.len() > 1024
            || self.url.len() > 4096
            || self.notes.len() > 65536
        {
            return Err(SetupError::InvalidInput);
        }
        let password = match (
            self.password.mode.as_str(),
            self.password.value.as_deref(),
            self.password.draft.as_deref(),
        ) {
            ("keep", None, None) => CredentialPassword::Keep,
            ("provided", Some(value), None) if value.len() <= 4096 => {
                CredentialPassword::Provided(value)
            }
            ("generated", None, Some(_)) => {
                CredentialPassword::Generated(generated.ok_or(SetupError::Stale)?)
            }
            _ => return Err(SetupError::InvalidInput),
        };
        Ok(CredentialForm {
            title: &self.title,
            username: &self.username,
            url: &self.url,
            notes: &self.notes,
            password,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriteInput {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub id: Option<String>,
    pub form: Form,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationInput {
    pub epoch: u64,
    pub operation: String,
}
#[derive(Serialize)]
pub(crate) struct Summary {
    #[serde(serialize_with = "serialize_secret")]
    pub(super) id: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) title: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) username: Zeroizing<String>,
}
#[derive(Serialize)]
pub(crate) struct Page {
    pub(super) epoch: u64,
    pub(super) revocation: u64,
    pub(super) generation: String,
    pub(super) revision: String,
    pub(super) summaries: Vec<Summary>,
    pub(super) next: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct Details {
    pub(super) epoch: u64,
    pub(super) revocation: u64,
    pub(super) generation: String,
    pub(super) revision: String,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) id: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) title: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) username: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) url: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) notes: Zeroizing<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}
#[derive(Serialize)]
pub(crate) struct Reveal {
    pub(super) epoch: u64,
    pub(super) revocation: u64,
    pub(super) generation: String,
    pub(super) revision: String,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) id: Zeroizing<String>,
    #[serde(serialize_with = "serialize_secret")]
    pub(super) password: Zeroizing<String>,
    pub(super) expires_in_ms: u64,
}
#[derive(Clone, Serialize)]
pub(crate) struct Operation {
    pub(super) epoch: u64,
    pub(super) revocation: u64,
    pub(super) operation: String,
    pub(super) state: &'static str,
}
#[derive(Default)]
pub(super) struct Credentials {
    cursor: Option<(String, CredentialCursor)>,
    serial: u64,
    pub(super) operation: Option<Operation>,
    draft: Option<(String, PasswordDraftRef)>,
}
fn decimal(value: &str) -> Result<u64, SetupError> {
    let number = value.parse::<u64>().map_err(|_| SetupError::InvalidInput)?;
    if number.to_string() != value {
        return Err(SetupError::InvalidInput);
    }
    Ok(number)
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    fn credential_scope(&mut self, epoch: u64, revocation: u64) -> Result<(), SetupError> {
        self.advance();
        self.check_epoch(epoch)?;
        if self.status.revocation != revocation || self.status.state != State::Unlocked {
            return Err(SetupError::Stale);
        }
        if self.uncertain || self.pending {
            return Err(SetupError::Busy);
        }
        Ok(())
    }
    pub(crate) fn credentials_list(
        &mut self,
        epoch: u64,
        revocation: u64,
        cursor: Option<&str>,
    ) -> Result<Page, SetupError> {
        self.credential_scope(epoch, revocation)?;
        let current = self.credentials.cursor.take();
        let continuation = match cursor {
            None => None,
            Some(token) => Some(
                &current
                    .as_ref()
                    .filter(|(key, _)| key == token)
                    .ok_or(SetupError::Stale)?
                    .1,
            ),
        };
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.list_credentials(host.generation(), 100, continuation));
        let page = result.map_err(|error| self.credential_error(error))?;
        self.credentials.serial = self
            .credentials
            .serial
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        let next = page.next.map(|cursor| {
            let token = format!("{}:{}:{}", epoch, revocation, self.credentials.serial);
            self.credentials.cursor = Some((token.clone(), cursor));
            token
        });
        Ok(Page {
            epoch,
            revocation,
            generation: page.generation.to_string(),
            revision: page.revision.to_string(),
            summaries: page
                .summaries
                .into_iter()
                .map(|s| Summary {
                    id: s.id,
                    title: s.title,
                    username: s.username,
                })
                .collect(),
            next,
        })
    }
    pub(crate) fn credentials_details(&mut self, input: Selection) -> Result<Details, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.get_credential_details(generation, revision, &input.id));
        let d = result.map_err(|error| self.credential_error(error))?;
        Ok(Details {
            epoch: input.epoch,
            revocation: input.revocation,
            generation: d.generation.to_string(),
            revision: d.revision.to_string(),
            id: d.id,
            title: d.title,
            username: d.username,
            url: d.url,
            notes: d.notes,
            created_at: d.created_at.to_string(),
            updated_at: d.updated_at.to_string(),
        })
    }
    pub(crate) fn credentials_reveal(&mut self, input: Selection) -> Result<Reveal, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.reveal_password(generation, revision, &input.id));
        let d = result.map_err(|error| self.credential_error(error))?;
        let remaining = d
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if remaining == 0 {
            return Err(SetupError::Stale);
        }
        Ok(Reveal {
            epoch: input.epoch,
            revocation: input.revocation,
            generation: d.generation.to_string(),
            revision: d.revision.to_string(),
            id: d.id,
            password: d.password,
            expires_in_ms: remaining,
        })
    }
    fn credential_error(&mut self, error: ControlError) -> SetupError {
        match error {
            ControlError::Host(HostError::Transaction(
                TransactionError::Vault(VaultError::RevisionConflict)
                | TransactionError::ReloadRequired
                | TransactionError::StaleCursor
                | TransactionError::StalePasswordDraft,
            )) => SetupError::Stale,
            ControlError::Host(HostError::Transaction(
                TransactionError::Busy | TransactionError::HistoryFull,
            )) => SetupError::Busy,
            ControlError::Host(HostError::Transaction(
                TransactionError::PasswordGeneration(
                    codeward_core::password::GenerationError::InvalidOptions,
                )
                | TransactionError::InvalidCredentialId
                | TransactionError::CredentialNotFound
                | TransactionError::Vault(_),
            )) => SetupError::InvalidInput,
            ControlError::Host(HostError::Transaction(TransactionError::PasswordGeneration(_))) => {
                SetupError::Unavailable
            }
            error => self.command_error(error),
        }
    }
    pub(super) fn credential_request(&mut self, revision: u64) -> Result<Request, SetupError> {
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        let mut id = [0; 16];
        id[..8].copy_from_slice(&self.status.epoch.to_le_bytes());
        id[8..].copy_from_slice(&self.next_request.to_le_bytes());
        Ok(Request {
            id: RequestId(id),
            expected_revision: revision,
        })
    }
    fn credentials_started(&mut self) -> Operation {
        self.pending = true;
        self.credentials.cursor = None;
        let operation = Operation {
            epoch: self.status.epoch,
            revocation: self.status.revocation,
            operation: self.next_request.to_string(),
            state: "saving",
        };
        self.credentials.operation = Some(operation.clone());
        operation
    }
    pub(crate) fn credentials_write(
        &mut self,
        input: WriteInput,
        update: bool,
    ) -> Result<Operation, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        if update != input.id.is_some() {
            return Err(SetupError::InvalidInput);
        }
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let request = self.credential_request(revision)?;
        let generated = if input.form.password.mode == "generated" {
            let token = input
                .form
                .password
                .draft
                .as_deref()
                .ok_or(SetupError::InvalidInput)?;
            Some(
                &self
                    .credentials
                    .draft
                    .as_ref()
                    .filter(|(key, _)| key == token)
                    .ok_or(SetupError::Stale)?
                    .1,
            )
        } else {
            None
        };
        let form = input.form.borrowed(generated)?;
        let mut queued = false;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                if let Some(id) = &input.id {
                    host.update_credential(generation, request, id, form)?;
                } else {
                    host.create_credential(generation, request, form)?;
                }
                queued = true;
                Ok(())
            });
        let operation = queued.then(|| self.credentials_started());
        result.map_err(|error| self.credential_error(error))?;
        operation.ok_or(SetupError::Unavailable)
    }
    pub(crate) fn credentials_delete(&mut self, input: Selection) -> Result<Operation, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let request = self.credential_request(revision)?;
        let mut queued = false;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                host.delete_credential(generation, request, &input.id)?;
                queued = true;
                Ok(())
            });
        let operation = queued.then(|| self.credentials_started());
        result.map_err(|error| self.credential_error(error))?;
        operation.ok_or(SetupError::Unavailable)
    }
    pub(crate) fn credentials_operation(
        &mut self,
        input: OperationInput,
    ) -> Result<Operation, SetupError> {
        self.advance();
        self.check_epoch(input.epoch)?;
        let mut operation = self
            .credentials
            .operation
            .as_ref()
            .filter(|o| o.operation == input.operation)
            .ok_or(SetupError::Stale)?
            .clone();
        operation.revocation = self.status.revocation;
        if self.status.state == State::Locked && operation.state == "saving" {
            operation.state = "locked";
        }
        Ok(operation)
    }
    pub(super) fn credential_completion(&mut self, completion: &HostCompletion) -> bool {
        let Some(operation) = &mut self.credentials.operation else {
            return false;
        };
        if operation.state != "saving" {
            return false;
        }
        match completion {
            HostCompletion::PreparationFailed { .. } => operation.state = "failed",
            HostCompletion::Transaction {
                outcome, refresh, ..
            } => {
                operation.state = match outcome {
                    Ok(OperationStatus::Committed { .. }) if matches!(refresh, Some(Ok(()))) => {
                        "saved"
                    }
                    Ok(OperationStatus::Committed { .. }) => "saved", // commit is authoritative even if refresh requires reopen
                    Ok(OperationStatus::OutcomeUnknown) | Err(TransactionError::OutcomeUnknown) => {
                        self.uncertain = true;
                        "unknown"
                    }
                    Ok(OperationStatus::Conflict) => "conflict",
                    Ok(OperationStatus::Canceled) => "locked",
                    _ => "failed",
                };
                if matches!(outcome, Ok(OperationStatus::Committed { .. }))
                    && !matches!(refresh, Some(Ok(())))
                {
                    if let Some(control) = &mut self.control {
                        control.lock();
                    }
                    self.status.state = State::Locked;
                }
            }
            _ => return false,
        }
        if operation.state == "saved" {
            self.credentials.draft = None;
        }
        self.pending = false;
        true
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirtyInput {
    pub epoch: u64,
    pub revocation: u64,
    pub dirty: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CloseInput {
    pub epoch: u64,
    pub revocation: u64,
    pub allow: bool,
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    pub(crate) fn set_dirty(&mut self, input: DirtyInput) -> Result<Status, SetupError> {
        self.advance();
        self.check_epoch(input.epoch)?;
        if self.status.revocation != input.revocation || self.status.state != State::Unlocked {
            return Err(SetupError::Stale);
        }
        self.dirty = input.dirty;
        Ok(self.status)
    }
    pub(crate) fn resolve_close(&mut self, input: &CloseInput) -> Result<Status, SetupError> {
        self.advance();
        self.check_epoch(input.epoch)?;
        if self.status.revocation != input.revocation || !self.status.close_requested {
            return Err(SetupError::Stale);
        }
        self.status.close_requested = false;
        if input.allow {
            self.dirty = false;
        }
        Ok(self.status)
    }
    pub(crate) fn request_close(&mut self) -> bool {
        self.advance();
        if self.dirty && self.status.state == State::Unlocked {
            self.status.close_requested = true;
            true
        } else {
            false
        }
    }
}

impl Credentials {
    pub(super) fn fail_pending(&mut self) -> bool {
        if let Some(operation) = &mut self.operation
            && operation.state == "saving"
        {
            operation.state = "unknown";
            true
        } else {
            false
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActivityInput {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub kind: String,
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    pub(crate) fn record_activity(&mut self, input: ActivityInput) -> Result<Status, SetupError> {
        use codeward_core::transaction::SessionActivity;
        self.advance();
        self.check_epoch(input.epoch)?;
        if self.status.revocation != input.revocation || self.status.state != State::Unlocked {
            return Err(SetupError::Stale);
        }
        let generation = decimal(&input.generation)?;
        let activity = match input.kind.as_str() {
            "keyboard" => SessionActivity::Keyboard,
            "pointer" => SessionActivity::Pointer,
            "assistive" => SessionActivity::AssistiveActivation,
            _ => return Err(SetupError::InvalidInput),
        };
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                if host.generation() != generation {
                    return Err(TransactionError::Locked.into());
                }
                let reference = host.activity_reference()?;
                host.record_activity(&reference, activity)
            });
        result.map_err(|error| self.credential_error(error))?;
        Ok(self.status)
    }
}

mod generation;
pub(crate) use generation::*;
