//! The shared case-driver core used by both modes which executes I/O based on callbacks
//! from the test case Python code.
//!
//! Reimplements the case-driver role of [`FuzzingProtocol`](https://github.com/crossbario/autobahn-testsuite/blob/v25.10.1/autobahntestsuite/autobahntestsuite/fuzzing.py#L80):
//! run the case callbacks, evaluate the verdict, collect the report.

use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes, BytesMut};
use chrono::{SecondsFormat, Utc};
use http::{HeaderMap, HeaderName, HeaderValue};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyList, PyString};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tungstenite::protocol::frame::FrameHeader;
use tungstenite::protocol::frame::coding::{Control, Data, OpCode};

use crate::asyncrt::attach_blocking;
use crate::constants::Constants;
use crate::protocol::{Protocol, bridge_message};
use crate::result::{Behavior, BehaviorClose, CaseResult, Results};
use crate::wirelog::{WireEntry, asciilog, binlog, mask_hex, rsv_bits};

/// Run a case to completion over an already-open WebSocket connection: call
/// `onOpen`, pump the read/write/timer loop until the connection ends, then
/// `onConnectionLost`, read back the verdict, and push it into `results`. Both
/// modes share this loop; they differ only in how the connection is opened.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) async fn drive(
    stream: &mut TcpStream,
    rbuf: &mut BytesMut,
    protocol: &Protocol,
    case: Arc<Py<PyAny>>,
    case_id: &str,
    case_index: usize,
    agent: &str,
    constants: &Constants,
    results: &Results,
) -> PyResult<()> {
    let case_start = Instant::now();
    // ISO-8601 UTC, microseconds + `+00:00` offset — matching Autobahn's
    // `datetime.now(timezone.utc).isoformat()`.
    let started = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false);
    {
        let case = case.clone();
        let constants = constants.clone();
        attach_blocking(move |py| case.bind(py).call_method0(&constants.on_open).map(|_| ()))
            .await?;
    }

    // Whether the active message is permessage-deflate compressed (RSV1 on its
    // first frame); compressed messages are decompressed and delivered as bytes.
    let mut partial: Option<(BytesMut, bool, bool)> = None;
    // Scratch the flush step drains into; reused across iterations so neither it
    // nor `out_queue` reallocates after warmup.
    let mut chunks: Vec<Bytes> = Vec::new();
    loop {
        // Drain the due `continueLater` callbacks and capture the kill/close
        // deadlines.
        let now = Instant::now();
        let (due, kill_at, close_at) = {
            let mut c = protocol.lock();
            let mut due = Vec::new();
            let mut keep = Vec::new();
            for l in c.later.drain(..) {
                if l.at <= now {
                    due.push(l);
                } else {
                    keep.push(l);
                }
            }
            if !keep.is_empty() {
                c.later = keep;
            }
            (due, c.kill_at, c.close_at)
        };
        // Run the due callbacks.
        if !due.is_empty() {
            let protocol = protocol.clone();
            attach_blocking(move |py| {
                for l in due {
                    l.func.bind(py).call0()?;
                    protocol
                        .lock_py(py)
                        .wirelog
                        .push(WireEntry::ContinueExecuted { tag: l.tag });
                }
                Ok(())
            })
            .await?;
        }
        // Fire kill/close deadlines and decide whether to continue.
        let keep_going = {
            let mut c = protocol.lock();
            if kill_at.is_some_and(|k| now >= k) {
                c.wirelog.push(WireEntry::KillExecuted);
                c.dropped_by_me = true;
                c.failed_by_me = true;
                false
            } else {
                if close_at.is_some_and(|ca| now >= ca) {
                    c.wirelog.push(WireEntry::CloseExecuted);
                    c.send_close(Some(1000), None).ok();
                    c.close_at = None;
                }
                !(c.we_sent_close && c.received_close)
            }
        };
        if !keep_going {
            break;
        }

        // Flush queued frames to the wire. Move them out under the lock (keeping
        // `out_queue`'s storage) so the writes happen without holding it.
        chunks.append(&mut protocol.lock().out_queue);
        for chunk in chunks.drain(..) {
            if stream.write_all(&chunk).await.is_err() {
                break;
            }
            stream.flush().await.ok();
        }

        match read_frame(stream, rbuf, protocol, Duration::from_millis(50)).await {
            Ok(Some((hdr, payload))) => {
                let case = case.clone();
                let protocol = protocol.clone();
                let constants = constants.clone();
                let pending = partial;
                partial = attach_blocking(move |py| {
                    let mut partial = pending;
                    dispatch(
                        py,
                        case.bind(py),
                        &protocol,
                        &hdr,
                        payload,
                        &mut partial,
                        &constants,
                    )?;
                    Ok(partial)
                })
                .await?;
            }
            Ok(None) => {}
            Err(_) => break, // peer dropped the connection
        }
    }

    let duration = i64::try_from(case_start.elapsed().as_millis()).unwrap_or(i64::MAX);
    let dropped = protocol.lock().dropped_by_me;
    let protocol = protocol.clone();
    let constants = constants.clone();
    let case_id = case_id.to_string();
    let agent = agent.to_string();
    let result = attach_blocking(move |py| {
        let case = case.bind(py);
        case.call_method1(&constants.on_connection_lost, (dropped,))?;
        let result = build_case_result(
            py, case, &protocol, &case_id, case_index, &agent, &started, duration, &constants,
        )?;
        Py::new(py, result)
    })
    .await?;
    results.lock().expect("results mutex").push(result);
    Ok(())
}

