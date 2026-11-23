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
        if (mode == PublishMode::Create) == disk.bytes.is_some() {
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

fn saved_disk() -> Memory {
    let disk = Memory::default();
    let mut setup = Setup::new();
    let epoch = begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Recovery);
    let code = setup.code(epoch).unwrap();
    setup.confirm(epoch, &code.code).unwrap();
    wait(&mut setup, State::Created);
    disk
}
#[test]
fn existing_vault_unlocks_retries_wrong_password_and_locks_without_writes() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let before = disk.0.lock().unwrap().bytes.clone();
    let storage = disk.clone();
    let events = Events::default();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    assert!(setup.password.is_none());
    setup
        .unlock(epoch, setup.status.revocation, "wrong synthetic password")
        .unwrap();
    wait(&mut setup, State::Locked);
    assert_eq!(setup.status.error, Some(SetupError::WrongPassword));
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    assert!(
        setup
            .control
            .as_mut()
            .unwrap()
            .command(|host| {
                let generation = host.generation();
                host.list_credentials(generation, 1, None)
            })
            .is_ok()
    );
    setup.lock_existing(epoch).unwrap();
    assert_eq!(setup.status.state, State::Locked);
    assert!(
        setup
            .control
            .as_mut()
            .unwrap()
            .command(|host| {
                let generation = host.generation();
                host.list_credentials(generation, 1, None)
            })
            .is_err()
    );
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    events.0.store(true, Ordering::SeqCst);
    assert_eq!(setup.status().state, State::Locked);
    assert_eq!(disk.0.lock().unwrap().bytes, before);
    assert_eq!(disk.0.lock().unwrap().publications, 1);
}
#[test]
fn missing_or_corrupt_vaults_are_never_created_by_open_or_unlock() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    for corrupt in [false, true] {
        let disk = Memory::default();
        if corrupt {
            disk.0.lock().unwrap().bytes = Some(vec![0]);
        }
        let before = disk.0.lock().unwrap().bytes.clone();
        let storage = disk.clone();
        let mut setup = Setup::new();
        let epoch = setup
            .open_existing(Events::default(), move || Ok(storage))
            .unwrap()
            .epoch;
        wait(
            &mut setup,
            if corrupt {
                State::Failed
            } else {
                State::Missing
            },
        );
        assert!(
            setup
                .unlock(epoch, setup.status.revocation, "sample")
                .is_err()
        );
        assert_eq!(disk.0.lock().unwrap().bytes, before);
        assert_eq!(disk.0.lock().unwrap().publications, 0);
    }
}
#[test]
fn locking_queued_authentication_rejects_late_success_and_stale_epochs() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    setup.lock_existing(epoch).unwrap();
    for _ in 0..25 {
        assert_eq!(setup.status().state, State::Locked);
        std::thread::sleep(Duration::from_millis(10));
    }
    setup.cancel(epoch).unwrap();
    let storage = disk.clone();
    let newer = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    assert_ne!(newer, epoch);
    assert!(
        setup
            .unlock(epoch, setup.status.revocation, "synthetic master phrase")
            .is_err()
    );
    assert!(setup.lock_existing(epoch).is_err());
    // A canceled KDF may already be running. Explicit retries must eventually
    // work, without allowing that canceled completion to install access.
    let deadline = Instant::now() + Duration::from_secs(15);
    while setup.status().state != State::Unlocked {
        if setup.status.state == State::Locked {
            setup
                .unlock(newer, setup.status.revocation, "synthetic master phrase")
                .unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unlock_command_boundary_revocation_never_installs_late_authentication() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    for offset in [1, 2, 3, 4] {
        let events = BoundaryEvents::default();
        let mut setup = Setup::new();
        let storage = disk.clone();
        let epoch = setup
            .open_existing(events.clone(), move || Ok(storage))
            .unwrap()
            .epoch;
        let deadline = Instant::now() + Duration::from_secs(15);
        while setup.status().state != State::Locked {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let old = setup.status.revocation;
        events.revoke_at.store(
            events.observations.load(Ordering::SeqCst) + offset,
            Ordering::SeqCst,
        );
        assert!(
            setup
                .unlock(epoch, setup.status.revocation, "synthetic master phrase")
                .is_err()
        );
        assert_eq!(setup.status.state, State::Locked);
        assert!(!setup.pending);
        assert!(setup.status.revocation > old);
        for _ in 0..10 {
            assert_ne!(setup.status().state, State::Unlocked);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Only a new explicit attempt may unlock after cancellation.
        while setup.status().state != State::Unlocked {
            if setup.status.state == State::Locked {
                setup
                    .unlock(epoch, setup.status.revocation, "synthetic master phrase")
                    .unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn timer_revocation_refuses_old_unlock_requests_and_revoked_opening_requires_reopen() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let events = Events::default();
    let storage = disk.clone();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    let old = setup.status.revocation;
    events.0.store(true, Ordering::SeqCst);
    assert_eq!(setup.status().state, State::Locked);
    assert!(setup.unlock(epoch, old, "synthetic master phrase").is_err());
    assert!(!setup.pending);
    setup.cancel(epoch).unwrap();
    let storage = disk.clone();
    let events = Events::default();
    setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap();
    events.0.store(true, Ordering::SeqCst);
    assert_eq!(setup.status().state, State::Failed);
    let storage = disk.clone();
    setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap();
    wait(&mut setup, State::Locked);
}

#[test]
fn credential_bridge_rejects_locked_and_stale_scope() {
    let mut setup = Setup::<Memory, Events>::new();
    assert!(setup.credentials_list(0, 0, None).is_err());
}

fn credential_input(
    epoch: u64,
    revocation: u64,
    generation: &str,
    revision: &str,
    id: Option<String>,
    keep: bool,
) -> credentials::WriteInput {
    credentials::WriteInput {
        epoch,
        revocation,
        generation: generation.into(),
        revision: revision.into(),
        id,
        form: credentials::Form {
            title: "Synthetic account".into(),
            username: "sample user".into(),
            url: "https://example.test".into(),
            notes: "sample notes".into(),
            password: credentials::Password {
                mode: if keep { "keep" } else { "provided" }.into(),
                draft: None,
                value: if keep {
                    None
                } else {
                    Some("synthetic credential password".into())
                },
            },
        },
    }
}
fn selected(
    epoch: u64,
    revocation: u64,
    generation: &str,
    revision: &str,
    id: &str,
) -> credentials::Selection {
    credentials::Selection {
        epoch,
        revocation,
        generation: generation.into(),
        revision: revision.into(),
        id: id.into(),
    }
}
fn operation_wait(setup: &mut Setup<Memory, Events>, epoch: u64, operation: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = setup
            .credentials_operation(credentials::OperationInput {
                epoch,
                operation: operation.into(),
            })
            .unwrap()
            .state;
        if state != "saving" {
            return state.into();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn credential_bridge_round_trip_keep_delete_and_dirty_close() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let events = Events::default();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    assert!(page.summaries.is_empty());
    assert!(setup.credentials_list(epoch, revocation + 1, None).is_err());
    let before_activity = setup
        .control
        .as_mut()
        .unwrap()
        .command(|host| Ok(host.inactivity_deadline()))
        .unwrap();
    std::thread::sleep(Duration::from_millis(2));
    setup
        .record_activity(credentials::ActivityInput {
            epoch,
            revocation,
            generation: page.generation.clone(),
            kind: "keyboard".into(),
        })
        .unwrap();
    let after_activity = setup
        .control
        .as_mut()
        .unwrap()
        .command(|host| Ok(host.inactivity_deadline()))
        .unwrap();
    assert!(after_activity > before_activity);
    let mut malformed = credential_input(
        epoch,
        revocation,
        &page.generation,
        &page.revision,
        None,
        false,
    );
    malformed.form.password.mode = "invalid".into();
    assert!(setup.credentials_write(malformed, false).is_err());
    assert_eq!(setup.status().state, State::Unlocked);
    let op = setup
        .credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                None,
                false,
            ),
            false,
        )
        .unwrap();
    assert_eq!(op.state, "saving");
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    assert_eq!(page.summaries.len(), 1);
    let id = page.summaries[0].id.to_string();
    let details = setup
        .credentials_details(selected(
            epoch,
            revocation,
            &page.generation,
            &page.revision,
            &id,
        ))
        .unwrap();
    assert_eq!(details.notes.as_str(), "sample notes");
    let reveal = setup
        .credentials_reveal(selected(
            epoch,
            revocation,
            &page.generation,
            &page.revision,
            &id,
        ))
        .unwrap();
    assert_eq!(reveal.password.as_str(), "synthetic credential password");
    assert!(reveal.expires_in_ms > 0 && reveal.expires_in_ms <= 30000);
    let op = setup
        .credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                Some(id.clone()),
                true,
            ),
            true,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    assert!(
        setup
            .credentials_details(selected(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                &id
            ))
            .is_err()
    );
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    assert_eq!(
        setup
            .credentials_reveal(selected(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                &id
            ))
            .unwrap()
            .password
            .as_str(),
        "synthetic credential password"
    );
    setup
        .set_dirty(credentials::DirtyInput {
            epoch,
            revocation,
            dirty: true,
        })
        .unwrap();
    assert!(setup.request_close());
    assert!(setup.status().close_requested);
    setup
        .resolve_close(&credentials::CloseInput {
            epoch,
            revocation,
            allow: false,
        })
        .unwrap();
    assert!(!setup.status().close_requested);
    let op = setup
        .credentials_delete(selected(
            epoch,
            revocation,
            &page.generation,
            &page.revision,
            &id,
        ))
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    assert!(
        setup
            .credentials_list(epoch, revocation, None)
            .unwrap()
            .summaries
            .is_empty()
    );
    events.0.store(true, Ordering::SeqCst);
    assert_eq!(setup.status().state, State::Locked);
    assert!(!setup.request_close());
    assert!(setup.credentials_list(epoch, revocation, None).is_err());
}
#[test]
fn credential_bridge_definite_failure_retries_and_uncertainty_blocks() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    disk.0.lock().unwrap().fail_stage = true;
    let op = setup
        .credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                None,
                false,
            ),
            false,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "failed");
    assert!(
        setup
            .credentials_list(epoch, revocation, None)
            .unwrap()
            .summaries
            .is_empty()
    );
    disk.0.lock().unwrap().fail_stage = false;
    disk.0.lock().unwrap().fail_barrier = true;
    let op = setup
        .credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                None,
                false,
            ),
            false,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "unknown");
    assert!(setup.credentials_list(epoch, revocation, None).is_err());
    assert!(
        setup
            .credentials_write(
                credential_input(
                    epoch,
                    revocation,
                    &page.generation,
                    &page.revision,
                    None,
                    false
                ),
                false
            )
            .is_err()
    );
}

