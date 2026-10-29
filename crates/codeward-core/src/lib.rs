//! Framework-independent Codeward core.
//!
//! Implements structural envelope parsing/encoding and exact public
//! authentication metadata only. Parsing is not authentication: a structurally
//! valid envelope must still pass slot/payload AEAD and JSON schema validation
//! before any credential is exposed. This crate does not yet unlock or save vaults.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod crypto;
pub mod encoding;
pub mod envelope;
