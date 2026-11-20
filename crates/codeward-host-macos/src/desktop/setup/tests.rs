use super::*;
use codeward_core::transaction::{
    EventSourceError, PublishError, PublishMode, SessionEvent, StorageError,
};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

// The core deliberately refuses concurrent master derivations process-wide.
// Serialize only these synthetic KDF scenarios; production remains nonblocking.
static MASTER_SCENARIOS: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Disk>>);
#[derive(Default)]
struct Disk {
    bytes: Option<Vec<u8>>,
    stage: Option<Vec<u8>>,
    fail_stage: bool,
    fail_barrier: bool,
    publications: usize,
    stage_gate: Option<Arc<Gate>>,
    barrier_gate: Option<Arc<Gate>>,
}
impl Storage for Memory {
    type Lease = Self;
    fn acquire(self) -> Result<Self, StorageError> {
        Ok(self)
    }
}
impl WriterLease for Memory {
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn stage(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_stage {
            return Err(StorageError::Write);
        }
        disk.stage = Some(bytes.to_vec());
        let gate = disk.stage_gate.clone();
        drop(disk);
        if let Some(gate) = gate {
            gate.block();
        }
        Ok(())
    }
    fn publish(&mut self, mode: PublishMode) -> Result<(), PublishError> {
        let mut disk = self.0.lock().unwrap();
        if mode != PublishMode::Create || disk.bytes.is_some() {
            return Err(PublishError::NotPublished);
        }
        disk.bytes = disk.stage.take();
        disk.publications += 1;
        Ok(())
    }
    fn persist(&mut self) -> Result<(), StorageError> {
        let gate = self.0.lock().unwrap().barrier_gate.clone();
        if let Some(gate) = gate {
            gate.block();
        }
        if self.0.lock().unwrap().fail_barrier {
            Err(StorageError::Barrier)
        } else {
            Ok(())
        }
    }
}
#[derive(Default, Clone)]
struct Events(Arc<AtomicBool>);
impl HeadlessEventSource for Events {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        Ok(self
            .0
            .swap(false, Ordering::SeqCst)
            .then_some(SessionEvent::Suspend))
    }
    fn wait(&mut self, _: Duration) -> Result<(), EventSourceError> {
        Ok(())
    }
}
fn wait(setup: &mut Setup<Memory, Events>, wanted: State) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = setup.status();
        if status.state == wanted {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "expected {wanted:?}, got {:?}",
            status.state
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn begin(setup: &mut Setup<Memory, Events>, disk: &Memory, events: Events) -> u64 {
    let storage = disk.clone();
    setup
        .begin(
            "synthetic master phrase",
            "synthetic master phrase",
            events,
            move || Ok(storage),
        )
        .unwrap()
        .epoch
}
#[test]
fn code_confirmation_commits_encrypted_bytes_once_and_existing_vault_is_preserved() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = Memory::default();
    let mut setup = Setup::new();
    let epoch = begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Recovery);
    let code = setup.code(epoch).unwrap();
    assert!(disk.0.lock().unwrap().bytes.is_none());
    // A wrong, checksum-valid code must not create a file or consume enrollment.
    let other = codeward_core::vault::Enrollment::begin()
        .unwrap()
        .recovery_code()
        .unwrap();
    setup.confirm(epoch, &other).unwrap();
    wait(&mut setup, State::Recovery);
    assert_eq!(setup.status.error, Some(SetupError::InvalidCode));
    assert!(disk.0.lock().unwrap().bytes.is_none());
    setup.confirm(epoch, &code.code).unwrap();
    assert_eq!(setup.status.state, State::Saving);
    assert!(setup.confirm(epoch, &code.code).is_err());
    wait(&mut setup, State::Created);
    assert!(setup.password.is_none());
    assert!(setup.code(epoch).is_err());
    let bytes = disk.0.lock().unwrap().bytes.clone().unwrap();
    assert!(codeward_core::vault::unlock_with_master(&bytes, "synthetic master phrase").is_ok());
    assert!(
        !bytes
            .windows(b"synthetic master phrase".len())
            .any(|w| w == b"synthetic master phrase")
    );
    assert_eq!(disk.0.lock().unwrap().publications, 1);
    begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Existing);
    assert_eq!(disk.0.lock().unwrap().bytes.as_ref(), Some(&bytes));
}
#[test]
fn revocation_and_cancel_drop_password_and_refuse_stale_code_and_epoch() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = Memory::default();
    let mut setup = Setup::new();
    let events = Events::default();
    let epoch = begin(&mut setup, &disk, events.clone());
    wait(&mut setup, State::Recovery);
    events.0.store(true, Ordering::SeqCst);
    assert!(setup.code(epoch).is_err());
    assert_eq!(setup.status.state, State::Locked);
    assert!(setup.password.is_none());
    let newer = begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Recovery);
    assert!(setup.cancel(epoch).is_err());
    assert!(setup.code(newer).is_ok());
    setup.cancel(newer).unwrap();
    assert!(setup.password.is_none());
    assert!(disk.0.lock().unwrap().bytes.is_none());
    let _ = begin(&mut setup, &disk, Events::default());
    setup.cancel(setup.status.epoch).unwrap();
    assert!(!setup.pending);
    assert!(setup.password.is_none());
}
#[test]
fn failed_stage_never_claims_creation_and_unknown_barrier_never_replays() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    for barrier in [false, true] {
        let disk = Memory::default();
        {
            let mut state = disk.0.lock().unwrap();
            state.fail_stage = !barrier;
            state.fail_barrier = barrier;
        }
        let mut setup = Setup::new();
        let epoch = begin(&mut setup, &disk, Events::default());
        wait(&mut setup, State::Recovery);
        let code = setup.code(epoch).unwrap();
        setup.confirm(epoch, &code.code).unwrap();
        wait(
            &mut setup,
            if barrier {
                State::Unknown
            } else {
                State::Failed
            },
        );
        assert!(setup.password.is_none());
        if barrier {
            assert_eq!(disk.0.lock().unwrap().publications, 1);
            setup.cancel(epoch).unwrap();
            let storage = disk.clone();
            assert!(
                setup
                    .begin("sample", "sample", Events::default(), move || Ok(storage))
                    .is_err()
            );
            assert!(setup.confirm(epoch, &code.code).is_err());
        } else {
            assert!(disk.0.lock().unwrap().bytes.is_none());
        }
    }
}
#[test]
fn invalid_password_bounds_and_confirmation_start_no_worker() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let mut setup = Setup::<Memory, Events>::new();
    for (password, confirmation) in [
        ("", ""),
        ("sample", "different"),
        (&"é".repeat(2049), &"é".repeat(2049)),
    ] {
        assert!(
            setup
                .begin(
                    password,
                    confirmation,
                    Events::default(),
                    || -> Result<Memory, TransactionError> { panic!("must not open storage") }
                )
                .is_err()
        );
    }
    assert!(setup.password.is_none());
    assert!(setup.control.is_none());
}

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    wake: Condvar,
}
impl Gate {
    fn block(&self) {
        self.entered.store(true, Ordering::SeqCst);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.wake.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
#[test]
fn cancel_during_stage_prevents_publication_but_admitted_save_keeps_its_outcome() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    for admitted in [false, true] {
        let gate = Arc::new(Gate::default());
        let disk = Memory::default();
        {
            let mut state = disk.0.lock().unwrap();
            if admitted {
                state.barrier_gate = Some(gate.clone());
            } else {
                state.stage_gate = Some(gate.clone());
            }
        }
        let mut setup = Setup::new();
        let epoch = begin(&mut setup, &disk, Events::default());
        wait(&mut setup, State::Recovery);
        let code = setup.code(epoch).unwrap();
        setup.confirm(epoch, &code.code).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !gate.entered.load(Ordering::SeqCst) {
            setup.status();
            if Instant::now() >= deadline {
                gate.release();
                panic!("worker did not reach gate");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(setup.status.state, State::Created);
        setup.cancel(epoch).unwrap();
        let password_released = setup.password.is_none();
        let no_code = setup.code(epoch).is_err();
        let storage = disk.clone();
        let refused = setup
            .begin("sample", "sample", Events::default(), move || Ok(storage))
            .is_err();
        gate.release();
        assert!(password_released && no_code && refused);
        wait(
            &mut setup,
            if admitted {
                State::Created
            } else {
                State::Failed
            },
        );
        assert_eq!(disk.0.lock().unwrap().publications, usize::from(admitted));
    }
}

#[derive(Clone, Default)]
struct BoundaryEvents {
    observations: Arc<std::sync::atomic::AtomicUsize>,
    revoke_at: Arc<std::sync::atomic::AtomicUsize>,
}
impl HeadlessEventSource for BoundaryEvents {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        let count = self.observations.fetch_add(1, Ordering::SeqCst) + 1;
        Ok((count == self.revoke_at.load(Ordering::SeqCst)).then_some(SessionEvent::Suspend))
    }
    fn wait(&mut self, _: Duration) -> Result<(), EventSourceError> {
        Ok(())
    }
}
#[test]
fn revocation_at_both_command_checks_clears_bridge_secrets_immediately() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    for confirm in [false, true] {
        for offset in [3, 4] {
            let events = BoundaryEvents::default();
            let disk = Memory::default();
            let storage = disk.clone();
            let mut setup = Setup::new();
            let epoch = setup
                .begin("sample", "sample", events.clone(), move || Ok(storage))
                .unwrap()
                .epoch;
            let deadline = Instant::now() + Duration::from_secs(5);
            while setup.status().state != State::Recovery {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            let code = setup.code(epoch).unwrap();
            events.revoke_at.store(
                events.observations.load(Ordering::SeqCst) + offset,
                Ordering::SeqCst,
            );
            if confirm {
                assert!(setup.confirm(epoch, &code.code).is_err());
            } else {
                assert!(setup.code(epoch).is_err());
            }
            assert!(
                setup.password.is_none(),
                "secret retained after command revocation"
            );
            assert_eq!(setup.status.state, State::Locked);
            while setup.pending {
                setup.status();
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(disk.0.lock().unwrap().bytes.is_none());
        }
    }
}