#[test]
fn credential_command_boundary_revocation_never_becomes_enrollment_completion() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    for offset in 1..=4 {
        let events = BoundaryEvents::default();
        let storage = disk.clone();
        let mut setup = Setup::new();
        let epoch = setup
            .open_existing(events.clone(), move || Ok(storage))
            .unwrap()
            .epoch;
        let deadline = Instant::now() + Duration::from_secs(15);
        while setup.status().state != State::Locked {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        setup
            .unlock(epoch, setup.status.revocation, "synthetic master phrase")
            .unwrap();
        while setup.status().state != State::Unlocked {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let revocation = setup.status.revocation;
        let page = setup.credentials_list(epoch, revocation, None).unwrap();
        events.revoke_at.store(
            events.observations.load(Ordering::SeqCst) + offset,
            Ordering::SeqCst,
        );
        let _ = setup.credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                None,
                false,
            ),
            false,
        );
        for _ in 0..100 {
            assert_ne!(setup.status().state, State::Created);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(setup.status().state, State::Locked);
    }
}

#[test]
fn generated_bridge_references_replace_safely_and_create_without_plaintext() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    let input = || credentials::GenerateInput {
        epoch,
        revocation,
        generation: page.generation.clone(),
        revision: page.revision.clone(),
        options: credentials::GeneratorOptions {
            length: 24,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
        },
    };
    let first = setup.generate_draft(input()).unwrap();
    let reference = |draft: &str| credentials::DraftInput {
        epoch,
        revocation,
        generation: page.generation.clone(),
        revision: page.revision.clone(),
        draft: draft.into(),
    };
    let first_value = setup.reveal_draft(reference(&first.draft)).unwrap();
    assert_eq!(first_value.password.len(), 24);
    assert!(first_value.expires_in_ms > 0 && first_value.expires_in_ms <= 30000);
    let mut invalid = input();
    invalid.options.length = 11;
    assert!(setup.generate_draft(invalid).is_err());
    assert_eq!(
        setup
            .reveal_draft(reference(&first.draft))
            .unwrap()
            .password
            .as_str(),
        first_value.password.as_str()
    );
    let second = setup.generate_draft(input()).unwrap();
    assert_ne!(first.draft, second.draft);
    assert!(setup.discard_draft(reference(&first.draft)).is_err());
    let value = setup.reveal_draft(reference(&second.draft)).unwrap();
    let mut create = credential_input(
        epoch,
        revocation,
        &page.generation,
        &page.revision,
        None,
        false,
    );
    create.form.password.mode = "generated".into();
    create.form.password.value = None;
    create.form.password.draft = Some(second.draft.clone());
    let op = setup.credentials_write(create, false).unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    assert!(setup.reveal_draft(reference(&second.draft)).is_err());
    let current = setup.credentials_list(epoch, revocation, None).unwrap();
    let id = &current.summaries[0].id;
    assert_eq!(
        setup
            .credentials_reveal(selected(
                epoch,
                revocation,
                &current.generation,
                &current.revision,
                id
            ))
            .unwrap()
            .password
            .as_str(),
        value.password.as_str()
    );
    setup.lock_existing(epoch).unwrap();
    assert!(setup.generate_draft(input()).is_err());
    assert!(setup.reveal_draft(reference(&second.draft)).is_err());
}

