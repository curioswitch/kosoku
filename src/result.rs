//! The per-case result type returned by the run entrypoints.
//!
//! The verdict/result fields mirror [`Case`](https://github.com/crossbario/autobahn-testsuite/blob/v25.10.1/autobahntestsuite/autobahntestsuite/case/case.py#L24);
//! the camelCase report schema comes from its `FuzzingFactory.createReports`.

use std::sync::{Arc, Mutex};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyInt, PyList, PyString};

use crate::asyncrt::attach_blocking;
use crate::traffic::TrafficStats;

// The Python exception to indicate test failures.
pyo3::import_exception!(kosoku, FailureError);

/// The shared sink the case drivers push results into as cases finish.
pub(crate) type Results = Arc<Mutex<Vec<Py<CaseResult>>>>;

/// Verdict for a case's message exchange. `value` is the Autobahn label.
#[pyclass(eq, frozen, rename_all = "SCREAMING_SNAKE_CASE", skip_from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Behavior {
    /// The peer behaved exactly as the case requires.
    Ok,
    /// Acceptable, but diverges from a SHOULD-level requirement (still conformant).
    NonStrict,
    /// Non-conforming: the peer violated a MUST-level requirement.
    Failed,
    /// The peer does not implement the feature the case exercises.
    Unimplemented,
    /// Informational only: the case probes behavior the spec leaves unspecified.
    Informational,
    /// The case could not be run to a verdict (a connection failure or a crash).
    Error,
}

#[pymethods]
impl Behavior {
    /// The verdict's string form, e.g. `"NON-STRICT"`.
    #[getter]
    #[allow(clippy::trivially_copy_pass_by_ref)] // pyo3 getters take `&self`
    fn value(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NonStrict => "NON-STRICT",
            Self::Failed => "FAILED",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Informational => "INFORMATIONAL",
            Self::Error => "ERROR",
        }
    }
}

impl Behavior {
    pub(crate) fn from_label(label: &str) -> PyResult<Self> {
        Ok(match label {
            "OK" => Self::Ok,
            "NON-STRICT" => Self::NonStrict,
            "FAILED" => Self::Failed,
            "UNIMPLEMENTED" => Self::Unimplemented,
            "INFORMATIONAL" => Self::Informational,
            "ERROR" => Self::Error,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unexpected behavior {other:?}"
                )));
            }
        })
    }

    fn is_pass(self) -> bool {
        matches!(self, Self::Ok | Self::NonStrict | Self::Informational)
    }
}

/// Verdict for a case's closing handshake. `value` is the Autobahn label.
#[pyclass(eq, frozen, rename_all = "SCREAMING_SNAKE_CASE", skip_from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BehaviorClose {
    /// The closing handshake completed as expected.
    Ok,
    /// The connection was failed by the wrong endpoint.
    Failed,
    /// The peer sent an unexpected close code.
    WrongCode,
    /// The connection was not closed cleanly where the spec requires it.
    Unclean,
    /// The client closed the TCP connection where the server should have.
    FailedByClient,
    /// Informational only: the closing behavior is left unspecified.
    Informational,
}

#[pymethods]
impl BehaviorClose {
    /// The verdict's string form, e.g. `"WRONG CODE"`.
    #[getter]
    #[allow(clippy::trivially_copy_pass_by_ref)] // pyo3 getters take `&self`
    fn value(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Failed => "FAILED",
            Self::WrongCode => "WRONG CODE",
            Self::Unclean => "UNCLEAN",
            Self::FailedByClient => "FAILED BY CLIENT",
            Self::Informational => "INFORMATIONAL",
        }
    }
}

impl BehaviorClose {
    pub(crate) fn from_label(label: &str) -> PyResult<Self> {
        Ok(match label {
            "OK" => Self::Ok,
            "FAILED" => Self::Failed,
            "WRONG CODE" => Self::WrongCode,
            "UNCLEAN" => Self::Unclean,
            "FAILED BY CLIENT" => Self::FailedByClient,
            "INFORMATIONAL" => Self::Informational,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unexpected behaviorClose {other:?}"
                )));
            }
        })
    }

    fn is_pass(self) -> bool {
        matches!(self, Self::Ok | Self::Informational)
    }
}

