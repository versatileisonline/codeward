//! Fixed-order compact serialization through a capped plaintext writer.

use std::io::{self, Write};

use serde::{Serialize, Serializer, ser::SerializeStruct};
use zeroize::Zeroizing;

use super::{Credential, CredentialError, CredentialSnapshot};
use crate::crypto::MAX_PLAINTEXT_LENGTH;

impl CredentialSnapshot {
    /// Encode compact JSON with fixed field order and ascending record IDs.
    ///
    /// Returns owned plaintext cleared on drop, not ciphertext or a committed file.
    /// Enforces the complete payload limit as well as the model's field bounds.
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>, CredentialError> {
        let mut writer = PlaintextWriter::default();
        if serde_json::to_writer(&mut writer, &Snapshot(self)).is_err() {
            return Err(writer.failure.unwrap_or(CredentialError::InvalidSnapshot));
        }
        Ok(writer.bytes)
    }
}

// Private wrappers keep general-purpose Serde serialization off the public
// credential types. The application receives plaintext only through encode().
struct Snapshot<'a>(&'a CredentialSnapshot);
struct Records<'a>(&'a [Credential]);
struct Record<'a>(&'a Credential);

impl Serialize for Snapshot<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("CredentialSnapshot", 2)?;
        object.serialize_field("schema", &1_u8)?;
        object.serialize_field("credentials", &Records(self.0.credentials()))?;
        object.end()
    }
}

impl Serialize for Records<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(Record))
    }
}

impl Serialize for Record<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = self.0.fields();
        let mut object = serializer.serialize_struct("Credential", 8)?;
        for (name, value) in [
            ("id", fields.id),
            ("title", fields.title),
            ("username", fields.username),
            ("password", fields.password),
            ("url", fields.url),
            ("notes", fields.notes),
        ] {
            object.serialize_field(name, value)?;
        }
        object.serialize_field("created_at", &fields.created_at)?;
        object.serialize_field("updated_at", &fields.updated_at)?;
        object.end()
    }
}

#[derive(Default)]
struct PlaintextWriter {
    bytes: Zeroizing<Vec<u8>>,
    failure: Option<CredentialError>,
}

impl PlaintextWriter {
    fn reject(&mut self, error: CredentialError) -> io::Error {
        self.failure = Some(error);
        io::Error::other(error)
    }
}

impl Write for PlaintextWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.bytes.len().checked_add(bytes.len());
        let Some(end) = end.filter(|end| *end <= MAX_PLAINTEXT_LENGTH) else {
            return Err(self.reject(CredentialError::LimitExceeded));
        };
        if end > self.bytes.capacity() {
            let capacity = end
                .max(self.bytes.capacity().saturating_mul(2))
                .min(MAX_PLAINTEXT_LENGTH);
            let additional = capacity - self.bytes.len();
            if self.bytes.try_reserve_exact(additional).is_err() {
                return Err(self.reject(CredentialError::AllocationFailed));
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