#[test]
fn generated_bridge_failed_save_retains_draft_and_generated_edit_reopens() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    let generate = |generation: &str, revision: &str| credentials::GenerateInput {
        epoch,
        revocation,
        generation: generation.into(),
        revision: revision.into(),
        options: credentials::GeneratorOptions {
            length: 36,
            lowercase: false,
            uppercase: false,
            digits: true,
            symbols: false,
        },
    };
    let generated = setup
        .generate_draft(generate(&page.generation, &page.revision))
        .unwrap();
    let reference = || credentials::DraftInput {
        epoch,
        revocation,
        generation: page.generation.clone(),
        revision: page.revision.clone(),
        draft: generated.draft.clone(),
    };
    let original = setup.reveal_draft(reference()).unwrap();
    assert_eq!(original.password.len(), 36);
    assert!(original.password.bytes().all(|b| b.is_ascii_digit()));
    let form = |id: Option<String>, generation: &str, revision: &str, token: &str| {
        let mut input = credential_input(epoch, revocation, generation, revision, id, false);
        input.form.password.mode = "generated".into();
        input.form.password.value = None;
        input.form.password.draft = Some(token.into());
        input
    };
    let mut invalid = form(None, &page.generation, &page.revision, &generated.draft);
    invalid.form.password.value = Some("no mixed intent".into());
    assert!(setup.credentials_write(invalid, false).is_err());
    disk.0.lock().unwrap().fail_stage = true;
    let op = setup
        .credentials_write(
            form(None, &page.generation, &page.revision, &generated.draft),
            false,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "failed");
    assert_eq!(
        setup.reveal_draft(reference()).unwrap().password.as_str(),
        original.password.as_str()
    );
    disk.0.lock().unwrap().fail_stage = false;
    let op = setup
        .credentials_write(
            form(None, &page.generation, &page.revision, &generated.draft),
            false,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    let page = setup.credentials_list(epoch, revocation, None).unwrap();
    let id = page.summaries[0].id.to_string();
    let edited = setup
        .generate_draft(generate(&page.generation, &page.revision))
        .unwrap();
    let expected = setup
        .reveal_draft(credentials::DraftInput {
            epoch,
            revocation,
            generation: page.generation.clone(),
            revision: page.revision.clone(),
            draft: edited.draft.clone(),
        })
        .unwrap();
    let mut update = form(
        Some(id.clone()),
        &page.generation,
        &page.revision,
        &edited.draft,
    );
    update.form.title = "Generated edit".into();
    let op = setup.credentials_write(update, true).unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    setup.lock_existing(epoch).unwrap();
    drop(setup);
    let storage = disk.clone();
    let mut reopened = Setup::new();
    let epoch = reopened
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut reopened, State::Locked);
    reopened
        .unlock(epoch, reopened.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut reopened, State::Unlocked);
    let revocation = reopened.status.revocation;
    let page = reopened.credentials_list(epoch, revocation, None).unwrap();
    assert_eq!(page.summaries[0].title.as_str(), "Generated edit");
    assert_eq!(
        reopened
            .credentials_reveal(selected(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                &id
            ))
            .unwrap()
            .password
            .as_str(),
        expected.password.as_str()
    );
    let generated = reopened
        .generate_draft(credentials::GenerateInput {
            epoch,
            revocation,
            generation: page.generation.clone(),
            revision: page.revision.clone(),
            options: credentials::GeneratorOptions {
                length: 24,
                lowercase: true,
                uppercase: true,
                digits: true,
                symbols: true,
            },
        })
        .unwrap();
    let mut update = credential_input(
        epoch,
        revocation,
        &page.generation,
        &page.revision,
        Some(id),
        false,
    );
    update.form.password.mode = "generated".into();
    update.form.password.value = None;
    update.form.password.draft = Some(generated.draft.clone());
    disk.0.lock().unwrap().fail_barrier = true;
    let op = reopened.credentials_write(update, true).unwrap();
    assert_eq!(
        operation_wait(&mut reopened, epoch, &op.operation),
        "unknown"
    );
    assert!(
        reopened
            .reveal_draft(credentials::DraftInput {
                epoch,
                revocation,
                generation: page.generation,
                revision: page.revision,
                draft: generated.draft
            })
            .is_err()
    );
}

