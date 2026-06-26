//! The `self.p` protocol objectthe cases drive.
//!
//! Roughly reimplements [`WebSocketProtocol`](https://github.com/crossbario/autobahn-python/blob/v0.10.9/autobahn/websocket/protocol.py#L476),
//! exposing the subset the cases use; framing follows RFC 6455.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::MutexExt;
use pyo3::types::{PyAnyMethods, PyList};
use pyo3::types::{PyBytes, PyDict, PyString};
use rand::RngExt as _;
use tungstenite::protocol::frame::coding::OpCode;
use tungstenite::protocol::frame::{Frame, FrameHeader};

use crate::traffic::TrafficStats;

use crate::wirelog::{WireEntry, asciilog, binlog, mask_hex};

/// A scheduled `continueLater` callback.
pub struct Later {
    /// When the callback is due.
    pub at: Instant,
    /// The Python callback to invoke.
    pub func: Py<PyAny>,
    /// The wirelog label the case attached, logged in the `CT`/`CTE` entries.
    pub tag: Option<String>,
}

/// The mutable per-connection state, read and written by case classes.
#[allow(clippy::struct_excessive_bools)]
pub struct ProtocolInner {
    /// Frames serialized by `self.p` methods, awaiting the driver's flush. Each
    /// entry is written (and flushed) as a unit, so `chopsize` chunks stay
    /// separate TCP writes.
    pub out_queue: Vec<Bytes>,

    pub connection_was_open: bool,
    pub we_sent_close: bool,
    pub closed_by_me: bool,
    pub failed_by_me: bool,
    pub received_close: bool,
    pub remote_close_code: Option<u16>,
    pub remote_close_reason: Option<Bytes>,
    pub local_close_code: Option<u16>,
    pub was_clean: bool,
    pub was_not_clean_reason: Option<String>,
    pub was_server_connection_drop_timeout: bool,
    pub was_open_handshake_timeout: bool,
    pub was_close_handshake_timeout: bool,
    pub dropped_by_me: bool,
    pub local_close_reason: Option<Bytes>,

    /// The opening-handshake request/response heads, verbatim (reported as
    /// `httpRequest`/`httpResponse`). Set by the driver before the case runs.
    pub http_request: String,
    pub http_response: String,

    pub kill_at: Option<Instant>,
    pub close_at: Option<Instant>,
    pub later: Vec<Later>,

    /// The ordered wire-log trace (octets, frames, timer events). Recorded only
    /// while `create_wirelog` is set (cases disable it around huge payloads).
    pub wirelog: Vec<WireEntry>,
    pub create_wirelog: bool,
    pub auto_fragment_size: i64,
    /// Frame counts keyed by opcode (the case-visible `txFrameStats`/`rxFrameStats`).
    /// 10.1.1 checks the data/continuation frame split.
    pub tx_frame_stats: HashMap<u8, u32>,
    pub rx_frame_stats: HashMap<u8, u32>,
    /// Octet write/read counts keyed by length (`txOctetStats`/`rxOctetStats`).
    pub tx_octet_stats: HashMap<usize, u32>,
    pub rx_octet_stats: HashMap<usize, u32>,
    /// Data-traffic counters feeding the `trafficStats` report. The getter hands
    /// the case this same (live) object; reads are via `traffic.get()`.
    pub traffic: Py<TrafficStats>,

    /// Compression offers that may be set by a case during initialization.
    offers: Option<Py<PyList>>,
    /// Callable indicating whether compression is accepted.
    accept: Option<Py<PyAny>>,

    /// Streaming-send state (`beginMessage`/`beginMessageFrame`/
    /// `sendMessageFrameData`/`endMessage`): the in-progress frame's opcode and
    /// the mask key plus running offset so chunks mask continuously (6.4.3/4).
    pub stream_opcode: u8,
    pub stream_mask: [u8; 4],
    pub stream_mask_off: usize,

