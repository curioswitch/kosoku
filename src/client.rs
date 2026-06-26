//! `fuzzingclient` mode: drive the Autobahn cases against a server under test.

use std::fmt::Write as _;
use std::sync::Arc;

use bytes::BytesMut;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use url::{Position, Url};

use crate::Case;
use crate::asyncrt::attach_blocking;
use crate::compress::{
    PerMessageDeflateOffer, PerMessageDeflateResponse, PerMessageDeflateResponseAccept,
};
use crate::constants::Constants;
use crate::protocol::Protocol;
use crate::result::{Results, record_error};
use crate::runner::{drive, invalid_http, read_head, to_header_map};

/// Connect and run `case` against `url`, recording its result into `results`. A
/// connection/handshake failure or a crash in the case code is recorded as an
/// ERROR result instead of aborting.
pub(crate) async fn run_case(
    case: Case,
    case_index: usize,
    url: Arc<Url>,
    constants: Constants,
    results: Results,
) -> PyResult<()> {
    // On success `drive` has already pushed the verdict; only failures (which
    // never reached one) are recorded here.
    if let Err(err) = run_case_inner(&case, case_index, &url, &constants, &results).await {
        record_error(&results, &case.id, case_index, url.authority(), err).await?;
    }
    Ok(())
}

async fn run_case_inner(
    case: &Case,
    case_index: usize,
    url: &Url,
    constants: &Constants,
    results: &Results,
) -> PyResult<()> {
    let host = url.host_str().unwrap_or_default();
    let port = url.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| PyIOError::new_err(format!("connect: {e}")))?;
    let _ = stream.set_nodelay(true); // best effort

    let (
        protocol,
        CaseInstance {
            instance,
            extension_request: ext_req,
        },
    ) = initialize_case(case).await?;
    let instance = Arc::new(instance);

    let target = &url[Position::BeforePath..Position::AfterQuery];
    let (hs, mut rbuf) = handshake(&mut stream, host, port, target, ext_req.as_deref())
        .await
        .map_err(|e| PyIOError::new_err(format!("handshake: {e}")))?;
    {
        let mut c = protocol.lock();
        c.http_request = hs.request;
        c.http_response = hs.response;
    }

    let neg_protocol = protocol.clone();
    let extensions = hs.extensions;
    attach_blocking(move |py| negotiate_deflate(py, &neg_protocol, extensions.as_ref())).await?;

    let agent = hs
        .server
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| url.authority());

    drive(
        &mut stream,
        &mut rbuf,
        &protocol,
        instance,
        &case.id,
        case_index,
        agent,
        constants,
        results,
    )
    .await
}

/// The Python case instance built before the handshake, plus the extension
/// request its `init()` implied.
struct CaseInstance {
    /// The instantiated Python case object.
    instance: Py<PyAny>,
    /// The `Sec-WebSocket-Extensions` value to offer, built from the case's
    /// deflate offers, if any.
    extension_request: Option<String>,
}

/// Initializes the case, passing in the Protocol that interfaces it with the runner.
async fn initialize_case(case: &Case) -> PyResult<(Protocol, CaseInstance)> {
    let case = case.clone();
    attach_blocking(move |py| {
        let protocol = Protocol::new(py, false)?;
        let instance = case.class.bind(py).call1((protocol.clone(),))?.unbind();
        // The case's init() may have assigned `self.p.perMessageCompressionOffers`.
        let offers = protocol.offers(py);
        let extension_request = match offers {
            Some(o) => build_offer_header(o.bind(py))?,
            _ => None,
        };
        Ok((
            protocol,
            CaseInstance {
                instance,
                extension_request,
            },
        ))
    })
    .await
}

/// The opening handshake's outcome.
struct Handshake {
    request: String,
    response: String,
    extensions: Option<HeaderValue>,
    server: Option<HeaderValue>,
}

/// The head of an HTTP response — bodies are WebSocket frames, handled elsewhere.
struct ResponseHead {
    status: StatusCode,
    headers: HeaderMap,
    /// The head verbatim (reported as `httpResponse`).
    raw: String,
}

fn parse_response_head(buf: &[u8]) -> std::io::Result<Option<(ResponseHead, usize)>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut resp = httparse::Response::new(&mut headers);
    let len = match resp.parse(buf).map_err(invalid_http)? {
        httparse::Status::Partial => return Ok(None),
        httparse::Status::Complete(len) => len,
    };
    let status = resp
        .code
        .and_then(|c| StatusCode::from_u16(c).ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid HTTP status",
            )
        })?;
    let head = ResponseHead {
        status,
        headers: to_header_map(resp.headers)?,
        raw: String::from_utf8_lossy(&buf[..len]).trim_end().to_owned(),
    };
    Ok(Some((head, len)))
}