#[test]
fn generated_bridge_command_boundary_revocation_never_delivers_a_preview() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    for offset in 1..=4 {
        let events = BoundaryEvents::default();
        let storage = disk.clone();
        let mut setup = Setup::new();
        let epoch = setup
            .open_existing(events.clone(), move || Ok(storage))
            .unwrap()
            .epoch;
        let deadline = Instant::now() + Duration::from_secs(15);
        while setup.status().state != State::Locked {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        setup
            .unlock(epoch, setup.status.revocation, "synthetic master phrase")
            .unwrap();
        while setup.status().state != State::Unlocked {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let revocation = setup.status.revocation;
        let page = setup.credentials_list(epoch, revocation, None).unwrap();
        let draft = setup
            .generate_draft(credentials::GenerateInput {
                epoch,
                revocation,
                generation: page.generation.clone(),
                revision: page.revision.clone(),
                options: credentials::GeneratorOptions {
                    length: 24,
                    lowercase: true,
                    uppercase: true,
                    digits: true,
                    symbols: true,
                },
            })
            .unwrap();
        events.revoke_at.store(
            events.observations.load(Ordering::SeqCst) + offset,
            Ordering::SeqCst,
        );
        assert!(
            setup
                .reveal_draft(credentials::DraftInput {
                    epoch,
                    revocation,
                    generation: page.generation,
                    revision: page.revision,
                    draft: draft.draft
                })
                .is_err()
        );
        assert_eq!(setup.status().state, State::Locked);
    }
}

#[test]
fn paper_recovery_is_restricted_replaces_only_master_and_requires_new_unlock() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = Memory::default();
    let mut setup = Setup::new();
    let epoch = begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Recovery);
    let code = setup.code(epoch).unwrap();
    setup.confirm(epoch, &code.code).unwrap();
    wait(&mut setup, State::Created);
    let storage = disk.clone();
    let events = Events::default();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let page = setup
        .credentials_list(epoch, setup.status.revocation, None)
        .unwrap();
    let op = setup
        .credentials_write(
            credential_input(
                epoch,
                setup.status.revocation,
                &page.generation,
                &page.revision,
                None,
                false,
            ),
            false,
        )
        .unwrap();
    assert_eq!(operation_wait(&mut setup, epoch, &op.operation), "saved");
    setup.lock_existing(epoch).unwrap();
    let revoked = setup.status.revocation;
    setup.begin_recovery(epoch, revoked, &code.code).unwrap();
    wait(&mut setup, State::RecoveryReady);
    assert!(setup.credentials_list(epoch, revoked, None).is_err());
    assert!(
        setup
            .complete_recovery(epoch, revoked, "replacement", "different")
            .is_err()
    );
    setup
        .complete_recovery(epoch, revoked, "replacement", "replacement")
        .unwrap();
    wait(&mut setup, State::RecoverySaved);
    events.0.store(true, Ordering::SeqCst);
    assert_eq!(setup.status().state, State::RecoverySaved);
    assert!(
        setup
            .credentials_list(epoch, setup.status.revocation, None)
            .is_err()
    );
    let bytes = disk.0.lock().unwrap().bytes.clone().unwrap();
    assert!(codeward_core::vault::unlock_with_master(&bytes, "synthetic master phrase").is_err());
    assert!(codeward_core::vault::unlock_with_master(&bytes, "replacement").is_ok());
    assert!(codeward_core::vault::begin_recovery(&bytes, &code.code).is_ok());
    setup.lock_existing(epoch).unwrap();
    setup
        .unlock(epoch, setup.status.revocation, "replacement")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let page = setup
        .credentials_list(epoch, setup.status.revocation, None)
        .unwrap();
    assert_eq!(page.summaries.len(), 1);
    let details = setup
        .credentials_reveal(selected(
            epoch,
            setup.status.revocation,
            &page.generation,
            &page.revision,
            &page.summaries[0].id,
        ))
        .unwrap();
    assert_eq!(details.password.as_str(), "synthetic credential password");
}

