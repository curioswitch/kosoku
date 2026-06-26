//! kosoku: a native WebSocket protocol-compliance fuzzing client that
//! drives Autobahn `TestSuite`'s Python case files.

mod asyncrt;
mod client;
mod compress;
mod constants;
mod deflate;
mod protocol;
mod result;
mod runner;
mod server;
mod traffic;
mod utf8;
mod wirelog;

use std::collections::HashSet;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use regex::Regex;
use tokio::sync::Semaphore;

use crate::constants::Constants;
use crate::result::{CaseResult, Results, finalize_results};
use crate::server::FuzzingServer;

/// A case to drive.
pub(crate) struct CaseInner {
    /// The case id, e.g. `"1.1.1"`.
    pub(crate) id: String,
    /// The Python case class.
    pub(crate) class: Py<PyAny>,
}

#[derive(Clone)]
pub(crate) struct Case {
    inner: Arc<CaseInner>,
}

impl Deref for Case {
    type Target = CaseInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Case {
    pub(crate) fn new(id: String, class: Py<PyAny>) -> Self {
        Self {
            inner: Arc::new(CaseInner { id, class }),
        }
    }
}

/// Run the Autobahn test cases against a WebSocket server.
///
/// Opens one connection per case to the server under test and runs the selected
/// cases, returning their results in case order.
///
/// Args:
///     url: WebSocket URL of the server under test, e.g. `ws://localhost:9001`.
///     cases: Case ids or `*` globs to run, e.g. `["1.*", "9.7.1"]`. The whole
///         suite runs when omitted.
///     exclude_cases: Case ids or `*` globs to remove from the selection.
///     concurrency: How many cases to run in parallel.
///
/// Returns:
///     The result of each case, in case order.
///
/// Raises:
///     TestFailure: If any case did not pass. Its `results` attribute holds the
///         same results that would otherwise be returned.
#[pyfunction]
#[pyo3(signature = (url, cases=None, exclude_cases=None, concurrency=1))]
async fn run_fuzzingclient(
    url: String,
    cases: Option<Vec<String>>,
    exclude_cases: Option<Vec<String>>,
    concurrency: usize,
) -> PyResult<Vec<Py<CaseResult>>> {
    let constants = Python::attach(Constants::get);

    let cases = Python::attach(|py| resolve_cases(py, cases, exclude_cases, &constants))?;

    let url = Arc::new(client::parse_ws_url(&url)?);

    let permits = Arc::new(Semaphore::new(concurrency.max(1)));
    let results: Results = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::with_capacity(cases.len());
    let runtime = Python::attach(asyncrt::get_runtime)?;
    for (i, case) in cases.into_iter().enumerate() {
        let url = url.clone();
        let permits = permits.clone();
        let constants = constants.clone();
        let results = results.clone();
        handles.push(runtime.spawn(async move {
            let _permit = permits.acquire_owned().await.expect("semaphore");
            client::run_case(case, i + 1, url, constants, results).await
        }));
    }

    for handle in handles {
        handle
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))??;
    }
    let mut results = results.lock().unwrap();
    Python::attach(|py| finalize_results(py, &mut results))?;
    Ok(std::mem::take(&mut results))
}

/// Serve the Autobahn test cases to a WebSocket client under test.
///
/// Returns a server to use as an async context manager: entering it starts
/// accepting connections, `address` and `port` give the endpoint to point the
/// client at, and `get_result()` yields the outcome once the client has run
/// every case.
///
/// Example:
///     async with run_fuzzingserver(port=9001) as server:
///         results = await server.get_result()
///
/// Args:
///     cases: Case ids or `*` globs to serve, e.g. `["1.*", "9.7.1"]`. The whole
///         suite is served when omitted.
///     exclude_cases: Case ids or `*` globs to remove from the selection.
///     host: Address to bind. Defaults to `127.0.0.1`.
///     port: Port to bind. Pass `0` for an ephemeral port and read the chosen
///         one back from `port`.
///
/// Returns:
///     A `FuzzingServer` to use as an `async with` context manager.
#[pyfunction]
#[pyo3(signature = (cases=None, exclude_cases=None, *, host=None, port=9001))]
fn run_fuzzingserver(
    py: Python<'_>,
    cases: Option<Vec<String>>,
    exclude_cases: Option<Vec<String>>,
    host: Option<&str>,
    port: u16,
) -> PyResult<FuzzingServer> {
    let constants = Constants::get(py);
    let cases = resolve_cases(py, cases, exclude_cases, &constants)?;
    Ok(FuzzingServer::new(
        host.unwrap_or("127.0.0.1").to_string(),
        port,
        cases,
        constants,
    ))
}

