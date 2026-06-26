//! `fuzzingserver` mode: serve Autobahn test cases to a test client that
//! connects to it.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, Mutex};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, header};
use pyo3::exceptions::{PyIOError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};
use url::Url;

use crate::asyncrt::attach_blocking;
use crate::compress::{PerMessageDeflateOffer, PerMessageDeflateOfferAccept};
use crate::constants::Constants;
use crate::protocol::{Protocol, ProtocolInner};
use crate::result::{Results, finalize_results, record_error};
use crate::runner::{drive, invalid_http, read_head, to_header_map};
use crate::{Case, asyncrt};

/// Accepts requests with a case parameter and runs the appropriate test case.
///
/// The client drives the sequence:
/// - `/getCaseCount` → we reply with the count
/// - `/runCase?case=N` → we run case N
/// - `/updateReports` → we stop.
pub(crate) async fn serve(
    listener: TcpListener,
    cases: Vec<Case>,
    constants: Constants,
    shutdown: Arc<Notify>,
    results: Results,
) -> PyResult<()> {
    let count = cases.len();
    let cases = Arc::new(cases);
    let mut conns = JoinSet::new();

    loop {
        let accepted = tokio::select! {
            biased;
            () = shutdown.notified() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, _peer) = accepted.map_err(|e| PyIOError::new_err(format!("accept: {e}")))?;
        stream.set_nodelay(true).ok();
        conns.spawn(handle_connection(
            stream,
            cases.clone(),
            count,
            constants.clone(),
            shutdown.clone(),
            results.clone(),
        ));
    }

    // Drain in-flight connections; each has pushed its own result (if any).
    while let Some(joined) = conns.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err),
            Err(join_err) => {
                return Err(PyRuntimeError::new_err(format!(
                    "connection task panicked: {join_err}"
                )));
            }
        }
    }

    Python::attach(|py| finalize_results(py, &mut results.lock().unwrap()))?;

    Ok(())
}

