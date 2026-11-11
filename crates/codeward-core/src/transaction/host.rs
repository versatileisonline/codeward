//! Headless control-side orchestration for one opened writer and trusted caller.

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Instant,
};

use zeroize::Zeroizing;

use super::session::{
    check_password_bounds,
    write::{OwnedWrite, PreparedWrite, WritePreparation, check_paper_bounds},
};
use super::*;
use crate::credentials::CredentialSnapshot;

/// Borrowed authentication input owned by the trusted caller.
pub enum AccessProof<'a> {
    /// Ordinary master-password authentication.
    Master(&'a str),
    /// Restricted paper-code authentication.
    Paper(&'a str),
}

/// Controlled failures from the headless host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostError {
    /// A session or storage operation refused the request.
    Transaction(TransactionError),
    /// A worker failed to start or return. A lost writer requires reopening.
    WorkerUnavailable,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transaction(error) => error.fmt(f),
            Self::WorkerUnavailable => f.write_str("headless worker unavailable"),
        }
    }
}

impl Error for HostError {}

impl From<TransactionError> for HostError {
    fn from(error: TransactionError) -> Self {
        Self::Transaction(error)
    }
}

/// Targeted worker result for the trusted caller. Contains no credential data.
pub enum HostCompletion {
    /// The current authentication completed. Revoked results are discarded.
    Authentication(Result<(), TransactionError>),
    /// Preparation was refused before an operation reference could be issued.
    PreparationFailed {
        /// Request whose preparation failed.
        request: RequestId,
        /// Controlled error, including lock before preparation finished.
        error: TransactionError,
    },
    /// A commit, known duplicate or reconciliation returned a safe outcome.
    Transaction {
        /// Original operation reference, also usable for status lookup.
        operation: OperationRef,
        /// Recorded worker outcome, or the error from a reconciliation attempt.
        outcome: Result<OperationStatus, TransactionError>,
        /// Snapshot adoption for a successful ordinary save. An error here does
        /// not undo or change the independent commit outcome.
        refresh: Option<Result<(), TransactionError>>,
    },
}

struct FinishedWrite<L: WriterLease> {
    worker: Committer<L>,
    operation: OperationRef,
    outcome: Result<OperationStatus, TransactionError>,
    refresh: bool,
}

struct FinishedPreparation<L: WriterLease> {
    worker: Committer<L>,
    authorization: Option<WritePreparation>,
    result: Result<PreparedWrite, TransactionError>,
    request: RequestId,
    refresh: bool,
}

enum Pending<L: WriterLease> {
    Authentication(Receiver<AuthenticationResult>),
    Preparation(Receiver<FinishedPreparation<L>>),
    Prepared {
        operation: OperationRef,
        refresh: bool,
    },
    Write(Receiver<FinishedWrite<L>>),
}

/// One headless vault opened under a trusted Rust caller. Keep it on the
/// control thread. Workers never own the live session, so lock can revoke
/// access while authentication or storage is blocked.
///
/// Poll for worker results and schedule `inactivity_deadline` even when idle.
/// The control loop supplies wakeups, trusted events and asynchronous opening.
/// Direct constructors are synchronous. The host moves write preparation and
/// commits to workers and holds no drafts.
pub struct HeadlessVault<L: WriterLease + Send + 'static> {
    // Drop revokes access before releasing the worker or dropping its receiver.
    session: Session,
    caller: CallerId,
    pending: Option<Pending<L>>,
    worker: Option<Committer<L>>,
    #[cfg(test)]
    preparation_gate: Option<(mpsc::Sender<()>, Receiver<()>)>,
}