/// Assemble the full `CaseResult` from the finished `Case`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_case_result(
    py: Python<'_>,
    case: &Bound<'_, PyAny>,
    protocol: &Protocol,
    case_id: &str,
    case_index: usize,
    agent: &str,
    started: &str,
    duration: i64,
    constants: &Constants,
) -> PyResult<CaseResult> {
    let c = protocol.lock_py(py);
    let close_reason = |bytes: &Option<Bytes>| -> Option<Py<PyString>> {
        bytes
            .as_ref()
            .map(|r| PyString::new(py, &String::from_utf8_lossy(r)).unbind())
    };
    let local_close_reason = close_reason(&c.local_close_reason);
    let remote_close_reason = close_reason(&c.remote_close_reason);

    let behavior = Behavior::from_label(case.getattr(&constants.behavior)?.extract::<&str>()?)?;
    let behavior_close =
        BehaviorClose::from_label(case.getattr(&constants.behavior_close)?.extract::<&str>()?)?;

    Ok(CaseResult {
        case_id: PyString::new(py, case_id).unbind(),
        case_index: case_index.into_pyobject(py).unwrap().unbind(),
        description: case.getattr(&constants.description)?.cast_into()?.unbind(),
        expectation: case.getattr(&constants.expectation)?.cast_into()?.unbind(),
        agent: PyString::new(py, agent).unbind(),
        started: PyString::new(py, started).unbind(),
        duration: duration.into_pyobject(py).unwrap().unbind(),
        report_time: case.getattr(&constants.report_time)?.cast_into()?.unbind(),
        report_compression_ratio: case
            .getattr(&constants.report_compression_ratio)?
            .cast_into()?
            .unbind(),
        behavior,
        behavior_close,
        expected: case.getattr(&constants.expected)?.unbind(),
        expected_close: case.getattr(&constants.expected_close)?.unbind(),
        received: case.getattr(&constants.received)?.unbind(),
        result: case.getattr(&constants.result)?.cast_into()?.unbind(),
        result_close: case.getattr(&constants.result_close)?.cast_into()?.unbind(),
        wirelog: PyList::new(py, &c.wirelog)?.unbind(),
        create_wirelog: PyBool::new(py, c.create_wirelog).to_owned().unbind(),
        closed_by_me: PyBool::new(py, c.closed_by_me).to_owned().unbind(),
        failed_by_me: PyBool::new(py, c.failed_by_me).to_owned().unbind(),
        dropped_by_me: PyBool::new(py, c.dropped_by_me).to_owned().unbind(),
        was_clean: PyBool::new(py, c.was_clean).to_owned().unbind(),
        was_not_clean_reason: c
            .was_not_clean_reason
            .as_deref()
            .map(|r| PyString::new(py, r).unbind()),
        was_server_connection_drop_timeout: PyBool::new(py, c.was_server_connection_drop_timeout)
            .to_owned()
            .unbind(),
        was_open_handshake_timeout: PyBool::new(py, c.was_open_handshake_timeout)
            .to_owned()
            .unbind(),
        was_close_handshake_timeout: PyBool::new(py, c.was_close_handshake_timeout)
            .to_owned()
            .unbind(),
        local_close_code: c
            .local_close_code
            .map(|v| v.into_pyobject(py).unwrap().unbind()),
        local_close_reason,
        remote_close_code: c
            .remote_close_code
            .map(|v| v.into_pyobject(py).unwrap().unbind()),
        remote_close_reason,
        is_server: PyBool::new(py, c.is_server).to_owned().unbind(),
        create_stats: PyBool::new(py, true).to_owned().unbind(),
        rx_octet_stats: (&c.rx_octet_stats).into_pyobject(py)?.unbind(),
        rx_frame_stats: (&c.rx_frame_stats).into_pyobject(py)?.unbind(),
        tx_octet_stats: (&c.tx_octet_stats).into_pyobject(py)?.unbind(),
        tx_frame_stats: (&c.tx_frame_stats).into_pyobject(py)?.unbind(),
        http_request: PyString::new(py, &c.http_request).unbind(),
        http_response: PyString::new(py, &c.http_response).unbind(),
        traffic_stats: case.getattr(&constants.traffic_stats)?.extract()?,
    })
}