async fn handle_connection(
    mut stream: TcpStream,
    cases: Arc<Vec<Case>>,
    count: usize,
    constants: Constants,
    shutdown: Arc<Notify>,
    results: Results,
) -> PyResult<()> {
    let Ok((req, mut rbuf)) = read_head(&mut stream, parse_request_head).await else {
        // not a valid upgrade; drop the connection
        return Ok(());
    };
    let Some(key) = req.headers.get(header::SEC_WEBSOCKET_KEY) else {
        return Ok(());
    };

    if req.target.path() == "/getCaseCount" {
        if send_upgrade(&mut stream, key, None).await.is_ok() {
            send_text_then_close(&mut stream, itoa::Buffer::new().format(count).as_bytes())
                .await
                .ok();
        }
        return Ok(());
    }
    if req.target.path() == "/updateReports" {
        send_upgrade(&mut stream, key, None).await.ok();
        // Testee signals it is done; stop accepting new connections.
        shutdown.notify_one();
        return Ok(());
    }

    let Some(n) = req
        .target
        .query_pairs()
        .find(|(k, _)| k == "case")
        .and_then(|(_, v)| v.parse::<usize>().ok())
    else {
        return Ok(());
    };
    if n < 1 || n > count {
        return Ok(());
    }

    let case = cases[n - 1].clone();
    // Create the protocol + case instance (its init() may set the server deflate
    // accept policy) and negotiate deflate before completing the handshake.
    let (protocol, instance, resp_ext) = {
        let case = case.clone();
        let offer = req.headers.get(header::SEC_WEBSOCKET_EXTENSIONS).cloned();
        attach_blocking(move |py| {
            let protocol = Protocol::new(py, true)?;
            let instance = case.class.bind(py).call1((protocol.clone(),))?.unbind();
            let resp_ext = negotiate_deflate(py, &protocol, offer.as_ref())?;
            Ok((protocol, instance, resp_ext))
        })
        .await?
    };

    let Ok(response) = send_upgrade(&mut stream, key, resp_ext.as_deref()).await else {
        return Ok(());
    };
    {
        let mut c = protocol.lock();
        c.http_request = req.raw;
        c.http_response = response.trim_end().to_string();
    }

    // Prioritize agent query string, then user-agent, and a fallback as Autobahn does.
    let agent = req
        .target
        .query_pairs()
        .find(|(k, _)| k == "agent")
        .map(|(_, v)| v.into_owned())
        .or_else(|| {
            req.headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "UnknownClient".to_string());

    // On success `drive` pushes the verdict; a crash mid-case never reached one,
    // so record it as an ERROR result.
    let instance = Arc::new(instance);
    if let Err(err) = drive(
        &mut stream,
        &mut rbuf,
        &protocol,
        instance,
        &case.id,
        n,
        &agent,
        &constants,
        &results,
    )
    .await
    {
        record_error(&results, &case.id, n, &agent, err).await?;
    }
    Ok(())
}

/// Base for resolving the origin-form request target (`/path?query`) into a
/// `Url` — only its path and query are read, so the host is a placeholder.
static REQUEST_BASE: LazyLock<Url> =
    LazyLock::new(|| Url::parse("ws://kosoku").expect("valid base url"));

/// The head of an HTTP request.
struct RequestHead {
    /// The request target, e.g. `/runCase?case=1&agent=ws`.
    target: Url,
    /// The HTTP headers.
    headers: HeaderMap,
    /// The head verbatim.
    raw: String,
}

fn parse_request_head(buf: &[u8]) -> std::io::Result<Option<(RequestHead, usize)>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let len = match req.parse(buf).map_err(invalid_http)? {
        httparse::Status::Partial => return Ok(None),
        httparse::Status::Complete(len) => len,
    };
    let head = RequestHead {
        target: REQUEST_BASE
            .join(req.path.unwrap_or("/"))
            .map_err(invalid_http)?,
        headers: to_header_map(req.headers)?,
        raw: String::from_utf8_lossy(&buf[..len]).trim_end().to_owned(),
    };
    Ok(Some((head, len)))
}