    /// Negotiated permessage-deflate session, once the handshake agrees on it
    /// (12.x/13.x). `None` means messages are sent/received uncompressed.
    pub compress: Option<crate::deflate::Deflate>,

    /// Whether we are the server endpoint (fuzzingserver mode). Servers send
    /// unmasked frames; clients mask. Also surfaced via `self.p.factory.isServer`.
    pub is_server: bool,
}

impl ProtocolInner {
    pub fn new(py: Python<'_>, is_server: bool) -> PyResult<Self> {
        Ok(ProtocolInner {
            is_server,
            out_queue: Vec::new(),
            connection_was_open: true, // set false only if the handshake fails
            we_sent_close: false,
            closed_by_me: false,
            failed_by_me: false,
            received_close: false,
            remote_close_code: None,
            remote_close_reason: None,
            local_close_code: None,
            was_clean: false,
            was_not_clean_reason: None,
            was_server_connection_drop_timeout: false,
            was_open_handshake_timeout: false,
            was_close_handshake_timeout: false,
            dropped_by_me: false,
            local_close_reason: None,
            http_request: String::new(),
            http_response: String::new(),
            kill_at: None,
            close_at: None,
            later: Vec::new(),
            wirelog: Vec::new(),
            // Autobahn's FuzzingProtocol defaults this on; cases toggle it off
            // around large payloads (9.x/12.x) to bound the log.
            create_wirelog: true,
            auto_fragment_size: 0,
            tx_frame_stats: HashMap::new(),
            rx_frame_stats: HashMap::new(),
            tx_octet_stats: HashMap::new(),
            rx_octet_stats: HashMap::new(),
            stream_opcode: 1,
            stream_mask: [0u8; 4],
            stream_mask_off: 0,
            offers: None,
            accept: None,
            compress: None,
            traffic: Py::new(py, TrafficStats::default())?,
        })
    }

    /// Queue one TCP write's worth of octets: record the `txOctetStats` count and
    /// (when logging) a `TO` wire-log entry, then enqueue for the driver to flush.
    fn queue(&mut self, chunk: Bytes, sync: bool) {
        let len = chunk.len();
        *self.tx_octet_stats.entry(len).or_insert(0) += 1;
        if self.create_wirelog {
            self.wirelog.push(WireEntry::TxOctets {
                len,
                data: binlog(&chunk),
                sync,
            });
        }
        self.out_queue.push(chunk);
    }

