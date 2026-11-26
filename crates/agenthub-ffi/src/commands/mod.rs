pub(crate) mod dispatch;
pub(crate) mod plans;

pub(crate) async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    agenthub_core::read_tasks::spawn_blocking(task)
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}
