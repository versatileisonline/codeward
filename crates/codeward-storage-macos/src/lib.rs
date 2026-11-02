//! Experimental macOS path, read and exclusion foundation for synthetic data.
//!
//! This is not a writable storage adapter or a supported credential store. Native
//! staging, publication, persistence barriers, ACL/privacy and cloud-location
//! validation remain separate work. Nothing here reports a committed save.

#![cfg(target_os = "macos")]
#![deny(missing_docs, unsafe_op_in_unsafe_fn)]

// Native calls stay in this adapter crate; codeward-core remains unsafe-free.