    /// Queue a frame header declaring `length` payload octets (no payload yet),
    /// then stream the masked payload via `send_message_frame_data`. Used to put
    /// a single frame on the wire in several TCP writes (6.4.3/6.4.4).
    pub fn begin_message_frame(&mut self, length: usize) -> std::io::Result<()> {
        let mask = self.next_mask();
        self.stream_mask = mask.unwrap_or([0; 4]);
        self.stream_mask_off = 0;
        let opcode = self.stream_opcode;
        *self.tx_frame_stats.entry(opcode).or_insert(0) += 1;
        let header = FrameHeader {
            is_final: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: OpCode::from(opcode),
            mask,
        };
        let mut buf = Vec::new();
        header
            .format(length as u64, &mut buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        // The payload arrives in later sendMessageFrameData calls (logged as TO
        // octets); log the frame now with its declared length.
        if self.create_wirelog {
            self.wirelog.push(WireEntry::TxFrame {
                len: length,
                data: String::new(),
                opcode,
                fin: true,
                rsv: 0,
                mask: mask_hex(mask),
                repeat_length: None,
                chopsize: None,
                sync: false,
            });
        }
        if opcode <= 2 {
            self.traffic
                .get()
                .sent_frame(buf.len() as u64, length as u64);
        }
        self.queue(Bytes::from(buf), false);
        Ok(())
    }

    /// Mask (client) and queue one chunk of the current streamed frame's
    /// payload. Server endpoints send unmasked, so the data passes through.
    pub fn send_message_frame_data(&mut self, data: Bytes) {
        let len = data.len();
        self.stream_mask_off += len;
        let chunk: Bytes = if self.is_server {
            data
        } else {
            let mut buf = BytesMut::from(data);
            for (i, b) in buf.iter_mut().enumerate() {
                *b ^= self.stream_mask[(self.stream_mask_off - len + i) % 4];
            }
            buf.freeze()
        };
        if self.stream_opcode <= 2 {
            self.traffic.get().sent_frame_chunk(chunk.len() as u64);
        }
        self.queue(chunk, false);
    }

    /// A fresh client mask, or `None` for a server endpoint (servers don't mask).
    fn next_mask(&self) -> Option<[u8; 4]> {
        (!self.is_server).then(|| rand::rng().random())
    }

    /// Serialize a frame (tungstenite codec, masked) and queue its bytes for the
    /// driver to write, optionally split into `chopsize` separate TCP writes.
    ///
    /// `repeat_length`, if set, repeats `payload` (or zero-fills, when empty) to
    /// that many octets on the wire — the fuzzer's way of cheaply emitting a huge
    /// frame. The wire-log records the *base* payload plus `repeat_length`, as
    /// Autobahn does. `sync` is carried into the log only (we flush every write).
    #[allow(clippy::too_many_arguments)]
    pub fn send_frame(
        &mut self,
        opcode: u8,
        fin: bool,
        rsv: u8,
        payload: Bytes,
        chopsize: Option<usize>,
        repeat_length: Option<usize>,
        sync: bool,
    ) -> std::io::Result<()> {
        *self.tx_frame_stats.entry(opcode).or_insert(0) += 1;
        let mask = self.next_mask();
        let header = FrameHeader {
            is_final: fin,
            // Autobahn's `rsv` is the 3-bit field value: 4=RSV1, 2=RSV2, 1=RSV3.
            rsv1: rsv & 0x4 != 0,
            rsv2: rsv & 0x2 != 0,
            rsv3: rsv & 0x1 != 0,
            opcode: OpCode::from(opcode),
            mask,
        };
        // Wire-log records the *base* payload (pre-repeat); log it before
        // `payload` is (possibly) moved into `wire_payload`.
        if self.create_wirelog {
            self.wirelog.push(WireEntry::TxFrame {
                len: payload.len(),
                data: asciilog(&payload),
                opcode,
                fin,
                rsv,
                mask: mask_hex(mask),
                repeat_length,
                chopsize,
                sync,
            });
        }
        let wire_payload: Bytes = match repeat_length {
            Some(target) if payload.is_empty() => Bytes::from(vec![0u8; target]),
            Some(target) => payload
                .iter()
                .copied()
                .cycle()
                .take(target)
                .collect::<Bytes>(),
            None => payload,
        };
        let ws_len = wire_payload.len();
        let frame = Frame::from_payload(header, wire_payload);
        let mut buf = Vec::new();
        frame
            .format(&mut buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if opcode <= 2 {
            self.traffic
                .get()
                .sent_frame(buf.len() as u64, ws_len as u64);
        }
        let buf = Bytes::from(buf);
        match chopsize {
            // Each chopsize chunk is queued separately → its own TCP write+flush
            // (zero-copy slices into the serialized frame).
            Some(cs) if cs > 0 => {
                let mut offset = 0;
                while offset < buf.len() {
                    let end = (offset + cs).min(buf.len());
                    self.queue(buf.slice(offset..end), sync);
                    offset = end;
                }
            }
            _ => self.queue(buf, sync),
        }
        Ok(())
    }

    /// Send a close frame and update the close-initiation state.
    ///
    /// The payload is `code` (2 bytes, big-endian) followed by `reason`, with
    /// each part included independently of the other. A fuzzer needs to emit
    /// malformed close frames — a reason without a code (1-byte payload), or an
    /// over-long reason (>125-octet control frame) — so neither is validated or
    /// clamped here.
    pub fn send_close(&mut self, code: Option<u16>, reason: Option<Bytes>) -> std::io::Result<()> {
        let mut payload = BytesMut::new();
        if let Some(c) = code {
            payload.extend_from_slice(&c.to_be_bytes());
        }
        if let Some(r) = &reason {
            payload.extend_from_slice(r);
        }
        self.local_close_reason = reason;
        self.local_close_code = code;
        let res = self.send_frame(8, true, 0, payload.freeze(), None, None, false);
        if !self.received_close {
            self.closed_by_me = true;
        }
        self.we_sent_close = true;
        res
    }
}

/// The `self.p` object passed to each Autobahn case.
#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub struct Protocol {
    inner: Arc<Mutex<ProtocolInner>>,
}

impl Protocol {
    pub fn new(py: Python<'_>, is_server: bool) -> PyResult<Self> {
        Ok(Protocol {
            inner: Arc::new(Mutex::new(ProtocolInner::new(py, is_server)?)),
        })
    }

