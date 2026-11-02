//! Schema visitors retain duplicates and borrow raw values until bounds pass.

use std::fmt;

use serde::de::{self, Deserialize, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use zeroize::Zeroizing;

use super::{
    Credential, CredentialError, CredentialSnapshot, MAX_CREDENTIALS, TEXT_LIMITS, bounds,
};

impl CredentialSnapshot {
    /// Decode the exact bounded version 1 snapshot without modifying input.
    ///
    /// Authentication must precede this call in a future vault workflow. This
    /// method validates schema only and never authorizes access or creates a session.
    /// Invalid input returns no partial snapshot or secret-bearing error message.
    pub fn decode(input: &[u8]) -> Result<Self, CredentialError> {
        let text = bounds::preflight(input)?;
        if !text
            .trim_start_matches([' ', '\t', '\r', '\n'])
            .starts_with('{')
        {
            return Err(CredentialError::InvalidSnapshot);
        }
        let mut deserializer = serde_json::Deserializer::from_str(text);
        let snapshot = serde::Deserializer::deserialize_map(&mut deserializer, SnapshotVisitor)
            .map_err(|_| CredentialError::InvalidSnapshot)?;
        deserializer
            .end()
            .map_err(|_| CredentialError::InvalidSnapshot)?;
        Ok(snapshot)
    }
}

enum Field {
    Schema,
    Credentials,
    Id,
    Title,
    Username,
    Password,
    Url,
    Notes,
    CreatedAt,
    UpdatedAt,
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldVisitor;
        impl Visitor<'_> for FieldVisitor {
            type Value = Field;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a version 1 field name")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Field, E> {
                match value {
                    "schema" => Ok(Field::Schema),
                    "credentials" => Ok(Field::Credentials),
                    "id" => Ok(Field::Id),
                    "title" => Ok(Field::Title),
                    "username" => Ok(Field::Username),
                    "password" => Ok(Field::Password),
                    "url" => Ok(Field::Url),
                    "notes" => Ok(Field::Notes),
                    "created_at" => Ok(Field::CreatedAt),
                    "updated_at" => Ok(Field::UpdatedAt),
                    _ => Err(invalid()),
                }
            }
        }
        deserializer.deserialize_identifier(FieldVisitor)
    }
}

struct SnapshotVisitor;

impl<'de> Visitor<'de> for SnapshotVisitor {
    type Value = CredentialSnapshot;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a version 1 credential snapshot")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut schema_seen = false;
        let mut records = None;
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Schema if !schema_seen => {
                    let raw = map.next_value::<&RawValue>()?;
                    if integer(raw).map_err(de::Error::custom)? != 1 {
                        return Err(invalid());
                    }
                    schema_seen = true;
                }
                Field::Credentials if records.is_none() => {
                    records = Some(map.next_value_seed(RecordsSeed)?);
                }
                _ => return Err(invalid()),
            }
        }
        if !schema_seen {
            return Err(invalid());
        }
        CredentialSnapshot::new(records.ok_or_else(invalid)?).map_err(de::Error::custom)
    }
}

struct RecordsSeed;

impl<'de> DeserializeSeed<'de> for RecordsSeed {
    type Value = Vec<Credential>;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        // Inspect a borrowed token before asking for a collection: Serde's
        // wrong-type diagnostics can otherwise decode a large escaped string.
        let raw = <&RawValue>::deserialize(deserializer)?;
        if !raw.get().starts_with('[') {
            return Err(invalid());
        }
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        serde::Deserializer::deserialize_seq(&mut parser, self).map_err(|_| invalid())
    }
}

impl<'de> Visitor<'de> for RecordsSeed {
    type Value = Vec<Credential>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bounded credential array")
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
        let mut records = Vec::new();
        while let Some(record) = seq.next_element_seed(RecordSeed {
            allowed: records.len() < MAX_CREDENTIALS,
        })? {
            if records.len() == records.capacity() {
                let additional = (MAX_CREDENTIALS - records.len()).min(128);
                records
                    .try_reserve_exact(additional)
                    .map_err(|_| de::Error::custom(CredentialError::AllocationFailed))?;
            }
            records.push(record);
        }
        Ok(records)
    }
}

struct RecordSeed {
    allowed: bool,
}

impl<'de> DeserializeSeed<'de> for RecordSeed {
    type Value = Credential;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Credential, D::Error> {
        // SeqAccess calls this only when an element exists; the 10,001st record
        // is rejected before reading its fields or allocating any of its text.
        if !self.allowed {
            return Err(de::Error::custom(CredentialError::LimitExceeded));
        }
        let raw = <&RawValue>::deserialize(deserializer)?;
        if !raw.get().starts_with('{') {
            return Err(invalid());
        }
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        serde::Deserializer::deserialize_map(&mut parser, self).map_err(|_| invalid())
    }
}

impl<'de> Visitor<'de> for RecordSeed {
    type Value = Credential;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a complete credential record")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Credential, M::Error> {
        let mut strings: [Option<Zeroizing<String>>; 6] = std::array::from_fn(|_| None);
        let mut created_at = None;
        let mut updated_at = None;
        while let Some(field) = map.next_key::<Field>()? {
            let index = match field {
                Field::Id => 0,
                Field::Title => 1,
                Field::Username => 2,
                Field::Password => 3,
                Field::Url => 4,
                Field::Notes => 5,
                Field::CreatedAt if created_at.is_none() => {
                    created_at = Some(integer(map.next_value()?).map_err(de::Error::custom)?);
                    continue;
                }
                Field::UpdatedAt if updated_at.is_none() => {
                    updated_at = Some(integer(map.next_value()?).map_err(de::Error::custom)?);
                    continue;
                }
                _ => return Err(invalid()),
            };
            if strings[index].is_some() {
                return Err(invalid());
            }
            let max = if index == 0 {
                32
            } else {
                TEXT_LIMITS[index - 1]
            };
            strings[index] = Some(string(map.next_value()?, max).map_err(de::Error::custom)?);
        }
        let [id, title, username, password, url, notes] = strings;
        let record = Credential {
            id: id.ok_or_else(invalid)?,
            title: title.ok_or_else(invalid)?,
            username: username.ok_or_else(invalid)?,
            password: password.ok_or_else(invalid)?,
            url: url.ok_or_else(invalid)?,
            notes: notes.ok_or_else(invalid)?,
            created_at: created_at.ok_or_else(invalid)?,
            updated_at: updated_at.ok_or_else(invalid)?,
        };
        record.fields().validate().map_err(de::Error::custom)?;
        Ok(record)
    }
}

fn string(raw: &RawValue, max: usize) -> Result<Zeroizing<String>, CredentialError> {
    let length = bounds::string_length(raw.get(), max)?;
    struct StringVisitor(usize);
    impl Visitor<'_> for StringVisitor {
        type Value = Zeroizing<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded credential string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            // Recheck to keep the allocation bound local to the copy as well.
            if value.len() != self.0 {
                return Err(invalid());
            }
            super::copy_text(value).map_err(de::Error::custom)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(raw.get());
    serde::Deserializer::deserialize_str(&mut deserializer, StringVisitor(length))
        .map_err(|_| CredentialError::InvalidSnapshot)
}

fn integer(raw: &RawValue) -> Result<u64, CredentialError> {
    let token = raw.get();
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CredentialError::InvalidSnapshot);
    }
    token.parse().map_err(|_| CredentialError::InvalidSnapshot)
}

fn invalid<E: de::Error>() -> E {
    E::custom(CredentialError::InvalidSnapshot)
}

#[cfg(test)]
mod tests;
