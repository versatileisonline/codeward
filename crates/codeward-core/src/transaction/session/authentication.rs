use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

pub(super) struct Attempt {
    canceled: AtomicBool,
    generation: u64,
    source: Fingerprint,
}

impl Attempt {
    pub(super) fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    fn check(&self) -> Result<(), TransactionError> {
        if self.canceled.load(Ordering::SeqCst) {
            Err(TransactionError::AuthenticationCanceled)
        } else {
            Ok(())
        }
    }
}

enum Kind {
    Master,
    Recovery,
}

impl Kind {
    fn authenticate(&self, bytes: &[u8], secret: &str) -> Result<Access, TransactionError> {
        match self {
            Self::Master => Ok(Access::Master(vault::unlock_with_master(bytes, secret)?)),
            Self::Recovery => Ok(Access::Recovery(vault::begin_recovery(bytes, secret)?)),
        }
    }
}

/// Opaque, single-use authentication work owned by Rust, never sent to the UI.
///
/// Move to a worker thread without borrowing the session or storage. Drop clears
/// the owned input; cancellation cannot interrupt a KDF already executing, but
/// its result cannot restore revoked access. Caller-owned input copies are separate.
/// A job is consumed when run and cannot be replayed:
///
/// ```compile_fail
/// use codeward_core::transaction::AuthenticationJob;
/// fn replay(job: AuthenticationJob) {
///     let first = job.run();
///     let second = job.run();
/// }
/// ```
pub struct AuthenticationJob {
    attempt: Arc<Attempt>,
    bytes: Vec<u8>,
    secret: Zeroizing<String>,
    kind: Kind,
    slot: Arc<()>,
}

/// Opaque authentication completion; carries no public keys or credential fields.
///
/// Deliver only to `Session::complete_authentication`. Drop releases any successful
/// access still owned here. Completion alone never unlocks a session or saves data.
/// Hosts must promptly deliver or drop completions, including after lock/close;
/// lock cannot erase an already returned result held elsewhere in Rust.
/// Secret owners intentionally have no diagnostic formatting:
///
/// ```compile_fail
/// use codeward_core::transaction::AuthenticationResult;
/// fn diagnose(result: AuthenticationResult) {
///     println!("{result:?}");
/// }
/// ```
pub struct AuthenticationResult {
    attempt: Arc<Attempt>,
    access: Result<Access, TransactionError>,
}

impl AuthenticationJob {
    /// Authenticate once, checking revocation before and after expensive work.
    /// The existing process-wide master-derivation guard remains in force.
    pub fn run(self) -> AuthenticationResult {
        self.run_with(|kind, bytes, secret| kind.authenticate(bytes, secret))
    }

    #[cfg(test)]
    pub(in crate::transaction) fn run_after(self, wait: impl FnOnce()) -> AuthenticationResult {
        self.run_with(|kind, bytes, secret| {
            wait();
            kind.authenticate(bytes, secret)
        })
    }

    fn run_with(
        self,
        authenticate: impl FnOnce(&Kind, &[u8], &str) -> Result<Access, TransactionError>,
    ) -> AuthenticationResult {
        let Self {
            attempt,
            bytes,
            secret,
            kind,
            slot,
        } = self;
        let access = attempt.check().and_then(|()| {
            let access = authenticate(&kind, &bytes, &secret);
            attempt.check()?;
            access
        });
        drop(secret);
        drop(slot);
        AuthenticationResult { attempt, access }
    }
}

impl Session {
    /// Revoke current access and prepare ordinary authentication of the worker's
    /// exact committed base. At most one queued/running job per session is allowed.
    /// Keep this session on the control thread so lock can revoke during `run`.
    pub fn begin_master_authentication<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        password: &str,
    ) -> Result<AuthenticationJob, TransactionError> {
        self.begin_authentication(worker, caller, password, Kind::Master)
    }

    /// Prepare restricted paper authentication; successful completion cannot browse
    /// credentials or report that a replacement master password has committed.
    pub fn begin_recovery_authentication<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        paper: &str,
    ) -> Result<AuthenticationJob, TransactionError> {
        self.begin_authentication(worker, caller, paper, Kind::Recovery)
    }

    fn begin_authentication<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        secret: &str,
        kind: Kind,
    ) -> Result<AuthenticationJob, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        worker.check_idle()?;
        if self.authentication_job.upgrade().is_some() {
            return Err(TransactionError::Busy);
        }
        self.lock();
        if self.exhausted {
            return Err(TransactionError::GenerationExhausted);
        }
        match kind {
            Kind::Master => check_password_bounds(secret, secret)?,
            Kind::Recovery if secret.len() > 256 => {
                return Err(
                    VaultError::Recovery(crate::recovery::RecoveryError::InvalidCode).into(),
                );
            }
            Kind::Recovery => (),
        }
        let base = worker.base_bytes()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(base.len())
            .map_err(|_| TransactionError::AllocationFailed)?;
        bytes.extend_from_slice(base);
        let mut owned_secret = Zeroizing::new(String::new());
        owned_secret
            .try_reserve_exact(secret.len())
            .map_err(|_| TransactionError::AllocationFailed)?;
        owned_secret.push_str(secret);
        let attempt = Arc::new(Attempt {
            canceled: AtomicBool::new(false),
            generation: self.generation,
            source: worker.fingerprint.ok_or(TransactionError::ReloadRequired)?,
        });
        let slot = Arc::new(());
        self.authentication_job = Arc::downgrade(&slot);
        self.authentication = Some(attempt.clone());
        self.start_inactivity_at(std::time::Instant::now())?;
        Ok(AuthenticationJob {
            attempt,
            bytes,
            secret: owned_secret,
            kind,
            slot,
        })
    }

    /// Install only the current attempt for this caller/opened vault/generation and
    /// unchanged committed base. Stale completions cannot revoke newer access.
    /// Failed or canceled current authentication leaves the session locked.
    pub fn complete_authentication<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        result: AuthenticationResult,
    ) -> Result<(), TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        let current = self
            .authentication
            .as_ref()
            .is_some_and(|attempt| Arc::ptr_eq(attempt, &result.attempt));
        if !current || result.attempt.generation != self.generation {
            return Err(TransactionError::AuthenticationCanceled);
        }
        self.authentication = None;
        let completion = (|| {
            result.attempt.check()?;
            worker.check_idle()?;
            if worker.fingerprint != Some(result.attempt.source) {
                return Err(TransactionError::ReloadRequired);
            }
            self.install(worker, result.access?)
        })();
        if completion.is_err() {
            self.inactivity.disarm();
        }
        completion
    }
}