    /// Lock the shared state off the GIL (the async driver loop). A GIL-holding
    /// caller must use [`Protocol::lock_py`] instead. Never hold the guard across
    /// an `.await` or a call back into Python (the case may re-enter `self.p`).
    pub fn lock(&self) -> MutexGuard<'_, ProtocolInner> {
        self.inner.lock().unwrap()
    }

    /// Like [`Protocol::lock`], for callers that hold the GIL: detaches from the
    /// interpreter while blocked on the lock so another thread can acquire the
    /// GIL (avoids a lock-vs-GIL deadlock).
    pub fn lock_py(&self, py: Python<'_>) -> MutexGuard<'_, ProtocolInner> {
        self.inner.lock_py_attached(py).unwrap()
    }
}

/// Encode a Python payload argument to wire bytes. `bytes` always pass through
/// unchanged (already exact, e.g. from `binascii`).
///
/// A `str` is encoded per the frame it rides in (`text`):
/// - TEXT frames carry UTF-8 on the wire, so a `str` text payload is UTF-8
///   encoded. This makes a py2 non-ASCII *text* literal (e.g. `"Hello-µ"`,
///   which py3 reads as codepoints) hit the wire as the same UTF-8 bytes py2
///   sent, and round-trip back through our UTF-8 TEXT decode (PORTING.md §4b).
/// - Otherwise (binary/control frames) a `str` is latin-1 encoded
///   (codepoint→byte, 1:1) to reproduce py2 byte-literal semantics. A codepoint
///   above 0xFF has no single-byte form, so it is rejected rather than silently
///   re-encoded (no case in the suite hits this — byte-precise payloads arrive
///   as `bytes`, and non-ASCII text rides TEXT frames).
///
/// Byte-precise invalid-UTF-8 *text* vectors (6.3/6.4) are bundled as real
/// `bytes`, so they pass through rather than being UTF-8 re-encoded here.
fn payload_to_bytes(payload: Option<&Bound<'_, PyAny>>, text: bool) -> PyResult<Bytes> {
    let Some(obj) = payload else {
        return Ok(Bytes::new());
    };
    if let Ok(b) = obj.extract::<Bytes>() {
        return Ok(b);
    }
    // Indexing a `bytes` object yields an int under py3 (a 1-char str in py2),
    // e.g. case6_4_2 sends `self.PAYLOAD[12]`; treat it as that single octet.
    if let Ok(n) = obj.extract::<i64>() {
        let byte = u8::try_from(n & 0xff).expect("masked to one byte");
        return Ok(Bytes::copy_from_slice(&[byte]));
    }
    let s = obj.extract::<String>()?;
    if text {
        return Ok(Bytes::from(s.into_bytes()));
    }
    let mut out = BytesMut::with_capacity(s.len());
    for ch in s.chars() {
        let byte = u8::try_from(ch as u32).map_err(|_| {
            PyValueError::new_err(format!(
                "codepoint U+{:04X} has no single-byte (latin-1) form for a binary/control payload",
                ch as u32
            ))
        })?;
        out.put_u8(byte);
    }
    Ok(out.freeze())
}

#[pymethods]
impl Protocol {
    // Constants the cases reference via `self.p.<NAME>`.
    #[classattr]
    const CLOSE_STATUS_CODE_NORMAL: u16 = 1000;
    #[classattr]
    const CLOSE_STATUS_CODE_GOING_AWAY: u16 = 1001;
    #[classattr]
    const CLOSE_STATUS_CODE_PROTOCOL_ERROR: u16 = 1002;
    #[classattr]
    const CLOSE_STATUS_CODE_UNSUPPORTED_DATA: u16 = 1003;
    #[classattr]
    const CLOSE_STATUS_CODE_INVALID_PAYLOAD: u16 = 1007;
    #[classattr]
    const CLOSE_STATUS_CODE_POLICY_VIOLATION: u16 = 1008;
    #[classattr]
    const CLOSE_STATUS_CODE_MESSAGE_TOO_BIG: u16 = 1009;
    #[classattr]
    const CLOSE_STATUS_CODE_INTERNAL_ERROR: u16 = 1011;
    #[classattr]
    const STATE_OPEN: u8 = 3;

