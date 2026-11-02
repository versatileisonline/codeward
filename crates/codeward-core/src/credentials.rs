//! Validated version 1 credential snapshots, independent of encryption and UI.
//!
//! These types establish schema validity only, never authentication or permission.
//! Owned text is cleared on drop. Borrowed inputs, caller copies and all allocator
//! or compiler temporaries are not guaranteed erased. No filesystem access occurs.

use std::{error::Error, fmt};

use zeroize::Zeroizing;

mod bounds;
mod decoding;

/// Maximum credentials in one version 1 snapshot.
pub const MAX_CREDENTIALS: usize = 10_000;
/// Largest accepted Unix millisecond timestamp (end of year 9999).
pub const MAX_TIMESTAMP: u64 = 253_402_300_799_999;

// title, username, password, URL and notes, in the normative schema order.
pub(super) const TEXT_LIMITS: [usize; 5] = [512, 1_024, 4_096, 4_096, 65_536];

/// Controlled failures without credential contents or library diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialError {
    /// Malformed JSON or a snapshot not matching the exact version 1 schema.
    InvalidSnapshot,
    /// Invalid record ID, text length or timestamp relationship.
    InvalidCredential,
    /// Two records have the same ID.
    DuplicateId,
    /// Snapshot size, depth or collection count exceeds the format limit.
    LimitExceeded,
    /// A bounded owned allocation could not be reserved.
    AllocationFailed,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSnapshot => "invalid credential snapshot",
            Self::InvalidCredential => "invalid credential fields",
            Self::DuplicateId => "duplicate credential ID",
            Self::LimitExceeded => "credential snapshot exceeds a format limit",
            Self::AllocationFailed => "credential working memory is unavailable",
        })
    }
}

impl Error for CredentialError {}

/// Borrowed record fields; callers own the lifetime/cleanup of their inputs.
///
/// IDs must already be 32 lowercase hex characters. ID generation and timestamps
/// are responsibilities of future authorized mutations, not this schema model.
pub struct CredentialInput<'a> {
    /// Opaque record ID, unique within its snapshot.
    pub id: &'a str,
    /// Nonempty title, at most 512 UTF-8 bytes.
    pub title: &'a str,
    /// Username, at most 1,024 UTF-8 bytes; empty is permitted.
    pub username: &'a str,
    /// Password, at most 4,096 UTF-8 bytes; empty is permitted.
    pub password: &'a str,
    /// Plain-text URL, at most 4,096 UTF-8 bytes; never fetched or normalized.
    pub url: &'a str,
    /// Notes, at most 65,536 UTF-8 bytes.
    pub notes: &'a str,
    /// Unix milliseconds within the accepted range.
    pub created_at: u64,
    /// Unix milliseconds, at least the creation timestamp.
    pub updated_at: u64,
}

impl CredentialInput<'_> {
    fn validate(&self) -> Result<(), CredentialError> {
        let text = [
            self.title,
            self.username,
            self.password,
            self.url,
            self.notes,
        ];
        if self.id.len() != 32
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.title.is_empty()
            || text.iter().zip(TEXT_LIMITS).any(|(s, max)| s.len() > max)
            || self.updated_at > MAX_TIMESTAMP
            || self.created_at > self.updated_at
        {
            return Err(CredentialError::InvalidCredential);
        }
        Ok(())
    }
}

/// Validated owned record. Deliberately not Debug, Clone or Copy.
pub struct Credential {
    id: Zeroizing<String>,
    title: Zeroizing<String>,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
    url: Zeroizing<String>,
    notes: Zeroizing<String>,
    created_at: u64,
    updated_at: u64,
}

impl Credential {
    /// Validate all bounds before copying inputs into owned zeroizing strings.
    ///
    /// Preserves user text exactly, including whitespace, Unicode and empty fields.
    pub fn new(input: CredentialInput<'_>) -> Result<Self, CredentialError> {
        input.validate()?;
        Ok(Self {
            id: copy_text(input.id)?,
            title: copy_text(input.title)?,
            username: copy_text(input.username)?,
            password: copy_text(input.password)?,
            url: copy_text(input.url)?,
            notes: copy_text(input.notes)?,
            created_at: input.created_at,
            updated_at: input.updated_at,
        })
    }

    /// Borrow this record's fields inside Rust; this is not a UI reply type.
    pub fn fields(&self) -> CredentialInput<'_> {
        CredentialInput {
            id: &self.id,
            title: &self.title,
            username: &self.username,
            password: &self.password,
            url: &self.url,
            notes: &self.notes,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Validated records kept in ascending ID order. Not an unlocked session.
pub struct CredentialSnapshot {
    credentials: Vec<Credential>,
}

impl CredentialSnapshot {
    /// Adopt validated records, enforce the count and reject duplicate IDs.
    ///
    /// Sorting needs no auxiliary record buffer. The caller owns any allocation
    /// performed before passing the vector; this constructor does not create IDs.
    pub fn new(mut credentials: Vec<Credential>) -> Result<Self, CredentialError> {
        if credentials.len() > MAX_CREDENTIALS {
            return Err(CredentialError::LimitExceeded);
        }
        credentials.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        if credentials.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(CredentialError::DuplicateId);
        }
        Ok(Self { credentials })
    }

    /// Borrow validated records for core operations, never a bulk UI response.
    pub fn credentials(&self) -> &[Credential] {
        &self.credentials
    }
}

fn copy_text(text: &str) -> Result<Zeroizing<String>, CredentialError> {
    let mut owned = Zeroizing::new(String::new());
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| CredentialError::AllocationFailed)?;
    owned.push_str(text);
    Ok(owned)
}

#[cfg(test)]
mod tests;
