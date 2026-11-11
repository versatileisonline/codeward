//! Shared synchronous and worker-side candidate preparation.

use std::borrow::Borrow;

use hmac::{Hmac, KeyInit, Mac};

use super::*;
use crate::{credentials::CredentialSnapshot, recovery::RecoveryError};

pub(in crate::transaction) enum WriteInput<S, T> {
    Contents(S),
    Recovery {
        password: T,
        confirmation: T,
    },
    Creation {
        password: T,
        confirmation: T,
        paper: T,
    },
}

type BorrowedWrite<'a> = WriteInput<&'a CredentialSnapshot, &'a str>;
pub(in crate::transaction) type OwnedWrite = WriteInput<CredentialSnapshot, Zeroizing<String>>;

impl<S: Borrow<CredentialSnapshot>, T: AsRef<str>> WriteInput<S, T> {
    fn validate(&self, access: &Access) -> Result<(), TransactionError> {
        match (self, access) {
            (Self::Contents(_), Access::Master(_)) => Ok(()),
            (
                Self::Recovery {
                    password,
                    confirmation,
                },
                Access::Recovery(_),
            ) => check_password_bounds(password.as_ref(), confirmation.as_ref()),
            (
                Self::Creation {
                    password,
                    confirmation,
                    paper,
                },
                Access::Enrollment(_),
            ) => {
                check_password_bounds(password.as_ref(), confirmation.as_ref())?;
                check_paper_bounds(paper.as_ref())
            }
            _ => Err(TransactionError::Locked),
        }
    }

    fn intent(&self, key: &SecretKey, request: Request) -> Result<[u8; 32], TransactionError> {
        let mut mac = Hmac::<Sha256>::new_from_slice(key.bytes()).expect("fixed HMAC key length");
        // Per-generation secret avoids retaining an offline password verifier.
        mac.update(b"Codeward volatile request v1\0");
        mac.update(match self {
            Self::Contents(_) => b"contents",
            Self::Recovery { .. } => b"recovery",
            Self::Creation { .. } => b"creation",
        });
        mac.update(&request.expected_revision.to_le_bytes());
        let mut field = |bytes: &[u8]| {
            mac.update(&(bytes.len() as u64).to_le_bytes());
            mac.update(bytes);
        };
        match self {
            Self::Contents(snapshot) => {
                field(&snapshot.borrow().encode().map_err(VaultError::from)?)
            }
            Self::Recovery {
                password,
                confirmation,
            } => {
                field(password.as_ref().as_bytes());
                field(confirmation.as_ref().as_bytes());
            }
            Self::Creation {
                password,
                confirmation,
                paper,
            } => {
                field(password.as_ref().as_bytes());
                field(confirmation.as_ref().as_bytes());
                field(paper.as_ref().as_bytes());
            }
        }
        Ok(mac.finalize().into_bytes().into())
    }
}

pub(in crate::transaction) fn check_paper_bounds(paper: &str) -> Result<(), TransactionError> {
    if paper.len() > 256 {
        return Err(VaultError::Recovery(RecoveryError::InvalidCode).into());
    }
    Ok(())
}

pub(in crate::transaction) enum PreparedWrite {
    Known(OperationRef),
    New(OperationRef),
}

/// The worker borrows no session and exclusively owns the original access state.
pub(in crate::transaction) struct WritePreparation {
    access: Access,
    intent_key: SecretKey,
    source: Option<Fingerprint>,
    generation: u64,
    permit: Arc<Permit>,
}

impl WritePreparation {
    pub(in crate::transaction) fn retain_if_current(self) -> Option<Self> {
        (!self.permit.canceled()).then_some(self)
    }