    #[pyo3(name = "sendFrame", signature = (opcode, payload=None, fin=true, rsv=0, mask=None, payload_len=None, chopsize=None, sync=false))]
    #[allow(clippy::too_many_arguments)]
    fn send_frame(
        &self,
        py: Python<'_>,
        opcode: u8,
        payload: Option<&Bound<'_, PyAny>>,
        fin: bool,
        rsv: u8,
        mask: Option<&Bound<'_, PyAny>>,
        payload_len: Option<usize>,
        chopsize: Option<usize>,
        sync: bool,
    ) -> PyResult<()> {
        let _ = mask; // mask is auto-generated per the role (client masks, server doesn't)
        // TEXT (1) and its CONTINUATION (0) frames carry UTF-8; a `str` payload
        // is UTF-8 encoded for both so a fragmented text message stays valid on
        // the wire. Binary/control str payloads stay byte-precise (latin-1).
        let bytes = payload_to_bytes(payload, opcode == 0 || opcode == 1)?;
        py.detach(|| {
            self.lock()
                .send_frame(opcode, fin, rsv, bytes, chopsize, payload_len, sync)
                .map_err(|e| PyIOError::new_err(e.to_string()))
        })
    }

    // camelCase params (isBinary, fragmentSize, doNotCompress) match the kwargs
    // the cases pass, so they keep their names (localized allow).
    #[allow(non_snake_case)]
    #[pyo3(name = "sendMessage", signature = (payload=None, isBinary=false, fragmentSize=None, sync=false, doNotCompress=false))]
    fn send_message(
        &self,
        py: Python<'_>,
        payload: Option<&Bound<'_, PyAny>>,
        isBinary: bool,
        fragmentSize: Option<usize>,
        sync: bool,
        doNotCompress: bool,
    ) -> PyResult<()> {
        let _ = (sync, doNotCompress);
        let mut bytes = payload_to_bytes(payload, !isBinary)?;
        py.detach(|| {
            let opcode = if isBinary { 2 } else { 1 };
            let mut conn = self.lock();

            // The uncompressed message size is the "app level" traffic; one message.
            conn.traffic.get().sent_message(bytes.len() as u64);

            // permessage-deflate: compress the whole message and flag RSV1 on its
            // first frame; fragmentation then splits the *compressed* octets.
            let mut first_rsv = 0u8;
            if let Some(deflate) = conn.compress.as_mut() {
                bytes = deflate.compress(bytes);
                first_rsv = 0x4; // RSV1
            }

            // An explicit fragmentSize wins; otherwise auto-fragment when the case
            // has set autoFragmentSize (10.x).
            let effective_fragment = fragmentSize.or_else(|| {
                usize::try_from(conn.auto_fragment_size)
                    .ok()
                    .filter(|&n| n > 0)
            });
            match effective_fragment {
                // Split into data + continuation frames of `fragmentSize` octets
                // (zero-copy slices of the message payload).
                Some(fs) if fs > 0 && fs < bytes.len() => {
                    let total = bytes.len();
                    let mut offset = 0;
                    while offset < total {
                        let end = (offset + fs).min(total);
                        // The opcode + RSV1 (compression flag) ride the first frame;
                        // continuations carry opcode 0 and no RSV.
                        let (op, rsv) = if offset == 0 {
                            (opcode, first_rsv)
                        } else {
                            (0, 0)
                        };
                        conn.send_frame(
                            op,
                            end == total,
                            rsv,
                            bytes.slice(offset..end),
                            None,
                            None,
                            false,
                        )
                        .map_err(|e| PyIOError::new_err(e.to_string()))?;
                        offset = end;
                    }
                    Ok(())
                }
                _ => conn
                    .send_frame(opcode, true, first_rsv, bytes, None, None, false)
                    .map_err(|e| PyIOError::new_err(e.to_string())),
            }
        })
    }