impl<L: WriterLease + Send + 'static> HeadlessVault<L> {
    /// Open an existing destination locked and retain writer exclusion.
    pub fn open<S: Storage<Lease = L>>(storage: S, caller: CallerId) -> Result<Self, HostError> {
        let (session, worker) = Committer::open_locked(storage, caller)?;
        Ok(Self::new(session, worker, caller))
    }

    /// Start volatile enrollment for an absent destination.
    pub fn begin_create<S: Storage<Lease = L>>(
        storage: S,
        caller: CallerId,
    ) -> Result<Self, HostError> {
        let (session, worker) = Committer::begin_create(storage, caller)?;
        Ok(Self::new(session, worker, caller))
    }

    fn new(session: Session, worker: Committer<L>, caller: CallerId) -> Self {
        Self {
            session,
            caller,
            worker: Some(worker),
            pending: None,
            #[cfg(test)]
            preparation_gate: None,
        }
    }

    /// Authenticate on a worker. Only a current, unrevoked completion can grant
    /// access. A paper code opens restricted recovery.
    pub fn begin_authentication(&mut self, proof: AccessProof<'_>) -> Result<(), HostError> {
        let (session, worker, caller) = self.idle()?;
        let job = match proof {
            AccessProof::Master(password) => {
                session.begin_master_authentication(worker, caller, password)?
            }
            AccessProof::Paper(paper) => {
                session.begin_recovery_authentication(worker, caller, paper)?
            }
        };
        self.pending = Some(Pending::Authentication(self.spawn(move || job.run())?));
        Ok(())
    }

    /// Display the temporary paper code during enrollment.
    pub fn recovery_code(&mut self) -> Result<Zeroizing<String>, HostError> {
        self.poll_deadline();
        let code = self.session.recovery_code(self.caller);
        self.discard_revoked_authentication();
        Ok(code?)
    }

    /// Restart an uncommitted enrollment with a fresh code.
    pub fn restart_enrollment(&mut self) -> Result<(), HostError> {
        let (session, worker, caller) = self.idle()?;
        Ok(session.restart_enrollment(worker, caller)?)
    }

    /// Queue confirmation and creation. Poll for the result; only a committed
    /// creation establishes readiness.
    pub fn create(
        &mut self,
        request: Request,
        password: &str,
        confirmation: &str,
        paper: &str,
    ) -> Result<(), HostError> {
        check_password_bounds(password, confirmation)?;
        check_paper_bounds(paper)?;
        self.dispatch_preparation(
            request,
            OwnedWrite::Creation {
                password: own_secret(password)?,
                confirmation: own_secret(confirmation)?,
                paper: own_secret(paper)?,
            },
        )
    }

    /// Queue an explicit content save and transfer ownership of its snapshot.
    /// Poll for the outcome. A duplicate reports its recorded status without replay.
    pub fn save(
        &mut self,
        request: Request,
        snapshot: CredentialSnapshot,
    ) -> Result<(), HostError> {
        self.dispatch_preparation(request, OwnedWrite::Contents(snapshot))
    }

    /// Queue replacement of the master password from restricted recovery.
    /// Poll for the outcome before reporting recovery complete.
    pub fn replace_master_password(
        &mut self,
        request: Request,
        password: &str,
        confirmation: &str,
    ) -> Result<(), HostError> {
        check_password_bounds(password, confirmation)?;
        self.dispatch_preparation(
            request,
            OwnedWrite::Recovery {
                password: own_secret(password)?,
                confirmation: own_secret(confirmation)?,
            },
        )
    }

    /// Reconcile the current uncertain operation using fresh proof. Successful
    /// reconciliation reports the proven state. A changed base needs fresh
    /// authentication before further ordinary access.
    pub fn reconcile(
        &mut self,
        operation: &OperationRef,
        proof: AccessProof<'_>,
    ) -> Result<(), HostError> {
        let (_, worker, caller) = self.idle()?;
        if worker.status(caller, operation)? != OperationStatus::OutcomeUnknown {
            return Err(TransactionError::OutcomeUnknown.into());
        }
        let (input, paper) = match proof {
            AccessProof::Master(password) => {
                check_password_bounds(password, password)?;
                (password, false)
            }
            AccessProof::Paper(code) => {
                check_paper_bounds(code)?;
                (code, true)
            }
        };
        let owned = own_secret(input)?;
        self.dispatch_write(
            operation.clone(),
            false,
            move |worker, caller, operation| {
                if paper {
                    worker.reconcile_recovery(caller, operation, &owned)
                } else {
                    worker.reconcile_master(caller, operation, &owned)
                }
            },
        )
    }

    /// Look up an operation after the writer returns, including after lock.
    pub fn status(&mut self, operation: &OperationRef) -> Result<OperationStatus, HostError> {
        let (_, worker, caller) = self.idle()?;
        Ok(worker.status(caller, operation)?)
    }

    /// List only current ordinary access. The trusted caller owns delivery scope.
    pub fn list_credentials(
        &mut self,
        generation: u64,
        limit: usize,
        cursor: Option<&CredentialCursor>,
    ) -> Result<CredentialPage, HostError> {
        let (session, worker, caller) = self.idle()?;
        Ok(session.list_credentials(worker, caller, generation, limit, cursor)?)
    }

    /// Current scoped generation. Poll before composing an access-state reply.
    pub fn generation(&self) -> u64 {
        self.session.generation()
    }

    /// Check expiry and whether the session has released its access state.
    pub fn is_locked(&mut self) -> bool {
        self.poll_deadline();
        self.session.is_locked()
    }

    /// Monotonic deadline the control loop should schedule, even without work.
    pub fn inactivity_deadline(&self) -> Option<Instant> {
        self.session.inactivity_deadline()
    }

    /// Whether a worker result is outstanding. An already revoked authentication
    /// can still be finishing its KDF without a result to poll.
    pub fn has_pending_work(&self) -> bool {
        self.pending.is_some()
    }

    /// Obtain a scoped interaction reference for the current session.
    pub fn activity_reference(&mut self) -> Result<ActivityRef, HostError> {
        self.poll_deadline();
        let reference = self.session.activity_reference(self.caller);
        self.discard_revoked_authentication();
        Ok(reference?)
    }

    /// Record a scoped interaction without requiring the storage worker.
    pub fn record_activity(
        &mut self,
        reference: &ActivityRef,
        activity: SessionActivity,
    ) -> Result<(), HostError> {
        self.poll_deadline();
        let result = self
            .session
            .record_activity(self.caller, reference, activity);
        self.discard_revoked_authentication();
        Ok(result?)
    }

    /// Deliver a trusted event. Security events revoke access immediately.
    pub fn observe_event(&mut self, event: SessionEvent) -> bool {
        let revoked = self.session.observe_event(event);
        if revoked {
            self.discard_authentication();
        }
        revoked
    }

    /// Revoke access without waiting for authentication, preparation or storage.
    pub fn lock(&mut self) {
        self.session.lock();
        self.discard_authentication();
    }

    /// Revoke and close without joining an in-flight worker. An admitted commit retains
    /// its worker and lease until it resolves. Reopening checks the actual file.
    pub fn close(mut self) {
        self.lock();
    }

    /// Check expiry and advance one worker handoff without waiting. Keep polling
    /// while `has_pending_work` is true, even if no completion is ready.
    pub fn poll(&mut self) -> Result<Option<HostCompletion>, HostError> {
        self.poll_deadline();
        let completion = match &self.pending {
            Some(Pending::Authentication(receiver)) => receiver.try_recv().map(|result| {
                let worker = self.worker.as_ref().expect("authentication retains writer");
                HostCompletion::Authentication(self.session.complete_authentication(
                    worker,
                    self.caller,
                    result,
                ))
            }),
            Some(Pending::Preparation(receiver)) => match receiver.try_recv() {
                Ok(done) => return self.finish_preparation(done),
                Err(error) => Err(error),
            },
            Some(Pending::Prepared { operation, refresh }) => {
                let (operation, refresh) = (operation.clone(), *refresh);
                self.pending = None;
                self.dispatch_prepared(operation, refresh)?;
                return Ok(None);
            }
            Some(Pending::Write(receiver)) => receiver.try_recv().map(|done| {
                let refresh = (done.refresh
                    && matches!(done.outcome, Ok(OperationStatus::Committed { .. })))
                .then(|| {
                    self.session.refresh_committed_contents(
                        &done.worker,
                        self.caller,
                        &done.operation,
                    )
                });
                self.worker = Some(done.worker);
                HostCompletion::Transaction {
                    operation: done.operation,
                    outcome: done.outcome,
                    refresh,
                }
            }),
            None => return Ok(None),
        };
        match completion {
            Ok(completion) => {
                self.pending = None;
                Ok(Some(completion))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.lock();
                Err(HostError::WorkerUnavailable)
            }
        }
    }

    fn poll_deadline(&mut self) {
        self.session.poll_inactivity();
        self.discard_revoked_authentication();
    }

    pub(super) fn take_inactivity_revocation(&mut self) -> bool {
        self.poll_deadline();
        let revoked = self.session.take_inactivity_revocation();
        // A command can observe expiry internally and then start fresh access.
        // If its reply is revoked, revoke that newly armed state as well.
        if revoked && self.session.inactivity_deadline().is_some() {
            self.lock();
        }
        revoked
    }

    #[cfg(test)]
    pub(super) fn set_deadline_for_test(&mut self, deadline: Instant) {
        self.session.set_deadline_for_test(deadline);
    }

    #[cfg(test)]
    pub(super) fn pause_preparation(&mut self) -> (Receiver<()>, mpsc::Sender<()>) {
        let (entered, checkpoint) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        self.preparation_gate = Some((entered, wait));
        (checkpoint, resume)
    }

    fn discard_revoked_authentication(&mut self) {
        if self.session.inactivity_deadline().is_none() {
            self.discard_authentication();
        }
    }

    fn discard_authentication(&mut self) {
        if matches!(self.pending, Some(Pending::Authentication(_))) {
            self.pending = None;
        }
    }

    fn idle(&mut self) -> Result<(&mut Session, &mut Committer<L>, CallerId), HostError> {
        self.poll_deadline();
        if self.pending.is_some() {
            return Err(TransactionError::Busy.into());
        }
        let worker = self.worker.as_mut().ok_or(HostError::WorkerUnavailable)?;
        Ok((&mut self.session, worker, self.caller))
    }

    fn dispatch_preparation(
        &mut self,
        request: Request,
        input: OwnedWrite,
    ) -> Result<(), HostError> {
        let (session, worker, caller) = self.idle()?;
        let mut authorization = session.begin_preparation(worker, caller, &input)?;
        let refresh = matches!(input, OwnedWrite::Contents(_));
        let mut worker = self
            .worker
            .take()
            .expect("preparation requires idle writer");
        #[cfg(test)]
        let gate = self.preparation_gate.take();
        self.pending = Some(Pending::Preparation(self.spawn(move || {
            #[cfg(test)]
            if let Some((entered, wait)) = gate {
                entered.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
            }
            let result = authorization.run(&mut worker, request, input);
            let authorization = authorization.retain_if_current();
            FinishedPreparation {
                worker,
                authorization,
                result,
                request: request.id,
                refresh,
            }
        })?));
        Ok(())
    }

    fn finish_preparation(
        &mut self,
        done: FinishedPreparation<L>,
    ) -> Result<Option<HostCompletion>, HostError> {
        self.poll_deadline();
        let current = done.authorization.is_some_and(|authorization| {
            self.session.finish_preparation(authorization, &done.result)
        });
        self.pending = None;
        self.worker = Some(done.worker);
        let error = match done.result {
            Ok(PreparedWrite::New(operation)) => {
                // Give the control loop another event check before any storage
                // work. A canceled permit drains without touching storage.
                self.pending = Some(Pending::Prepared {
                    operation,
                    refresh: done.refresh,
                });
                return Ok(None);
            }
            Ok(PreparedWrite::Known(operation)) if current => {
                let outcome = self.status(&operation)?;
                return Ok(Some(HostCompletion::Transaction {
                    operation,
                    outcome: Ok(outcome),
                    refresh: None,
                }));
            }
            Err(error) if current => error,
            _ => TransactionError::Locked,
        };
        Ok(Some(HostCompletion::PreparationFailed {
            request: done.request,
            error,
        }))
    }

    fn dispatch_prepared(
        &mut self,
        operation: OperationRef,
        refresh: bool,
    ) -> Result<(), HostError> {
        self.dispatch_write(operation, refresh, |worker, _, _| {
            loop {
                match worker.advance() {
                    Some(OperationStatus::Pending(_)) => continue,
                    Some(status) => return Ok(status),
                    None => return Err(TransactionError::OutcomeUnknown),
                }
            }
        })
    }

    fn dispatch_write(
        &mut self,
        operation: OperationRef,
        refresh: bool,
        run: impl FnOnce(
            &mut Committer<L>,
            CallerId,
            &OperationRef,
        ) -> Result<OperationStatus, TransactionError>
        + Send
        + 'static,
    ) -> Result<(), HostError> {
        let mut worker = self.worker.take().expect("dispatch requires idle writer");
        let caller = self.caller;
        self.pending = Some(Pending::Write(self.spawn(move || {
            let outcome = run(&mut worker, caller, &operation);
            FinishedWrite {
                worker,
                operation,
                outcome,
                refresh,
            }
        })?));
        Ok(())
    }

    fn spawn<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<Receiver<T>, HostError> {
        spawn_worker(work).inspect_err(|_| self.lock())
    }
}

fn own_secret(input: &str) -> Result<Zeroizing<String>, TransactionError> {
    let mut owned = Zeroizing::new(String::new());
    owned
        .try_reserve_exact(input.len())
        .map_err(|_| TransactionError::AllocationFailed)?;
    owned.push_str(input);
    Ok(owned)
}

pub(super) fn spawn_worker<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<Receiver<T>, HostError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .spawn(move || {
            let _ = sender.send(work());
        })
        .map_err(|_| HostError::WorkerUnavailable)?;
    Ok(receiver)
}

#[cfg(test)]
mod tests;
