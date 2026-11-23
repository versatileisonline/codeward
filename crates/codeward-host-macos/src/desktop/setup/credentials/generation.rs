//! Native ownership of generated drafts; the renderer receives opaque references.
use super::*;
use codeward_core::password::PasswordOptions;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratorOptions {
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenerateInput {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub options: GeneratorOptions,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DraftInput {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub draft: String,
}
#[derive(Serialize)]
pub(crate) struct DraftReply {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub draft: String,
}
#[derive(Serialize)]
pub(crate) struct DraftReveal {
    pub epoch: u64,
    pub revocation: u64,
    pub generation: String,
    pub revision: String,
    pub draft: String,
    #[serde(serialize_with = "serialize_secret")]
    pub password: Zeroizing<String>,
    pub expires_in_ms: u64,
}
impl Credentials {
    pub(crate) fn clear_draft(&mut self) {
        self.draft = None;
    }
}
impl<L: WriterLease + Send + 'static, E: HeadlessEventSource> Setup<L, E> {
    pub(crate) fn generate_draft(
        &mut self,
        input: GenerateInput,
    ) -> Result<DraftReply, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let serial = self
            .credentials
            .serial
            .checked_add(1)
            .ok_or(SetupError::Unavailable)?;
        let options = PasswordOptions {
            length: input.options.length,
            lowercase: input.options.lowercase,
            uppercase: input.options.uppercase,
            digits: input.options.digits,
            symbols: input.options.symbols,
        };
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.generate_password_draft(generation, revision, options));
        let reference = result.map_err(|e| self.credential_error(e))?;
        let token = format!("{}:{}:{}", input.epoch, input.revocation, serial);
        self.credentials.serial = serial;
        self.credentials.draft = Some((token.clone(), reference));
        Ok(DraftReply {
            epoch: input.epoch,
            revocation: input.revocation,
            generation: input.generation,
            revision: input.revision,
            draft: token,
        })
    }
    pub(crate) fn reveal_draft(&mut self, input: DraftInput) -> Result<DraftReveal, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let reference = &self
            .credentials
            .draft
            .as_ref()
            .filter(|(token, r)| {
                token == &input.draft && r.generation() == generation && r.revision() == revision
            })
            .ok_or(SetupError::Stale)?
            .1;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| {
                let reveal = host.reveal_password_draft(reference)?;
                host.validate_password_draft(reference)?;
                Ok(reveal)
            });
        let reveal = result.map_err(|e| self.credential_error(e))?;
        let remaining = reveal
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if remaining == 0 {
            return Err(SetupError::Stale);
        }
        Ok(DraftReveal {
            epoch: input.epoch,
            revocation: input.revocation,
            generation: input.generation,
            revision: input.revision,
            draft: input.draft,
            password: reveal.password,
            expires_in_ms: remaining,
        })
    }
    pub(crate) fn discard_draft(&mut self, input: DraftInput) -> Result<DraftReply, SetupError> {
        self.credential_scope(input.epoch, input.revocation)?;
        let generation = decimal(&input.generation)?;
        let revision = decimal(&input.revision)?;
        let reference = &self
            .credentials
            .draft
            .as_ref()
            .filter(|(token, r)| {
                token == &input.draft && r.generation() == generation && r.revision() == revision
            })
            .ok_or(SetupError::Stale)?
            .1;
        let result = self
            .control
            .as_mut()
            .ok_or(SetupError::Unavailable)?
            .command(|host| host.discard_password_draft(reference));
        result.map_err(|e| self.credential_error(e))?;
        self.credentials.draft = None;
        Ok(DraftReply {
            epoch: input.epoch,
            revocation: input.revocation,
            generation: input.generation,
            revision: input.revision,
            draft: input.draft,
        })
    }
}