    #[pyo3(name = "sendClose", signature = (code=None, reason=None))]
    fn send_close(
        &self,
        py: Python<'_>,
        code: Option<u16>,
        reason: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        // A missing reason (`None`) and an empty reason yield different frames, so
        // the `Option` is preserved: encode (byte-precise; cases send invalid
        // UTF-8 here, e.g. 7.5.1) only when a reason was actually passed.
        let reason = reason
            .map(|r| payload_to_bytes(Some(r), false))
            .transpose()?;
        py.detach(|| self.send_close_impl(code, reason))
    }

    // `reasonUtf8` matches the kwarg the 7.3/7.5 cases pass (localized allow).
    #[allow(non_snake_case)]
    #[pyo3(name = "sendCloseFrame", signature = (code=None, reasonUtf8=None, isReply=false))]
    fn send_close_frame(
        &self,
        py: Python<'_>,
        code: Option<u16>,
        reasonUtf8: Option<&Bound<'_, PyAny>>,
        isReply: bool,
    ) -> PyResult<()> {
        let _ = isReply; // reply vs initiated close: irrelevant to the bytes we emit
        let reason = reasonUtf8
            .map(|r| payload_to_bytes(Some(r), false))
            .transpose()?;
        py.detach(|| self.send_close_impl(code, reason))
    }

    #[pyo3(name = "closeAfter")]
    fn close_after(&self, py: Python<'_>, secs: f64) {
        let mut c = self.lock_py(py);
        c.close_at = Some(Instant::now() + Duration::from_secs_f64(secs));
        c.wirelog.push(WireEntry::CloseScheduled { delay: secs });
    }

    #[pyo3(name = "killAfter")]
    fn kill_after(&self, py: Python<'_>, secs: f64) {
        let mut c = self.lock_py(py);
        c.kill_at = Some(Instant::now() + Duration::from_secs_f64(secs));
        c.wirelog.push(WireEntry::KillScheduled { delay: secs });
    }

    // `tag` is the wirelog label Autobahn attaches to the CT/CTE entries; the
    // callback itself takes no args.
    #[pyo3(name = "continueLater", signature = (secs, func, tag=None))]
    fn continue_later(
        &self,
        py: Python<'_>,
        secs: f64,
        func: Py<PyAny>,
        tag: Option<&Bound<'_, PyAny>>,
    ) {
        let tag = tag.and_then(|t| t.str().ok()).map(|s| s.to_string());
        let mut c = self.lock_py(py);
        c.wirelog.push(WireEntry::ContinueScheduled {
            delay: secs,
            tag: tag.clone(),
        });
        c.later.push(Later {
            at: Instant::now() + Duration::from_secs_f64(secs),
            func,
            tag,
        });
    }