    pub(in crate::transaction) fn run<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        request: Request,
        input: WriteInput<impl Borrow<CredentialSnapshot>, impl AsRef<str>>,
    ) -> Result<PreparedWrite, TransactionError> {
        if self.permit.canceled() {
            return Err(TransactionError::Locked);
        }
        let intent = input.intent(&self.intent_key, request)?;
        if let Some(prior) = worker.prior(request, self.generation, &intent)? {
            return Ok(PreparedWrite::Known(prior));
        }
        if self.source != worker.fingerprint {
            return Err(TransactionError::ReloadRequired);
        }
        if request.expected_revision != self.source.map_or(0, |source| source.revision) {
            return Err(VaultError::RevisionConflict.into());
        }
        if self.permit.canceled() {
            return Err(TransactionError::Locked);
        }
        let candidate = match (input, &mut self.access) {
            (WriteInput::Contents(snapshot), Access::Master(access)) => {
                access.prepare_contents(request.expected_revision, snapshot.borrow())?
            }
            (
                WriteInput::Recovery {
                    password,
                    confirmation,
                },
                Access::Recovery(access),
            ) => access.prepare_master_password(
                request.expected_revision,
                password.as_ref(),
                confirmation.as_ref(),
            )?,
            (
                WriteInput::Creation {
                    password,
                    confirmation,
                    paper,
                },
                Access::Enrollment(access),
            ) => access.prepare(password.as_ref(), confirmation.as_ref(), paper.as_ref())?,
            _ => unreachable!("preparation requires matching access"),
        };
        Ok(PreparedWrite::New(worker.submit(
            request,
            self.generation,
            intent,
            candidate,
            self.permit.clone(),
        )?))
    }
}

impl Session {
    /// Admit an explicitly requested whole-snapshot content edit/deletion.
    /// Identical known requests return their existing reference before encryption.
    pub fn prepare_contents<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        snapshot: &CredentialSnapshot,
    ) -> Result<OperationRef, TransactionError> {
        self.prepare_write(worker, caller, request, BorrowedWrite::Contents(snapshot))
    }

    /// Admit a coherent replacement master slot/payload in restricted recovery.
    /// No password is retained in request history.
    pub fn prepare_recovery<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        password: &str,
        confirmation: &str,
    ) -> Result<OperationRef, TransactionError> {
        self.prepare_write(
            worker,
            caller,
            request,
            BorrowedWrite::Recovery {
                password,
                confirmation,
            },
        )
    }

    /// Confirm enrollment and admit creation with pending, unsaved status.
    /// Initial publication must atomically refuse an existing file.
    pub fn prepare_creation<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        password: &str,
        confirmation: &str,
        paper: &str,
    ) -> Result<OperationRef, TransactionError> {
        self.prepare_write(
            worker,
            caller,
            request,
            BorrowedWrite::Creation {
                password,
                confirmation,
                paper,
            },
        )
    }

    fn prepare_write<L: WriterLease>(
        &mut self,
        worker: &mut Committer<L>,
        caller: CallerId,
        request: Request,
        input: BorrowedWrite<'_>,
    ) -> Result<OperationRef, TransactionError> {
        let mut preparation = self.begin_preparation(worker, caller, &input)?;
        let result = preparation.run(worker, request, input);
        self.finish_preparation(preparation, &result);
        result.map(|prepared| match prepared {
            PreparedWrite::Known(operation) | PreparedWrite::New(operation) => operation,
        })
    }

    pub(in crate::transaction) fn begin_preparation<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        input: &WriteInput<impl Borrow<CredentialSnapshot>, impl AsRef<str>>,
    ) -> Result<WritePreparation, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        let access = self.access.as_ref().ok_or(TransactionError::Locked)?;
        input.validate(access)?;
        let intent_key = self.intent_key.take().ok_or(TransactionError::Locked)?;
        let permit = Arc::new(Permit::new());
        self.worker_access = Some(permit.clone());
        Ok(WritePreparation {
            access: self.access.take().expect("authorized access"),
            intent_key,
            source: self.source,
            generation: self.generation,
            permit,
        })
    }

    pub(in crate::transaction) fn finish_preparation(
        &mut self,
        preparation: WritePreparation,
        result: &Result<PreparedWrite, TransactionError>,
    ) -> bool {
        if !self
            .worker_access
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &preparation.permit))
        {
            return false;
        }
        self.worker_access = None;
        self.access = Some(preparation.access);
        self.intent_key = Some(preparation.intent_key);
        if matches!(result, Ok(PreparedWrite::New(_))) {
            self.permit = Some(preparation.permit);
        }
        true
    }
}
