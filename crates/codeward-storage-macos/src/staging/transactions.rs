//! Test-only native bridge for the core protocol. Never exported or compiled into
//! the runtime library; real-vault admission/location policy remains incomplete.

use std::sync::{Arc, Mutex};

use codeward_core::transaction::{PublishError, PublishMode, Storage, StorageError, WriterLease};

use super::*;
use crate::{PublicationError, VaultPath, test_support::TestDir};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    Stage,
    Read,
    PublishBefore,
    PublishAfter,
    Barrier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Read,
    Stage,
    Publish(PublishMode),
    Barrier,
}

#[derive(Default)]
struct Control {
    fault: Option<Fault>,
    events: Vec<Event>,
}
type Controls = Arc<Mutex<Control>>;

struct SyntheticStorage {
    path: VaultPath,
    control: Controls,
}

impl SyntheticStorage {
    // Only the exclusively created synthetic test directory supplies a path.
    fn new(dir: &TestDir) -> (Self, Controls) {
        Self::from_path(VaultPath::resolve(&dir.vault()).unwrap())
    }

    fn from_path(path: VaultPath) -> (Self, Controls) {
        let control = Arc::new(Mutex::new(Control::default()));
        (
            Self {
                path,
                control: control.clone(),
            },
            control,
        )
    }
}

struct PendingStage {
    prepared: PreparedStage,
    encrypted: Vec<u8>,
}

struct SyntheticLease {
    native: ReadLease,
    // Outer None means no successful read/publication selected a state. Inner
    // None is observed absence, never a reason to promote an abandoned stage.
    observed: Option<Option<Vec<u8>>>,
    pending: Option<PendingStage>,
    control: Controls,
}

impl Storage for SyntheticStorage {
    type Lease = SyntheticLease;

    fn acquire(self) -> Result<Self::Lease, StorageError> {
        let native = self.path.acquire().map_err(|error| match error {
            NativeError::WriterUnavailable => StorageError::WriterUnavailable,
            _ => StorageError::Unsupported,
        })?;
        Ok(SyntheticLease {
            native,
            observed: None,
            pending: None,
            control: self.control,
        })
    }
}

impl SyntheticLease {
    fn event(&self, event: Event) {
        self.control.lock().unwrap().events.push(event);
    }

    fn fault(&self, fault: Fault) -> bool {
        let mut control = self.control.lock().unwrap();
        if control.fault == Some(fault) {
            control.fault = None;
            true
        } else {
            false
        }
    }
}

impl WriterLease for SyntheticLease {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        self.event(Event::Read);
        self.observed = None;
        if self.fault(Fault::Read) {
            return Err(StorageError::Read);
        }
        let bytes = self.native.read_current().map_err(|_| StorageError::Read)?;
        // Bounded encrypted-only test copies. Never persist a request ledger or
        // treat a structurally valid read as authenticated application access.
        self.observed = Some(bytes.clone());
        Ok(bytes)
    }

    fn stage(&mut self, encrypted: &[u8]) -> Result<(), StorageError> {
        self.event(Event::Stage);
        // Dropping test ownership preserves all residue; it never unlinks files.
        self.pending = None;
        if self.fault(Fault::Stage) {
            return Err(StorageError::Write);
        }
        let stage = self
            .native
            .prepare_stage(encrypted)
            .map_err(|error| match error {
                NativeError::Write => StorageError::Write,
                NativeError::SyncStage => StorageError::SyncStage,
                NativeError::Close => StorageError::Close,
                _ => StorageError::CreateStage,
            })?;
        // Retain the actual opaque stage; it owns the same native exclusion as
        // this worker without borrowing a field or reconstructing private parts.
        self.pending = Some(PendingStage {
            prepared: stage,
            encrypted: encrypted.to_vec(),
        });
        Ok(())
    }

    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        self.event(Event::Publish(mode));
        let stage = self.pending.take().ok_or(PublishError::NotPublished)?;
        let expected = self.observed.as_ref().ok_or(PublishError::NotPublished)?;
        if (mode == PublishMode::Create) != expected.is_none() {
            return Err(PublishError::NotPublished);
        }
        if self.fault(Fault::PublishBefore) {
            return Err(PublishError::OutcomeUnknown);
        }
        let result = match expected {
            Some(bytes) => stage.prepared.publish_replace(bytes),
            None => stage.prepared.publish_create(),
        };
        result.map_err(|error| match error {
            PublicationError::NotPublished(_) => PublishError::NotPublished,
            PublicationError::OutcomeUnknown => PublishError::OutcomeUnknown,
        })?;
        if self.fault(Fault::PublishAfter) {
            return Err(PublishError::OutcomeUnknown);
        }
        self.observed = Some(Some(stage.encrypted));
        Ok(())
    }

    fn persist(&mut self) -> Result<(), StorageError> {
        self.event(Event::Barrier);
        if self.fault(Fault::Barrier) {
            return Err(StorageError::Barrier);
        }
        let expected = self.observed.as_ref().ok_or(StorageError::Barrier)?;
        self.native
            .synchronize_namespace(expected.as_deref())
            .map_err(|_| StorageError::Barrier)
    }
}

mod tests;
