use super::*;
use crate::credentials::copy_text;

#[cfg(test)]
mod tests;

/// Maximum number of credential summaries in a single page under ADR-0001.
pub const MAX_CREDENTIAL_SUMMARIES: usize = 100;

/// Opaque continuation for one caller, opened vault, generation and revision.
pub struct CredentialCursor {
    scope: Arc<()>,
    caller: CallerId,
    generation: u64,
    source: Fingerprint,
    offset: usize,
    query: Zeroizing<String>,
}

/// Private display fields for a list result. Owned text clears on drop.
/// This type has no diagnostic formatting or serialization.
pub struct CredentialSummary {
    /// Record identifier within the committed snapshot.
    pub id: Zeroizing<String>,
    /// Exact stored title.
    pub title: Zeroizing<String>,
    /// Exact stored username, which can be empty.
    pub username: Zeroizing<String>,
}

/// Bounded result targeted to the trusted caller of `Session::list_credentials`.
/// The host retains the original caller and opened-vault context when dispatching
/// and drops the result if that context, revision or generation is no longer current.
pub struct CredentialPage {
    /// Session generation used to authorize this read.
    pub generation: u64,
    /// Authenticated committed revision read by this request.
    pub revision: u64,
    /// Summaries in ascending record-ID order.
    pub summaries: Vec<CredentialSummary>,
    /// Continuation when more records exist at the same revision.
    pub next: Option<CredentialCursor>,
}

impl Session {
    /// List a page from current ordinary access for the trusted caller. Pass no
    /// cursor to start. Pending/uncertain writes and observed conflicts prevent
    /// listing, and a completed write requires a current authenticated snapshot.
    /// The host must poll and recheck scope/generation/revision before delivering
    /// a queued page. Lock cannot clear copies the caller already owns.
    pub fn list_credentials<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        limit: usize,
        cursor: Option<&CredentialCursor>,
    ) -> Result<CredentialPage, TransactionError> {
        self.search_credentials(worker, caller, generation, "", limit, cursor)
    }

    /// Case-insensitive substring search of title and username independently.
    /// Queries retain exact whitespace and are limited to 4,096 UTF-8 bytes.
    /// Continuations bind the normalized query to the ordinary listing scope.
    #[allow(clippy::too_many_arguments)]
    pub fn search_credentials<L: WriterLease>(
        &mut self,
        worker: &Committer<L>,
        caller: CallerId,
        generation: u64,
        query: &str,
        limit: usize,
        cursor: Option<&CredentialCursor>,
    ) -> Result<CredentialPage, TransactionError> {
        self.check_scope(worker, caller)?;
        self.poll_inactivity();
        let Some(Access::Master(access)) = self.access.as_ref() else {
            return Err(TransactionError::Locked);
        };
        if generation != self.generation {
            return Err(TransactionError::Locked);
        }
        if !(1..=MAX_CREDENTIAL_SUMMARIES).contains(&limit) {
            return Err(TransactionError::InvalidPageSize);
        }
        if query.len() > 4096 {
            return Err(TransactionError::InvalidSearchQuery);
        }
        let query = Zeroizing::new(query.to_lowercase());
        worker.check_idle()?;
        let source = self.source.ok_or(TransactionError::ReloadRequired)?;
        if Some(source) != worker.fingerprint {
            return Err(TransactionError::ReloadRequired);
        }
        let offset = if let Some(cursor) = cursor {
            if cursor.caller != caller || !Arc::ptr_eq(&cursor.scope, &self.scope) {
                return Err(TransactionError::Unauthorized);
            }
            if cursor.generation != generation || cursor.source != source || cursor.query != query {
                return Err(TransactionError::StaleCursor);
            }
            cursor.offset
        } else {
            0
        };
        let records = access.snapshot().credentials();
        let mut matches = records
            .iter()
            .enumerate()
            .skip(offset)
            .filter(|(_, record)| {
                let fields = record.fields();
                query.is_empty()
                    || Zeroizing::new(fields.title.to_lowercase()).contains(query.as_str())
                    || Zeroizing::new(fields.username.to_lowercase()).contains(query.as_str())
            });
        let mut summaries = Vec::new();
        summaries
            .try_reserve_exact(limit)
            .map_err(|_| TransactionError::AllocationFailed)?;
        for (_, record) in matches.by_ref().take(limit) {
            let fields = record.fields();
            summaries.push(CredentialSummary {
                id: copy_text(fields.id).map_err(VaultError::from)?,
                title: copy_text(fields.title).map_err(VaultError::from)?,
                username: copy_text(fields.username).map_err(VaultError::from)?,
            });
        }
        let next_offset = matches.next().map(|(index, _)| index);
        if self.poll_inactivity() {
            return Err(TransactionError::Locked);
        }
        Ok(CredentialPage {
            generation,
            revision: source.revision,
            summaries,
            next: next_offset.map(|offset| CredentialCursor {
                scope: self.scope.clone(),
                caller,
                generation,
                source,
                offset,
                query,
            }),
        })
    }
}
