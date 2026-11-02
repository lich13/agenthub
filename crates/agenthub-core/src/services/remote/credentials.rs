//! Serialize blocking credential-store access without blocking the async runtime.
//!
//! Native credential APIs cannot be cancelled once entered. A timed-out read
//! therefore keeps its permit inside the worker until that worker really exits.
//! Transactions time out only while waiting for the credential gate. Once a
//! worker is submitted, callers await its definite result, including any wait
//! in Tokio's blocking pool, rather than reporting a pending write as cancelled.
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

struct CredentialWorker<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for CredentialWorker<T> {
    fn drop(&mut self) {
        // A task still queued in Tokio's blocking pool can be cancelled. A
        // native call already running cannot; it continues to own the permit.
        self.0.abort();
    }
}

#[derive(Clone)]
pub struct CredentialAccess {
    gate: Arc<Semaphore>,
}

impl Default for CredentialAccess {
    fn default() -> Self {
        Self {
            gate: Arc::new(Semaphore::new(crate::resource_limits::CREDENTIAL_WORKERS)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialAccessError {
    TimedOut,
    WorkerUnavailable,
}

impl fmt::Display for CredentialAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TimedOut => "credential access timed out",
            Self::WorkerUnavailable => "credential worker unavailable",
        })
    }
}

impl std::error::Error for CredentialAccessError {}

impl CredentialAccess {
    pub async fn read<T, F>(&self, timeout: Duration, work: F) -> Result<T, CredentialAccessError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        tokio::time::timeout(timeout, async {
            let permit = self.acquire().await?;
            Self::execute(permit, work).await
        })
        .await
        .map_err(|_| CredentialAccessError::TimedOut)?
    }

    pub async fn transaction<T, F>(
        &self,
        queue_timeout: Duration,
        work: F,
    ) -> Result<T, CredentialAccessError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let permit = tokio::time::timeout(queue_timeout, self.acquire())
            .await
            .map_err(|_| CredentialAccessError::TimedOut)??;
        Self::execute(permit, work).await
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, CredentialAccessError> {
        self.gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CredentialAccessError::WorkerUnavailable)
    }

    async fn execute<T, F>(
        permit: OwnedSemaphorePermit,
        work: F,
    ) -> Result<T, CredentialAccessError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let mut worker = CredentialWorker(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        }));
        // Join errors may contain a panic payload. Never return that payload
        // across a credential boundary.
        (&mut worker.0)
            .await
            .map_err(|_| CredentialAccessError::WorkerUnavailable)
    }
}