pub(crate) fn invalid_http(err: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

pub(crate) fn to_header_map(headers: &[httparse::Header<'_>]) -> std::io::Result<HeaderMap> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for h in headers {
        let name = HeaderName::from_bytes(h.name.as_bytes()).map_err(invalid_http)?;
        let value = HeaderValue::from_bytes(h.value).map_err(invalid_http)?;
        map.append(name, value);
    }
    Ok(map)
}

/// Read from `stream` until `parse` reports a complete HTTP head. Returns the
/// parsed head and the read buffer advanced past it — i.e. holding the bytes
/// that follow the head (the start of the WebSocket frame stream).
pub(crate) async fn read_head<T>(
    stream: &mut TcpStream,
    parse: impl Fn(&[u8]) -> std::io::Result<Option<(T, usize)>>,
) -> std::io::Result<(T, BytesMut)> {
    let mut rbuf = BytesMut::new();
    loop {
        rbuf.reserve(1024);
        if stream.read_buf(&mut rbuf).await? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "peer closed during handshake",
            ));
        }
        if let Some((head, len)) = parse(&rbuf)? {
            rbuf.advance(len);
            return Ok((head, rbuf));
        }
    }
}

/// Read one frame, waiting up to `timeout`.
async fn read_frame(
    stream: &mut TcpStream,
    rbuf: &mut BytesMut,
    protocol: &Protocol,
    timeout: Duration,
) -> std::io::Result<Option<(FrameHeader, Bytes)>> {
    match tokio::time::timeout(timeout, read_frame_inner(stream, rbuf, protocol)).await {
        Ok(result) => result,
        Err(_elapsed) => Ok(None),
    }
}

async fn read_frame_inner(
    stream: &mut TcpStream,
    rbuf: &mut BytesMut,
    protocol: &Protocol,
) -> std::io::Result<Option<(FrameHeader, Bytes)>> {
    loop {
        let parsed = {
            let mut cur = Cursor::new(&rbuf[..]);
            match FrameHeader::parse(&mut cur) {
                Ok(Some((hdr, plen))) => Some((
                    hdr,
                    usize::try_from(cur.position()).expect("buffer offset fits usize"),
                    usize::try_from(plen).expect("frame length fits usize"),
                )),
                Ok(None) => None,
                Err(e) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    ));
                }
            }
        };
        if let Some((hdr, hsize, plen)) = parsed {
            let total = hsize + plen;
            if rbuf.len() >= total {
                let mut frame = rbuf.split_to(total);
                let mut payload = frame.split_off(hsize);
                // Client→server frames are masked; unmask in place.
                if let Some(mask) = hdr.mask {
                    for (i, b) in payload.iter_mut().enumerate() {
                        *b ^= mask[i % 4];
                    }
                }
                // Record the received frame.
                let opcode = u8::from(hdr.opcode);
                {
                    let mut c = protocol.lock();
                    *c.rx_frame_stats.entry(opcode).or_insert(0) += 1;
                    if opcode <= 2 {
                        c.traffic.get().received_frame(total as u64, plen as u64);
                    }
                    if c.create_wirelog {
                        c.wirelog.push(WireEntry::RxFrame {
                            len: payload.len(),
                            data: asciilog(&payload),
                            opcode,
                            fin: hdr.is_final,
                            rsv: rsv_bits(hdr.rsv1, hdr.rsv2, hdr.rsv3),
                            masked: hdr.mask.is_some(),
                            mask: mask_hex(hdr.mask),
                        });
                    }
                }
                return Ok(Some((hdr, payload.freeze())));
            }
        }
        // Append more bytes straight into `rbuf` (read_buf writes the spare
        // capacity, growing the length); the just-read octets are the tail.
        rbuf.reserve(8192);
        let before = rbuf.len();
        let n = stream.read_buf(rbuf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        // Record the octets read: rxOctetStats and (when logging) an RO entry.
        {
            let mut c = protocol.lock();
            *c.rx_octet_stats.entry(n).or_insert(0) += 1;
            if c.create_wirelog {
                c.wirelog.push(WireEntry::RxOctets {
                    len: n,
                    data: binlog(&rbuf[before..]),
                });
            }
        }
    }
}