/// The result of one Autobahn test case.
#[pyclass(frozen, get_all)]
pub(crate) struct CaseResult {
    /// Case id, e.g. `"1.1.1"`.
    pub(crate) case_id: Py<PyString>,
    /// 1-based position of this case in the run.
    pub(crate) case_index: Py<PyInt>,
    /// The case's `DESCRIPTION` text.
    pub(crate) description: Py<PyString>,
    /// The case's `EXPECTATION` text — what a conformant peer should do.
    pub(crate) expectation: Py<PyString>,
    /// Name of the peer under test — from its `Server` header (client mode) or
    /// the `agent` it reported (server mode).
    pub(crate) agent: Py<PyString>,
    /// ISO-8601 UTC timestamp of when the case started.
    pub(crate) started: Py<PyString>,
    /// Case run time, in milliseconds.
    pub(crate) duration: Py<PyInt>,
    /// Whether the report shows `duration` (set by the timing cases, 9.x/12.x).
    pub(crate) report_time: Py<PyBool>,
    /// Whether the report shows the compression ratio (set by the 12.x cases).
    pub(crate) report_compression_ratio: Py<PyBool>,
    /// Verdict for the message exchange.
    pub(crate) behavior: Behavior,
    /// Verdict for the closing handshake.
    pub(crate) behavior_close: BehaviorClose,
    /// Accepted event sequences, keyed by the verdict each would yield.
    pub(crate) expected: Py<PyAny>,
    /// Expected close parameters: who closes, whether a clean close is required,
    /// and the acceptable close codes.
    pub(crate) expected_close: Py<PyAny>,
    /// The events actually observed: messages, pings, and pongs.
    pub(crate) received: Py<PyAny>,
    /// Prose explaining `behavior`, e.g. "Actual events match at least one
    /// expected." or "Actual events differ from any expected."
    pub(crate) result: Py<PyString>,
    /// Prose explaining `behavior_close`, e.g. "Connection was properly closed"
    /// or "The close code should have been ...".
    pub(crate) result_close: Py<PyString>,
    /// Ordered wire-log trace of octets, frames, and timer events.
    pub(crate) wirelog: Py<PyList>,
    /// Whether the wire log was captured (disabled for very large payloads).
    pub(crate) create_wirelog: Py<PyBool>,
    /// Whether kosoku sent the close frame first.
    pub(crate) closed_by_me: Py<PyBool>,
    /// Whether kosoku failed the connection on a protocol error (by sending a
    /// close or dropping the TCP).
    pub(crate) failed_by_me: Py<PyBool>,
    /// Whether kosoku dropped the TCP connection.
    pub(crate) dropped_by_me: Py<PyBool>,
    /// Whether the closing handshake completed cleanly — close sent and
    /// received, then the responsible side dropped the connection.
    pub(crate) was_clean: Py<PyBool>,
    /// Why the close was not clean, when `was_clean` is false.
    pub(crate) was_not_clean_reason: Option<Py<PyString>>,
    /// Whether kosoku expected the server to drop the connection but it did not
    /// in time (client mode).
    pub(crate) was_server_connection_drop_timeout: Py<PyBool>,
    /// Whether the opening handshake timed out.
    pub(crate) was_open_handshake_timeout: Py<PyBool>,
    /// Whether the closing handshake timed out.
    pub(crate) was_close_handshake_timeout: Py<PyBool>,
    /// The close code kosoku sent, if any.
    pub(crate) local_close_code: Option<Py<PyInt>>,
    /// The close reason kosoku sent, if any.
    pub(crate) local_close_reason: Option<Py<PyString>>,
    /// The close code the peer sent, if any.
    pub(crate) remote_close_code: Option<Py<PyInt>>,
    /// The close reason the peer sent, if any.
    pub(crate) remote_close_reason: Option<Py<PyString>>,
    /// Whether kosoku acted as the server (the peer under test is the client).
    pub(crate) is_server: Py<PyBool>,
    /// Whether octet and frame statistics were collected.
    pub(crate) create_stats: Py<PyBool>,
    /// Count of socket reads by size.
    pub(crate) rx_octet_stats: Py<PyDict>,
    /// Count of received frames by opcode.
    pub(crate) rx_frame_stats: Py<PyDict>,
    /// Count of socket writes by size.
    pub(crate) tx_octet_stats: Py<PyDict>,
    /// Count of sent frames by opcode.
    pub(crate) tx_frame_stats: Py<PyDict>,
    /// The opening-handshake request, verbatim.
    pub(crate) http_request: Py<PyString>,
    /// The opening-handshake response, verbatim.
    pub(crate) http_response: Py<PyString>,
    /// Traffic and compression counters for the case.
    pub(crate) traffic_stats: Option<Py<TrafficStats>>,
}