fn recoverable_disk() -> (Memory, String) {
    let disk = Memory::default();
    let mut setup = Setup::new();
    let epoch = begin(&mut setup, &disk, Events::default());
    wait(&mut setup, State::Recovery);
    let paper = setup.code(epoch).unwrap().code.to_string();
    setup.confirm(epoch, &paper).unwrap();
    wait(&mut setup, State::Created);
    (disk, paper)
}
fn open_recoverable(disk: &Memory, events: Events) -> Setup<Memory, Events> {
    let storage = disk.clone();
    let mut setup = Setup::new();
    setup.open_existing(events, move || Ok(storage)).unwrap();
    wait(&mut setup, State::Locked);
    setup
}
fn paper_ready(setup: &mut Setup<Memory, Events>, paper: &str) {
    setup
        .begin_recovery(setup.status.epoch, setup.status.revocation, paper)
        .unwrap();
    wait(setup, State::RecoveryReady);
}
#[test]
fn recovery_bad_proof_stale_scope_and_revocation_never_browse_or_write() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let (disk, paper) = recoverable_disk();
    let events = Events::default();
    let mut setup = open_recoverable(&disk, events.clone());
    let epoch = setup.status.epoch;
    let revision = setup.status.revocation;
    assert!(setup.begin_recovery(epoch + 1, revision, &paper).is_err());
    assert!(setup.begin_recovery(epoch, revision + 1, &paper).is_err());
    setup.begin_recovery(epoch, revision, "wrong code").unwrap();
    wait(&mut setup, State::Locked);
    assert_eq!(setup.status.error, Some(SetupError::InvalidCode));
    paper_ready(&mut setup, &paper);
    assert!(
        setup
            .credentials_list(epoch, setup.status.revocation, None)
            .is_err()
    );
    let before = disk.0.lock().unwrap().bytes.clone();
    events.0.store(true, Ordering::SeqCst);
    assert!(
        setup
            .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
            .is_err()
    );
    assert_eq!(setup.status().state, State::Locked);
    assert!(setup.status.revocation > revision);
    assert_eq!(disk.0.lock().unwrap().bytes, before);
    paper_ready(&mut setup, &paper);
    setup.cancel(epoch).unwrap();
    assert_eq!(setup.status.state, State::Locked);
    assert!(
        setup
            .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
            .is_err()
    );
}
#[test]
fn recovery_definite_failure_needs_fresh_proof_and_unknown_reconciles_without_replay() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let (disk, paper) = recoverable_disk();
    let mut setup = open_recoverable(&disk, Events::default());
    let epoch = setup.status.epoch;
    paper_ready(&mut setup, &paper);
    let before = disk.0.lock().unwrap().bytes.clone();
    disk.0.lock().unwrap().fail_stage = true;
    setup
        .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
        .unwrap();
    wait(&mut setup, State::RecoveryFailed);
    assert_eq!(disk.0.lock().unwrap().bytes, before);
    assert!(
        setup
            .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
            .is_err()
    );
    disk.0.lock().unwrap().fail_stage = false;
    paper_ready(&mut setup, &paper);
    disk.0.lock().unwrap().fail_barrier = true;
    setup
        .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
        .unwrap();
    wait(&mut setup, State::RecoveryUnknown);
    let publications = disk.0.lock().unwrap().publications;
    setup.cancel(epoch).unwrap();
    let storage = disk.clone();
    let current = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap();
    assert_eq!(current.epoch, epoch);
    assert_eq!(current.state, State::RecoveryUnknown);
    assert!(setup.lock_existing(epoch).is_err());
    assert!(
        setup
            .unlock(epoch, current.revocation, "replacement")
            .is_err()
    );
    assert!(
        setup
            .begin_recovery(epoch, current.revocation, &paper)
            .is_err()
    );
    setup
        .reconcile_recovery(epoch, current.revocation, "wrong code")
        .unwrap();
    wait(&mut setup, State::RecoveryUnknown);
    assert_eq!(setup.status.error, Some(SetupError::InvalidCode));
    disk.0.lock().unwrap().fail_barrier = false;
    setup
        .reconcile_recovery(epoch, setup.status.revocation, &paper)
        .unwrap();
    wait(&mut setup, State::RecoverySaved);
    assert_eq!(disk.0.lock().unwrap().publications, publications);
    assert!(
        setup
            .credentials_list(epoch, setup.status.revocation, None)
            .is_err()
    );
    // Reattachment after the worker completes must still show its proven outcome.
    let storage = disk.clone();
    let resumed = setup
        .open_existing(Events::default(), move || Ok(storage))
        .unwrap();
    assert_eq!(resumed.epoch, epoch);
    assert_eq!(resumed.state, State::RecoverySaved);
}

