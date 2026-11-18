pub mod claude;
pub mod grok;
pub mod jobs;
pub mod plans;
pub mod probe;
pub mod provider_sessions;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod threads;
pub mod updates;

/// Synchronous commands run on the main thread, so blocking work goes to the bounded pool.
async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    agenthub_core::read_tasks::spawn_blocking(task)
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}
