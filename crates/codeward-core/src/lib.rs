//! Framework-independent Codeward core.
//!
//! Implements envelope parsing/encoding, public authentication metadata,
//! bounded cryptographic primitives, paper-secret transcription checks and strict
//! credential JSON handling.
//! Parsing is not authentication; authenticated bytes still require credential
//! schema validation before exposure. This crate does not yet unlock or save vaults.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod credentials;
pub mod crypto;
pub mod encoding;
pub mod envelope;
pub mod recovery;
