use std::sync::{Mutex, mpsc};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Fault {
    Read,
    Create,
    ShortWrite,
    Close,
    Sync,
    PublishBefore,
    UnknownBefore,
    UnknownAfter,
    Barrier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Event {
    Read,
    Create,
    Write,
    Close,
    Sync,
    Publish(PublishMode),
    Barrier,
}

pub(super) struct Disk {
    pub live: Option<Vec<u8>>,
    pub durable: Option<Vec<u8>>,
    pub stages: Vec<Vec<u8>>,
    pub events: Vec<Event>,
    pub fault: Option<Fault>,
    pub held: bool,
    pub stage_block: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    staged: Option<Vec<u8>>,
}

#[derive(Clone)]
pub(super) struct Model(pub Arc<Mutex<Disk>>);
pub(super) struct Lease(Model);

impl Model {
    pub fn new(bytes: Option<Vec<u8>>) -> Self {
        Self(Arc::new(Mutex::new(Disk {
            durable: bytes.clone(),
            live: bytes,
            stages: Vec::new(),
            events: Vec::new(),
            fault: None,
            held: false,
            stage_block: None,
            staged: None,
        })))
    }
    pub fn fault(&self, fault: Fault) {
        self.0.lock().unwrap().fault = Some(fault);
    }
    pub fn live(&self) -> Option<Vec<u8>> {
        self.0.lock().unwrap().live.clone()
    }
    pub fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().events.clone()
    }
}

impl Storage for Model {
    type Lease = Lease;
    fn acquire(self) -> Result<Lease, StorageError> {
        {
            let mut disk = self.0.lock().unwrap();
            if disk.held {
                return Err(StorageError::WriterUnavailable);
            }
            disk.held = true;
        }
        Ok(Lease(self))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .held = false;
    }
}

impl WriterLease for Lease {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        let mut disk = self.0.0.lock().unwrap();
        disk.events.push(Event::Read);
        if disk.fault == Some(Fault::Read) {
            disk.fault = None;
            return Err(StorageError::Read);
        }
        if disk
            .live
            .as_ref()
            .is_some_and(|bytes| bytes.len() > crate::envelope::MAX_FILE_LENGTH)
        {
            return Err(StorageError::Read);
        }
        Ok(disk.live.clone())
    }

    fn stage(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let block = self.0.0.lock().unwrap().stage_block.take();
        if let Some((entered, resume)) = block {
            entered.send(()).unwrap();
            resume.recv().unwrap();
        }
        let mut disk = self.0.0.lock().unwrap();
        disk.staged = None;
        disk.events.push(Event::Create);
        if disk.fault == Some(Fault::Create) {
            disk.fault = None;
            return Err(StorageError::CreateStage);
        }
        disk.events.push(Event::Write);
        if disk.fault == Some(Fault::ShortWrite) {
            disk.fault = None;
            disk.stages.push(bytes[..bytes.len() / 2].to_vec());
            return Err(StorageError::Write);
        }
        disk.stages.push(bytes.to_vec());
        for (event, fault, error) in [
            (Event::Close, Fault::Close, StorageError::Close),
            (Event::Sync, Fault::Sync, StorageError::SyncStage),
        ] {
            disk.events.push(event);
            if disk.fault == Some(fault) {
                disk.fault = None;
                return Err(error);
            }
        }
        disk.staged = Some(bytes.to_vec());
        Ok(())
    }

    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        let mut disk = self.0.0.lock().unwrap();
        disk.events.push(Event::Publish(mode));
        if mode == PublishMode::Create && disk.live.is_some() {
            return Err(PublishError::NotPublished);
        }
        match disk.fault {
            Some(Fault::PublishBefore) => {
                disk.fault = None;
                return Err(PublishError::NotPublished);
            }
            Some(Fault::UnknownBefore) => {
                disk.fault = None;
                return Err(PublishError::OutcomeUnknown);
            }
            _ => {}
        }
        disk.live = Some(
            disk.staged
                .take()
                .expect("only complete synchronized stage publishes"),
        );
        if disk.fault == Some(Fault::UnknownAfter) {
            disk.fault = None;
            return Err(PublishError::OutcomeUnknown);
        }
        Ok(())
    }

    fn persist(&mut self) -> Result<(), StorageError> {
        let mut disk = self.0.0.lock().unwrap();
        disk.events.push(Event::Barrier);
        if disk.fault == Some(Fault::Barrier) {
            disk.fault = None;
            return Err(StorageError::Barrier);
        }
        disk.durable = disk.live.clone();
        Ok(())
    }
}
