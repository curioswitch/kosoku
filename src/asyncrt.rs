//! Async runtime glue.

use pyo3::{PyResult, Python, exceptions::PyRuntimeError, sync::PyOnceLock};
use tokio::runtime::Runtime;

/// The shared multi-threaded tokio runtime, created on first use.
pub fn get_runtime(py: Python<'_>) -> PyResult<&'static Runtime> {
    static RT: PyOnceLock<Runtime> = PyOnceLock::new();
    RT.get_or_try_init(py, || {
        Runtime::new().map_err(|_| PyRuntimeError::new_err("failed to initialize tokio runtime"))
    })
}

/// Attaches to Python from within an async tokio task and runs the provided
/// function.
pub async fn attach_blocking<R, F>(f: F) -> PyResult<R>
where
    F: for<'py> FnOnce(Python<'py>) -> PyResult<R> + Send + 'static,
    R: Send + 'static,
{
    match tokio::task::spawn_blocking(move || Python::attach(f)).await {
        Ok(result) => result,
        Err(join_error) => Err(PyRuntimeError::new_err(format!(
            "python worker task failed: {join_error}"
        ))),
    }
}