impl CaseResult {
    /// A result for a case that could not be run to a verdict — a connection
    /// failure or a crash in the case code. Counts as a failure (`behavior` is
    /// the synthetic `"ERROR"`); `message` is the error.
    pub(crate) fn errored(
        py: Python<'_>,
        case_id: &str,
        case_index: usize,
        agent: &str,
        err: &PyErr,
    ) -> PyResult<Self> {
        let py_empty_str = PyString::new(py, "").unbind();
        let py_false = PyBool::new(py, false).to_owned().unbind();
        Ok(CaseResult {
            case_id: PyString::new(py, case_id).unbind(),
            case_index: case_index.into_pyobject(py).unwrap().unbind(),
            description: py_empty_str.clone_ref(py),
            expectation: py_empty_str.clone_ref(py),
            agent: PyString::new(py, agent).unbind(),
            started: py_empty_str.clone_ref(py),
            duration: PyInt::new(py, 0).unbind(),
            report_time: py_false.clone_ref(py),
            report_compression_ratio: py_false.clone_ref(py),
            behavior: Behavior::Error,
            behavior_close: BehaviorClose::Failed,
            expected: py.None(),
            expected_close: py.None(),
            received: py.None(),
            result: err.value(py).str()?.unbind(),
            result_close: py_empty_str.clone_ref(py),
            wirelog: PyList::empty(py).unbind(),
            create_wirelog: py_false.clone_ref(py),
            closed_by_me: py_false.clone_ref(py),
            failed_by_me: py_false.clone_ref(py),
            dropped_by_me: py_false.clone_ref(py),
            was_clean: py_false.clone_ref(py),
            was_not_clean_reason: None,
            was_server_connection_drop_timeout: py_false.clone_ref(py),
            was_open_handshake_timeout: py_false.clone_ref(py),
            was_close_handshake_timeout: py_false.clone_ref(py),
            local_close_code: None,
            local_close_reason: None,
            remote_close_code: None,
            remote_close_reason: None,
            is_server: py_false.clone_ref(py),
            create_stats: PyBool::new(py, true).to_owned().unbind(),
            rx_octet_stats: PyDict::new(py).unbind(),
            rx_frame_stats: PyDict::new(py).unbind(),
            tx_octet_stats: PyDict::new(py).unbind(),
            tx_frame_stats: PyDict::new(py).unbind(),
            http_request: py_empty_str.clone_ref(py),
            http_response: py_empty_str.clone_ref(py),
            traffic_stats: None,
        })
    }
}

#[pymethods]
impl CaseResult {
    /// Whether the case passed — both `behavior` and `behavior_close` are
    /// acceptable (`OK`, `NON-STRICT`, or `INFORMATIONAL`).
    #[getter]
    fn passed(&self) -> bool {
        self.behavior.is_pass() && self.behavior_close.is_pass()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "CaseResult(case_id={:?}, behavior={:?}, behavior_close={:?}, passed={})",
            self.case_id.to_str(py)?,
            self.behavior.value(),
            self.behavior_close.value(),
            self.passed()
        ))
    }
}

/// Sort the results into case order, then return them — raising `FailureError`
/// if any case failed. Results arrive in completion order, so we order them by
/// each result's own `case_index` rather than tracking position alongside.
pub(crate) fn finalize_results(py: Python<'_>, results: &mut Vec<Py<CaseResult>>) -> PyResult<()> {
    results.sort_by_key(|cr| {
        cr.get()
            .case_index
            .bind(py)
            .extract::<usize>()
            .expect("case_index is a usize")
    });

    let mut failures = Vec::new();
    for cr in &*results {
        let cr = cr.get();
        if !cr.passed() {
            failures.push(format!(
                "  {}: behavior={}, behaviorClose={} — {}",
                cr.case_id.to_str(py)?,
                cr.behavior.value(),
                cr.behavior_close.value(),
                cr.result.to_str(py)?
            ));
        }
    }
    if !failures.is_empty() {
        let message = format!(
            "{} of {} cases failed:\n{}",
            failures.len(),
            results.len(),
            failures.join("\n")
        );
        let results_py = PyList::new(py, results.iter().map(|cr| cr.clone_ref(py)))?;
        return Err(FailureError::new_err((message, results_py.unbind())));
    }
    Ok(())
}

/// Record an `ERROR` result — a case that couldn't be run to a verdict, e.g. a
/// connection failure or a crash in the case code. The mirror of [`drive`]'s
/// own push for the cases that never reached a verdict.
///
/// [`drive`]: crate::runner::drive
pub(crate) async fn record_error(
    results: &Results,
    case_id: &str,
    case_index: usize,
    agent: &str,
    err: PyErr,
) -> PyResult<()> {
    let case_id = case_id.to_string();
    let agent = agent.to_string();
    let result = attach_blocking(move |py| {
        Py::new(
            py,
            CaseResult::errored(py, &case_id, case_index, &agent, &err)?,
        )
    })
    .await?;
    results.lock().expect("results mutex").push(result);
    Ok(())
}