    // --- streaming send API (a message put on the wire frame-by-frame, each
    //     frame optionally in several payload chunks) ---
    #[pyo3(name = "beginMessage", signature = (is_binary=false))]
    fn begin_message(&self, py: Python<'_>, is_binary: bool) {
        self.lock_py(py).stream_opcode = if is_binary { 2 } else { 1 };
    }

    #[pyo3(name = "beginMessageFrame")]
    fn begin_message_frame(&self, py: Python<'_>, length: usize) -> PyResult<()> {
        py.detach(|| {
            self.lock()
                .begin_message_frame(length)
                .map_err(|e| PyIOError::new_err(e.to_string()))
        })
    }

    #[pyo3(name = "sendMessageFrameData")]
    fn send_message_frame_data(
        &self,
        py: Python<'_>,
        payload: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let text = self.lock_py(py).stream_opcode == 1;
        let bytes = payload_to_bytes(payload, text)?;
        py.detach(|| self.lock().send_message_frame_data(bytes));
        Ok(())
    }

    // The frame's length and FIN were committed in beginMessageFrame, so there
    // is nothing more to write; provided so cases can bracket their sends.
    #[pyo3(name = "endMessage")]
    #[allow(clippy::unused_self)] // a `self.p` method the cases call
    fn end_message(&self) {}

    // Toggle wire-log capture, recording the change as a `WLM` entry (matching
    // Autobahn — the marker is logged even when turning logging off).
    #[pyo3(name = "enableWirelog")]
    fn enable_wirelog(&self, py: Python<'_>, enable: bool) {
        let mut c = self.lock_py(py);
        if enable != c.create_wirelog {
            c.create_wirelog = enable;
            c.wirelog.push(WireEntry::WirelogMode(enable));
        }
    }

    // --- attributes the cases read/set ---
    #[getter(createWirelog)]
    fn create_wirelog(&self, py: Python<'_>) -> bool {
        self.lock_py(py).create_wirelog
    }
    #[setter(createWirelog)]
    fn set_create_wirelog(&self, py: Python<'_>, v: bool) {
        self.lock_py(py).create_wirelog = v;
    }
    #[getter(autoFragmentSize)]
    fn auto_fragment_size(&self, py: Python<'_>) -> i64 {
        self.lock_py(py).auto_fragment_size
    }
    #[setter(autoFragmentSize)]
    fn set_auto_fragment_size(&self, py: Python<'_>, v: i64) {
        self.lock_py(py).auto_fragment_size = v;
    }