fn resolve_cases(
    py: Python<'_>,
    case_ids: Option<Vec<String>>,
    exclude_ids: Option<Vec<String>>,
    constants: &Constants,
) -> PyResult<Vec<Case>> {
    let case_ids = case_ids.unwrap_or_default();
    let exclude_ids = exclude_ids.unwrap_or_default();

    let case_index = py
        .import(&constants.kosoku_cases)?
        .getattr(&constants.cases)?
        .cast_into::<PyDict>()?;

    let all_ids: Vec<String> = case_index.keys().extract()?;
    if all_ids.is_empty() {
        return Err(PyValueError::new_err(
            "kosoku.cases index is empty — this is a bug in kosoku",
        ));
    }

    // `cases` selects (the whole suite when empty); `exclude_cases` then removes.
    // Overlapping selections are de-duplicated after sorting.
    let mut ids = if case_ids.is_empty() {
        all_ids
    } else {
        expand_patterns(&case_ids, &all_ids, true)?
    };
    if !exclude_ids.is_empty() {
        let excluded: HashSet<String> = expand_patterns(&exclude_ids, &ids, false)?
            .into_iter()
            .collect();
        ids.retain(|id| !excluded.contains(id));
    }
    sort_ids(&mut ids);
    ids.dedup();

    let mut selected = Vec::with_capacity(ids.len());
    for id in ids {
        let class = case_index.as_any().get_item(&id)?;
        selected.push(Case::new(id, class.unbind()));
    }
    Ok(selected)
}

/// Expand Autobahn case patterns against the suite's `all_ids`: a plain id
/// matches exactly; an entry containing `*` is a glob (see [`compile_glob`]).
fn expand_patterns(
    patterns: &[String],
    all_ids: &[String],
    require_match: bool,
) -> PyResult<Vec<String>> {
    let mut out = Vec::new();
    for pat in patterns {
        if pat.contains('*') {
            let glob = compile_glob(pat)?;
            let before = out.len();
            out.extend(all_ids.iter().filter(|id| glob.is_match(id)).cloned());
            if require_match && out.len() == before {
                return Err(PyValueError::new_err(format!(
                    "no cases match pattern: {pat}"
                )));
            }
        } else if all_ids.iter().any(|id| id == pat) {
            out.push(pat.clone());
        } else if require_match {
            return Err(PyValueError::new_err(format!("no such case: {pat}")));
        }
    }
    Ok(out)
}

/// Translate a `*` case glob to a regex the way Autobahn does — escape the `.`
/// separators and turn `*` into `.*` anchored at the start.
fn compile_glob(pattern: &str) -> PyResult<Regex> {
    let translated = format!("^{}", pattern.replace('.', r"\.").replace('*', ".*"));
    Regex::new(&translated)
        .map_err(|e| PyValueError::new_err(format!("invalid case pattern {pattern:?}: {e}")))
}

/// Sort case ids numerically by component (so 1.2.10 follows 1.2.9, and 6.10.x
/// follows 6.9.x — not lexicographically).
fn sort_ids(ids: &mut [String]) {
    ids.sort_by_key(|id| {
        id.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
}

#[pymodule]
mod _kosoku {
    #[pymodule_export]
    use super::{FuzzingServer, run_fuzzingclient, run_fuzzingserver};
    #[pymodule_export]
    use crate::compress::{
        PerMessageDeflateOffer, PerMessageDeflateOfferAccept, PerMessageDeflateResponse,
        PerMessageDeflateResponseAccept,
    };
    #[pymodule_export]
    use crate::result::{Behavior, BehaviorClose, CaseResult};
    #[pymodule_export]
    use crate::traffic::TrafficStats;
    #[pymodule_export]
    use crate::utf8::Utf8Validator;
}