#[test]
fn recovery_cancel_during_stage_or_admitted_barrier_preserves_authoritative_outcome() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    for admitted in [false, true] {
        let (disk, paper) = recoverable_disk();
        let mut setup = open_recoverable(&disk, Events::default());
        let epoch = setup.status.epoch;
        paper_ready(&mut setup, &paper);
        let gate = Arc::new(Gate::default());
        {
            let mut state = disk.0.lock().unwrap();
            if admitted {
                state.barrier_gate = Some(gate.clone());
            } else {
                state.stage_gate = Some(gate.clone());
            }
        }
        setup
            .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !gate.entered.load(Ordering::SeqCst) {
            setup.status();
            if Instant::now() >= deadline {
                gate.release();
                panic!("recovery worker did not reach gate");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        setup.cancel(epoch).unwrap();
        let storage = disk.clone();
        let status = setup
            .open_existing(Events::default(), move || Ok(storage))
            .unwrap();
        let retained = status.epoch == epoch && status.state == State::RecoverySaving;
        let refused = setup.lock_existing(epoch).is_err()
            && setup
                .unlock(epoch, status.revocation, "replacement")
                .is_err()
            && setup
                .credentials_list(epoch, status.revocation, None)
                .is_err();
        gate.release();
        assert!(retained && refused);
        wait(
            &mut setup,
            if admitted {
                State::RecoverySaved
            } else {
                State::RecoveryFailed
            },
        );
        assert_eq!(
            disk.0.lock().unwrap().publications,
            if admitted { 2 } else { 1 }
        );
        assert!(
            setup
                .credentials_list(epoch, setup.status.revocation, None)
                .is_err()
        );
    }
}

#[derive(Default, Clone)]
struct UnreliableEvents(Arc<AtomicBool>);
impl HeadlessEventSource for UnreliableEvents {
    fn take_revocation(&mut self) -> Result<Option<SessionEvent>, EventSourceError> {
        if self.0.load(Ordering::SeqCst) {
            Err(EventSourceError)
        } else {
            Ok(None)
        }
    }
    fn wait(&mut self, _: Duration) -> Result<(), EventSourceError> {
        Ok(())
    }
}
#[test]
fn recovery_unknown_without_event_source_reports_restart_required() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let (disk, paper) = recoverable_disk();
    let storage = disk.clone();
    let source = UnreliableEvents::default();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(source.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    let deadline = Instant::now() + Duration::from_secs(15);
    while setup.status().state != State::Locked {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    setup
        .begin_recovery(epoch, setup.status.revocation, &paper)
        .unwrap();
    while setup.status().state != State::RecoveryReady {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    disk.0.lock().unwrap().fail_barrier = true;
    setup
        .complete_recovery(epoch, setup.status.revocation, "replacement", "replacement")
        .unwrap();
    while setup.status().state != State::RecoveryUnknown {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    source.0.store(true, Ordering::SeqCst);
    let status = setup.status();
    assert_eq!(status.state, State::RecoveryUnknown);
    assert_eq!(status.error, Some(SetupError::Unavailable));
    assert!(matches!(
        setup.reconcile_recovery(epoch, status.revocation, &paper),
        Err(SetupError::Unavailable)
    ));
    assert_eq!(setup.status().error, Some(SetupError::Unavailable));
}

#[test]
fn credential_search_bridge_keeps_pages_scoped_and_returns_committed_write_identity() {
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let storage = disk.clone();
    let events = Events::default();
    let mut setup = Setup::new();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    assert!(
        setup
            .credentials_search(epoch, setup.status.revocation, "", None)
            .is_err()
    );
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    assert!(matches!(
        setup.credentials_search(epoch, revocation, &"é".repeat(2049), None),
        Err(SetupError::InvalidInput)
    ));
    let mut ids = Vec::new();
    for _ in 0..2 {
        let page = setup
            .credentials_search(epoch, revocation, "no matches", None)
            .unwrap();
        assert!(page.summaries.is_empty());
        let operation = setup
            .credentials_write(
                credential_input(
                    epoch,
                    revocation,
                    &page.generation,
                    &page.revision,
                    None,
                    false,
                ),
                false,
            )
            .unwrap();
        assert!(operation.credential_id.is_none());
        assert_eq!(
            operation_wait(&mut setup, epoch, &operation.operation),
            "saved"
        );
        let saved = setup
            .credentials_operation(credentials::OperationInput {
                epoch,
                operation: operation.operation,
            })
            .unwrap();
        ids.push(saved.credential_id.unwrap());
    }
    assert_ne!(ids[0], ids[1]);
    let page = setup
        .credentials_search(epoch, revocation, "ACCOUNT", None)
        .unwrap();
    assert_eq!(page.summaries.len(), 2);
    for id in &ids {
        assert!(page.summaries.iter().any(|s| s.id.as_str() == id));
    }
    let operation = setup
        .credentials_write(
            credential_input(
                epoch,
                revocation,
                &page.generation,
                &page.revision,
                Some(ids[0].clone()),
                true,
            ),
            true,
        )
        .unwrap();
    assert_eq!(
        operation_wait(&mut setup, epoch, &operation.operation),
        "saved"
    );
    let saved = setup
        .credentials_operation(credentials::OperationInput {
            epoch,
            operation: operation.operation,
        })
        .unwrap();
    assert_eq!(saved.credential_id.as_deref(), Some(ids[0].as_str()));
    events.0.store(true, Ordering::SeqCst);
    let locked = setup
        .credentials_operation(credentials::OperationInput {
            epoch,
            operation: saved.operation,
        })
        .unwrap();
    assert_eq!(locked.state, "saved");
    assert!(locked.credential_id.is_none());
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let page = setup
        .credentials_search(epoch, revocation, "", None)
        .unwrap();
    let operation = setup
        .credentials_delete(selected(
            epoch,
            revocation,
            &page.generation,
            &page.revision,
            &ids[0],
        ))
        .unwrap();
    assert_eq!(
        operation_wait(&mut setup, epoch, &operation.operation),
        "saved"
    );
    assert!(
        setup
            .credentials_operation(credentials::OperationInput {
                epoch,
                operation: operation.operation
            })
            .unwrap()
            .credential_id
            .is_none()
    );
    events.0.store(true, Ordering::SeqCst);
    assert!(
        setup
            .credentials_search(epoch, revocation, "account", None)
            .is_err()
    );
}

#[test]
fn credential_search_bridge_paginates_whole_vault_and_keeps_overlapping_query_cursors() {
    use codeward_core::credentials::{Credential, CredentialInput, CredentialSnapshot};
    let _master = MASTER_SCENARIOS.lock().unwrap();
    let disk = saved_disk();
    let records = (0..205)
        .map(|index| {
            Credential::new(CredentialInput {
                id: &format!("{index:032x}"),
                title: "Shared account",
                username: if index == 204 { "Late match" } else { "user" },
                password: "hidden",
                url: "https://secret.test",
                notes: "private note",
                created_at: 0,
                updated_at: 0,
            })
            .unwrap()
        })
        .collect();
    // Install only a synthetic committed fixture into this in-memory storage.
    let snapshot = CredentialSnapshot::new(records).unwrap();
    let bytes = disk.0.lock().unwrap().bytes.clone().unwrap();
    let access =
        codeward_core::vault::unlock_with_master(&bytes, "synthetic master phrase").unwrap();
    disk.0.lock().unwrap().bytes =
        Some(access.prepare_contents(1, &snapshot).unwrap().into_bytes());
    drop(access);
    let mut setup = Setup::new();
    let storage = disk.clone();
    let events = Events::default();
    let epoch = setup
        .open_existing(events.clone(), move || Ok(storage))
        .unwrap()
        .epoch;
    wait(&mut setup, State::Locked);
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    let revocation = setup.status.revocation;
    let late = setup
        .credentials_search(epoch, revocation, "LATE", None)
        .unwrap();
    assert_eq!(late.summaries.len(), 1);
    assert_eq!(
        late.summaries[0].id.as_str(),
        "000000000000000000000000000000cc"
    );
    let first = setup
        .credentials_search(epoch, revocation, "SHARED", None)
        .unwrap();
    let unrelated = setup
        .credentials_search(epoch, revocation, "user", None)
        .unwrap();
    assert_eq!(first.summaries.len(), 100);
    assert_eq!(unrelated.summaries.len(), 100);
    let second = setup
        .credentials_search(epoch, revocation, "shared", first.next.as_deref())
        .unwrap();
    assert_eq!(second.summaries.len(), 100);
    assert_eq!(
        second.summaries[0].id.as_str(),
        "00000000000000000000000000000064"
    );
    let last = setup
        .credentials_search(epoch, revocation, "shared", second.next.as_deref())
        .unwrap();
    assert_eq!(last.summaries.len(), 5);
    assert!(last.next.is_none());
    assert!(matches!(
        setup.credentials_search(epoch, revocation, "different", unrelated.next.as_deref()),
        Err(SetupError::Stale)
    ));
    assert!(
        setup
            .credentials_search(epoch + 1, revocation, "", None)
            .is_err()
    );
    assert!(
        setup
            .credentials_search(epoch, revocation + 1, "", None)
            .is_err()
    );
    let before_lock = setup
        .credentials_search(epoch, revocation, "shared", None)
        .unwrap();
    events.0.store(true, Ordering::SeqCst);
    assert!(
        setup
            .credentials_search(epoch, revocation, "shared", before_lock.next.as_deref())
            .is_err()
    );
    setup
        .unlock(epoch, setup.status.revocation, "synthetic master phrase")
        .unwrap();
    wait(&mut setup, State::Unlocked);
    assert!(matches!(
        setup.credentials_search(
            epoch,
            setup.status.revocation,
            "shared",
            before_lock.next.as_deref()
        ),
        Err(SetupError::Stale)
    ));
}
