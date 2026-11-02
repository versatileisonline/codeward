//! Framework-independent Codeward core.
//!
//! Implements envelope parsing/encoding, public authentication metadata,
//! bounded cryptographic primitives, paper-secret transcription checks and strict
//! credential JSON handling. The vault layer authenticates complete envelopes and
//! gates contents through that schema before returning in-memory access objects.
//! Session-bound transaction admission and an adapter-driven commit protocol are
//! testable without a UI. No native storage adapter or UI commands are implemented.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod credentials;
pub mod crypto;
pub mod encoding;
pub mod envelope;
pub mod recovery;
pub mod transaction;
pub mod vault;