    #[getter(connectionWasOpen)]
    fn connection_was_open(&self, py: Python<'_>) -> bool {
        self.lock_py(py).connection_was_open
    }
    #[getter(closedByMe)]
    fn closed_by_me(&self, py: Python<'_>) -> bool {
        self.lock_py(py).closed_by_me
    }
    #[getter(wasClean)]
    fn was_clean(&self, py: Python<'_>) -> bool {
        self.lock_py(py).was_clean
    }
    #[getter(droppedByMe)]
    fn dropped_by_me(&self, py: Python<'_>) -> bool {
        self.lock_py(py).dropped_by_me
    }
    #[getter(remoteCloseCode)]
    fn remote_close_code(&self, py: Python<'_>) -> Option<u16> {
        self.lock_py(py).remote_close_code
    }
    // The reason bytes we last sent in a close frame, as `bytes` (cases such as
    // 7.5.1 hex-encode it for reporting). `None` if no reason was sent.
    #[getter(localCloseReason)]
    fn local_close_reason(&self, py: Python<'_>) -> Option<Py<PyBytes>> {
        self.lock_py(py)
            .local_close_reason
            .as_ref()
            .map(|r| PyBytes::new(py, r).unbind())
    }
    // Cases reassign this attribute (e.g. to its hex form); we keep no need for
    // the written value, so accept and drop it.
    #[setter(localCloseReason)]
    #[allow(clippy::unused_self)] // a `self.p` setter the cases assign
    fn set_local_close_reason(&self, _value: &Bound<'_, PyAny>) {}

    #[getter(perMessageCompressionOffers)]
    pub(crate) fn offers(&self, py: Python<'_>) -> Option<Py<PyList>> {
        self.lock_py(py).offers.as_ref().map(|o| o.clone_ref(py))
    }
    #[setter(perMessageCompressionOffers)]
    fn set_offers(&self, py: Python<'_>, value: Py<PyList>) {
        self.lock_py(py).offers = Some(value);
    }
    #[getter(perMessageCompressionAccept)]
    pub(crate) fn accept(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.lock_py(py).accept.as_ref().map(|a| a.clone_ref(py))
    }
    #[setter(perMessageCompressionAccept)]
    fn set_accept(&self, py: Python<'_>, value: Py<PyAny>) {
        self.lock_py(py).accept = Some(value);
    }
    // The marker the cases key "deflate active?" off of (`is None`): `True` once
    // a deflate session is negotiated, `None` otherwise. Derived from `compress`
    // rather than stored — the driver only ever sets the session.
    #[getter(_perMessageCompress)]
    fn compress_marker(&self, py: Python<'_>) -> Option<bool> {
        self.lock_py(py).compress.is_some().then_some(true)
    }

    #[getter]
    #[allow(clippy::unused_self)] // a `self.p` getter the cases read
    fn state(&self) -> u8 {
        Self::STATE_OPEN
    }
    // Traffic counters the 12.x/13.x cases snapshot (`copy.deepcopy`) for the
    // compression-ratio report. Hands back the connection's own (live)
    // `TrafficStats`; the cases `deepcopy` it to take their snapshot.
    #[getter(trafficStats)]
    fn traffic_stats(&self, py: Python<'_>) -> Py<TrafficStats> {
        self.lock_py(py).traffic.clone_ref(py)
    }
    // Frames transmitted, keyed by opcode (e.g. {1: text frames, 0: continuations}).
    #[getter(txFrameStats)]
    fn tx_frame_stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        (&self.lock_py(py).tx_frame_stats).into_pyobject(py)
    }
    #[getter]
    fn factory(&self, py: Python<'_>) -> Factory {
        Factory {
            is_server: self.lock_py(py).is_server,
        }
    }
}

impl Protocol {
    fn send_close_impl(&self, code: Option<u16>, reason: Option<Bytes>) -> PyResult<()> {
        self.lock()
            .send_close(code, reason)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }
}

/// The `self.p.factory` object — cases read `.isServer`.
#[pyclass]
pub struct Factory {
    is_server: bool,
}

#[pymethods]
impl Factory {
    #[getter(isServer)]
    fn is_server(&self) -> bool {
        self.is_server
    }
}

/// Bridge a received data payload into the Python value the case expects:
/// text → `str` (UTF-8, latin-1 fallback), binary → `str` (latin-1, 1:1 byte→codepoint).
pub fn bridge_message<'py>(py: Python<'py>, payload: &[u8], binary: bool) -> Bound<'py, PyString> {
    // A valid-UTF-8 text payload, or any ASCII payload, is already its own `str`
    // form, so hand the bytes to Python directly without the latin-1 rebuild.
    // ASCII is both valid UTF-8 and identical to its latin-1 decoding, so this is
    // correct for binary too (where non-ASCII must stay byte→codepoint, 1:1).
    if (!binary || payload.is_ascii())
        && let Ok(s) = std::str::from_utf8(payload)
    {
        return PyString::new(py, s);
    }
    let s: String = payload.iter().map(|&b| b as char).collect();
    PyString::new(py, &s)
}