fn dispatch(
    py: Python<'_>,
    case: &Bound<'_, PyAny>,
    protocol: &Protocol,
    hdr: &FrameHeader,
    payload: Bytes,
    partial: &mut Option<(BytesMut, bool, bool)>,
    constants: &Constants,
) -> PyResult<()> {
    match hdr.opcode {
        OpCode::Data(Data::Text | Data::Binary) => {
            let binary = matches!(hdr.opcode, OpCode::Data(Data::Binary));
            if hdr.is_final {
                deliver(py, case, protocol, payload, binary, hdr.rsv1, constants)?;
            } else {
                // Start the reassembly buffer (RSV1, the compression flag, is set
                // on the first frame only). `from` reclaims the payload in place
                // when it's uniquely owned, so no copy in the common case.
                *partial = Some((BytesMut::from(payload), binary, hdr.rsv1));
            }
        }
        OpCode::Data(Data::Continue) => {
            if let Some((buf, _, _)) = partial.as_mut() {
                buf.extend_from_slice(&payload);
                if hdr.is_final {
                    let (buf, bin, comp) = partial.take().unwrap();
                    deliver(py, case, protocol, buf.freeze(), bin, comp, constants)?;
                }
            }
        }
        OpCode::Control(Control::Ping) => {
            // Echo the ping back as a pong; `payload` is still needed below for
            // the onPing callback, so clone (cheap — refcounted).
            protocol
                .lock_py(py)
                .send_frame(10, true, 0, payload.clone(), None, None, false)
                .ok();
            // Deliver the payload as a latin-1 str, symmetric with how the cases
            // author ping payloads as str literals and how we send them (§4c).
            case.call_method1(&constants.on_ping, (bridge_message(py, &payload, true),))?;
        }
        OpCode::Control(Control::Pong) => {
            case.call_method1(&constants.on_pong, (bridge_message(py, &payload, true),))?;
        }
        OpCode::Control(Control::Close) => {
            let (code, reason_bytes) = if payload.len() >= 2 {
                (
                    Some(u16::from_be_bytes([payload[0], payload[1]])),
                    payload.slice(2..),
                )
            } else {
                (None, Bytes::new())
            };
            let need_echo = {
                let mut c = protocol.lock_py(py);
                c.received_close = true;
                c.remote_close_code = code;
                c.remote_close_reason = (!reason_bytes.is_empty()).then(|| reason_bytes.clone());
                if c.we_sent_close {
                    c.was_clean = true;
                }
                !c.we_sent_close
            };
            if need_echo {
                let mut c = protocol.lock_py(py);
                c.send_close(code, None).ok();
                c.was_clean = true;
            }
            let reason = PyString::new(py, &String::from_utf8_lossy(&reason_bytes));
            case.call_method1(&constants.on_close, (true, code, reason))?;
        }
        _ => {} // reserved opcodes from the server: ignore
    }
    Ok(())
}

/// Deliver a fully-reassembled message into the case's `onMessage`.
fn deliver(
    py: Python<'_>,
    case: &Bound<'_, PyAny>,
    protocol: &Protocol,
    buf: Bytes,
    binary: bool,
    compressed: bool,
    constants: &Constants,
) -> PyResult<()> {
    if compressed {
        let data = {
            let mut c = protocol.lock_py(py);
            match c.compress.as_mut() {
                // `detach` releases the GIL for the CPU-bound inflate.
                Some(d) => py
                    .detach(|| d.decompress(buf))
                    .map_err(|e| PyValueError::new_err(e.to_string()))?,
                None => buf,
            }
        };
        protocol
            .lock_py(py)
            .traffic
            .get()
            .received_message(data.len() as u64);
        case.call_method1(&constants.on_message, (data, binary))?;
    } else {
        protocol
            .lock_py(py)
            .traffic
            .get()
            .received_message(buf.len() as u64);
        let msg = bridge_message(py, &buf, binary);
        case.call_method1(&constants.on_message, (msg, binary))?;
    }
    Ok(())
}