/// Send the 101 upgrade response, echoing `extensions` (the negotiated
/// permessage-deflate response) if any.
async fn send_upgrade(
    stream: &mut TcpStream,
    key: &HeaderValue,
    extensions: Option<&str>,
) -> std::io::Result<String> {
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let ext_line = match extensions {
        Some(e) => format!("Sec-WebSocket-Extensions: {e}\r\n"),
        None => String::new(),
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{ext_line}\r\n"
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    Ok(resp)
}

/// Server-side permessage-deflate negotiation: parse the client's offers and run
/// the case's `perMessageCompressionAccept` callback to determine compression.
fn negotiate_deflate(
    py: Python<'_>,
    protocol: &Protocol,
    offer_header: Option<&HeaderValue>,
) -> PyResult<Option<String>> {
    let Some(header) = offer_header.and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    // The case set its accept policy on `self.p.perMessageCompressionAccept`.
    let Some(accept_cb) = protocol.accept(py) else {
        return Ok(None);
    };
    if accept_cb.is_none(py) {
        return Ok(None);
    }

    let offers = PyList::new(
        py,
        crate::deflate::parse_offers(header)
            .into_iter()
            .map(|o| PerMessageDeflateOffer {
                accept_no_context_takeover: o.accept_no_context_takeover,
                accept_max_window_bits: o.accept_max_window_bits,
                request_no_context_takeover: o.request_no_context_takeover,
                request_max_window_bits: o.request_max_window_bits,
            }),
    )?;
    // The callback returns a PerMessageDeflateOfferAccept to accept, or None to
    // decline.
    let result = accept_cb.bind(py).call1((offers,))?;
    if result.is_none() {
        return Ok(None);
    }
    let accept = result.cast_into::<PerMessageDeflateOfferAccept>()?;

    let accept = accept.get();
    let offer = accept.offer.get();
    let server_nct = accept
        .no_context_takeover
        .unwrap_or(offer.request_no_context_takeover);
    let server_wb = accept.window_bits.unwrap_or(offer.request_max_window_bits);
    let client_nct = accept.request_no_context_takeover;
    let client_wb = accept.request_max_window_bits;

    let mut ext = String::from("permessage-deflate");
    if server_nct {
        ext += "; server_no_context_takeover";
    }
    if server_wb != 0 {
        let _ = write!(ext, "; server_max_window_bits={server_wb}");
    }
    if client_nct {
        ext += "; client_no_context_takeover";
    }
    if client_wb != 0 {
        let _ = write!(ext, "; client_max_window_bits={client_wb}");
    }
    protocol.lock_py(py).compress = Some(crate::deflate::Deflate::new(
        server_nct, server_wb, client_nct, client_wb,
    ));
    Ok(Some(ext))
}

/// Send a single text frame followed by a normal close.
async fn send_text_then_close(stream: &mut TcpStream, text: &[u8]) -> std::io::Result<()> {
    let text = Bytes::copy_from_slice(text);
    let chunks = attach_blocking(move |py| -> PyResult<Vec<Bytes>> {
        let mut inner = ProtocolInner::new(py, true)?;
        inner.send_frame(1, true, 0, text, None, None, false).ok();
        inner.send_close(Some(1000), None).ok();
        Ok(std::mem::take(&mut inner.out_queue))
    })
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    for chunk in chunks {
        stream.write_all(&chunk).await?;
    }
    stream.flush().await
}

struct ServerInner {
    /// The host to bind, e.g. `127.0.0.1`.
    host: String,
    /// The requested port; `0` asks the OS for an ephemeral one.
    requested_port: u16,
    /// The cases to serve, in run order.
    cases: Vec<Case>,
    constants: Constants,
    /// Tripped to stop the accept loop (on `/updateReports` or `__aexit__`).
    shutdown: Arc<Notify>,
    /// The bound local address, set once serving starts (`__aenter__`).
    local_addr: Mutex<Option<SocketAddr>>,
    /// Per-case results, pushed by the serve task as cases finish; `get_result`
    /// re-reads them once the run is over.
    results: Results,
    /// The serving task, set in `__aenter__`; `get_result` awaits it (once) for
    /// the run to finish — it carries no value, the results live in `results`.
    serving: Mutex<Option<JoinHandle<PyResult<()>>>>,
}

/// A server that runs the Autobahn cases for a WebSocket client under test.
///
/// Created by `run_fuzzingserver` and used as an async context manager:
/// entering starts serving, `address`/`port` give the bound endpoint, and
/// `get_result()` returns the per-case results once the client has finished.
#[pyclass(frozen)]
pub(crate) struct FuzzingServer {
    inner: Arc<ServerInner>,
}

#[pymethods]
impl FuzzingServer {
    /// The bound IP address. Available once serving has started.
    #[getter]
    fn address(&self) -> PyResult<String> {
        match *self.inner.local_addr.lock().expect("address mutex") {
            Some(addr) => Ok(addr.ip().to_string()),
            None => Err(PyRuntimeError::new_err(
                "server not started yet (use `async with`)",
            )),
        }
    }

    /// The bound TCP port — the real port chosen by the OS when `0` was
    /// requested. Available once serving has started.
    #[getter]
    fn port(&self) -> PyResult<u16> {
        match *self.inner.local_addr.lock().expect("address mutex") {
            Some(addr) => Ok(addr.port()),
            None => Err(PyRuntimeError::new_err(
                "server not started yet (use `async with`)",
            )),
        }
    }

    /// Start serving and return the server, now bound to `address` and `port`.
    #[allow(clippy::unused_async)] // must be awaitable for `async with`
    async fn __aenter__(slf: Py<Self>) -> PyResult<Py<Self>> {
        let inner = slf.get().inner.clone();
        if inner.serving.lock().expect("serving mutex").is_some() {
            return Err(PyRuntimeError::new_err("fuzzingserver already started"));
        }
        let runtime = Python::attach(asyncrt::get_runtime)?;
        // Bind (resolving an ephemeral `port=0` to a real port) within the runtime
        // context so `from_std` can register the socket with the reactor.
        let listener = {
            let _guard = runtime.enter();
            let std_listener = std::net::TcpListener::bind((
                inner.host.as_str(),
                inner.requested_port,
            ))
            .map_err(|e| {
                PyIOError::new_err(format!("bind {}:{}: {e}", inner.host, inner.requested_port))
            })?;
            std_listener
                .set_nonblocking(true)
                .map_err(|e| PyIOError::new_err(format!("set listener non-blocking: {e}")))?;
            TcpListener::from_std(std_listener)
                .map_err(|e| PyIOError::new_err(format!("register listener: {e}")))?
        };
        let local = listener
            .local_addr()
            .map_err(|e| PyIOError::new_err(format!("listener address: {e}")))?;
        *inner.local_addr.lock().expect("address mutex") = Some(local);

        let cases = inner.cases.clone();
        let constants = inner.constants.clone();
        let shutdown = inner.shutdown.clone();
        let results = inner.results.clone();
        let handle = runtime
            .spawn(async move { serve(listener, cases, constants, shutdown, results).await });
        *inner.serving.lock().expect("serving mutex") = Some(handle);
        Ok(slf)
    }

    /// Wait for the run to finish and return the per-case results.
    ///
    /// The client under test ends the run once it has driven every case.
    ///
    /// Returns:
    ///     The result of each case, in case order.
    ///
    /// Raises:
    ///     FailureError: If any case did not pass. Its `results` attribute holds
    ///         the same results that would otherwise be returned.
    #[pyo3(signature = () -> "list[CaseResult]")]
    async fn get_result(slf: Py<Self>) -> PyResult<Py<PyList>> {
        let inner = slf.get().inner.clone();
        if inner.local_addr.lock().expect("address mutex").is_none() {
            return Err(PyRuntimeError::new_err(
                "get_result() needs an active server (use `async with`)",
            ));
        }
        // The first call awaits the run; later calls find the task already taken
        // and just re-read the (now final) results.
        let handle = inner.serving.lock().expect("serving mutex").take();
        if let Some(handle) = handle {
            handle
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))??;
        }
        Python::attach(|py| {
            Ok(PyList::new(
                py,
                inner
                    .results
                    .lock()
                    .expect("results mutex")
                    .iter()
                    .map(|cr| cr.clone_ref(py)),
            )?
            .unbind())
        })
    }

    /// Stop the server, closing it if the run has not already finished.
    #[pyo3(signature = (_exc_type=None, _exc_value=None, _traceback=None))]
    #[allow(clippy::unused_async)] // must be awaitable for `async with`
    async fn __aexit__(
        slf: Py<Self>,
        _exc_type: Option<Py<PyAny>>,
        _exc_value: Option<Py<PyAny>>,
        _traceback: Option<Py<PyAny>>,
    ) -> bool {
        let inner = slf.get().inner.clone();
        inner.shutdown.notify_one();
        if let Some(handle) = inner.serving.lock().expect("serving mutex").take() {
            handle.abort();
        }
        false
    }
}

impl FuzzingServer {
    pub(crate) fn new(host: String, port: u16, cases: Vec<Case>, constants: Constants) -> Self {
        Self {
            inner: Arc::new(ServerInner {
                host,
                requested_port: port,
                cases,
                constants,
                shutdown: Arc::new(Notify::new()),
                local_addr: Mutex::new(None),
                results: Arc::new(Mutex::new(Vec::new())),
                serving: Mutex::new(None),
            }),
        }
    }
}
