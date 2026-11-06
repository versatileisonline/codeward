//! Test-only fault/event wrapper around the shared experimental native adapter.
//! Controls and arbitrary synthetic paths are absent from runtime builds.

use std::sync::{Arc, Mutex};

use codeward_core::transaction::{PublishError, PublishMode, Storage, StorageError, WriterLease};

use super::*;
use crate::{
    VaultPath,
    test_support::TestDir,
    writer::{ExperimentalStorage, ExperimentalWriterLease},
};

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

struct SyntheticLease {
    inner: ExperimentalWriterLease,
    control: Controls,
}

impl Storage for SyntheticStorage {
    type Lease = SyntheticLease;

    fn acquire(self) -> Result<Self::Lease, StorageError> {
        let inner = ExperimentalStorage::from_path(self.path).acquire()?;
        Ok(SyntheticLease {
            inner,
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
        if self.fault(Fault::Read) {
            return Err(StorageError::Read);
        }
        self.inner.read_current()
    }

    fn stage(&mut self, encrypted: &[u8]) -> Result<(), StorageError> {
        self.event(Event::Stage);
        if self.fault(Fault::Stage) {
            return Err(StorageError::Write);
        }
        self.inner.stage(encrypted)
    }

    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        self.event(Event::Publish(mode));
        if self.fault(Fault::PublishBefore) {
            return Err(PublishError::OutcomeUnknown);
        }
        self.inner.publish(mode)?;
        if self.fault(Fault::PublishAfter) {
            return Err(PublishError::OutcomeUnknown);
        }
        Ok(())
    }

    fn persist(&mut self) -> Result<(), StorageError> {
        self.event(Event::Barrier);
        if self.fault(Fault::Barrier) {
            return Err(StorageError::Barrier);
        }
        self.inner.persist()
    }
}

mod tests;