/// Client opening handshake. Returns the handshake outcome and the read buffer
/// holding any bytes the server already sent past the response head.
async fn handshake(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    path: &str,
    extensions: Option<&str>,
) -> std::io::Result<(Handshake, BytesMut)> {
    let ext_line = match extensions {
        Some(e) => format!("Sec-WebSocket-Extensions: {e}\r\n"),
        None => String::new(),
    };
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n{ext_line}\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;

    let (resp, rbuf) = read_head(stream, parse_response_head).await?;
    if resp.status != StatusCode::SWITCHING_PROTOCOLS {
        return Err(std::io::Error::other(format!(
            "expected HTTP 101, got {}",
            resp.status
        )));
    }
    let mut headers = resp.headers;
    Ok((
        Handshake {
            request: req.trim_end().to_string(),
            extensions: headers.remove(header::SEC_WEBSOCKET_EXTENSIONS),
            server: headers.remove(header::SERVER),
            response: resp.raw,
        },
        rbuf,
    ))
}

/// Build the `Sec-WebSocket-Extensions` request value from the case's offers
/// or `None` if empty.
fn build_offer_header(offers: &Bound<'_, PyList>) -> PyResult<Option<String>> {
    let mut parts = Vec::new();
    for offer in offers {
        let offer = offer.cast_into::<PerMessageDeflateOffer>()?;
        let offer = offer.get();
        let mut token = String::from("permessage-deflate");
        if offer.accept_no_context_takeover {
            token += "; client_no_context_takeover";
        }
        if offer.accept_max_window_bits {
            token += "; client_max_window_bits";
        }
        if offer.request_no_context_takeover {
            token += "; server_no_context_takeover";
        }
        if offer.request_max_window_bits != 0 {
            let _ = write!(
                token,
                "; server_max_window_bits={}",
                offer.request_max_window_bits
            );
        }
        parts.push(token);
    }
    Ok((!parts.is_empty()).then(|| parts.join(", ")))
}

/// Given the server's `Sec-WebSocket-Extensions` response, run the case's accept
/// callback.
fn negotiate_deflate(
    py: Python<'_>,
    protocol: &Protocol,
    response_header: Option<&HeaderValue>,
) -> PyResult<()> {
    let Some(p) = response_header
        .and_then(|v| v.to_str().ok())
        .and_then(crate::deflate::parse_response)
    else {
        return Ok(());
    };
    // The case set its accept callback on `self.p.perMessageCompressionAccept`.
    let Some(accept_cb) = protocol.accept(py) else {
        return Ok(());
    };
    if accept_cb.is_none(py) {
        return Ok(());
    }

    let response = PerMessageDeflateResponse {
        client_max_window_bits: p.client_max_window_bits,
        client_no_context_takeover: p.client_no_context_takeover,
        server_max_window_bits: p.server_max_window_bits,
        server_no_context_takeover: p.server_no_context_takeover,
    };
    let result = accept_cb.bind(py).call1((response,))?;
    if result.is_none() {
        return Ok(());
    }
    let accept = result.cast_into::<PerMessageDeflateResponseAccept>()?;
    let accept = accept.get();
    // The accept may narrow the client compressor below the server's terms;
    // decompression follows the server's.
    protocol.lock_py(py).compress = Some(crate::deflate::Deflate::new(
        accept
            .no_context_takeover
            .unwrap_or(p.client_no_context_takeover),
        accept.window_bits.unwrap_or(p.client_max_window_bits),
        p.server_no_context_takeover,
        p.server_max_window_bits,
    ));
    Ok(())
}

/// Parse the target URL. The scheme is optional; when present it must be `ws`
/// or `http` (we connect over plain TCP either way), and any other scheme is an
/// error. A bare `host[:port][/path]` is accepted as if `ws://` were given.
pub(crate) fn parse_ws_url(input: &str) -> PyResult<Url> {
    let owned;
    let to_parse: &str = if input.contains("://") {
        input
    } else {
        owned = format!("ws://{input}");
        &owned
    };
    let url = Url::parse(to_parse)
        .map_err(|e| PyValueError::new_err(format!("invalid url {input:?}: {e}")))?;
    if !matches!(url.scheme(), "ws" | "http") {
        return Err(PyValueError::new_err(format!(
            "invalid url scheme {:?}: expected ws or http (or none)",
            url.scheme()
        )));
    }
    if url.host_str().is_none() {
        return Err(PyValueError::new_err(format!("url {input:?} has no host")));
    }
    Ok(url)
}
