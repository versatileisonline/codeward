//! Framework-independent Codeward core.
//!
//! The first slice implements structural envelope validation and exact public
//! authentication metadata only. Parsing is not authentication: a structurally
//! valid envelope must still pass slot/payload AEAD and JSON schema validation
//! before any credential is exposed. This crate does not yet unlock or save vaults.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod envelope;
