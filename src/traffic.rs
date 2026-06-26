//! Per-connection WebSocket data-traffic counters, exposed to the cases as
//! `autobahn.websocket.protocol.TrafficStats`. Only data frames/messages are
//! counted — not control frames or the handshake. "App" is the uncompressed
//! message payload, "WebSocket" the (possibly compressed) frame payload, "wire"
//! the full octets including framing.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::constants::Constants;

/// Per-case octet, frame, and message counters for sent and received traffic.
#[pyclass(frozen)]
#[derive(Default)]
pub struct TrafficStats {
    out_wire: AtomicU64,
    out_ws: AtomicU64,
    out_app: AtomicU64,
    out_frames: AtomicU64,
    out_messages: AtomicU64,
    in_wire: AtomicU64,
    in_ws: AtomicU64,
    in_app: AtomicU64,
    in_frames: AtomicU64,
    in_messages: AtomicU64,
}

impl TrafficStats {
    /// Count a sent data frame: its full wire octets and declared payload size.
    pub fn sent_frame(&self, wire_octets: u64, payload_octets: u64) {
        self.out_frames.fetch_add(1, Relaxed);
        self.out_wire.fetch_add(wire_octets, Relaxed);
        self.out_ws.fetch_add(payload_octets, Relaxed);
    }

    /// Count the wire octets of a streamed frame's payload, sent after the
    /// header was already counted by `sent_frame`.
    pub fn sent_frame_chunk(&self, wire_octets: u64) {
        self.out_wire.fetch_add(wire_octets, Relaxed);
    }

    /// Count a sent application message's (uncompressed) payload.
    pub fn sent_message(&self, app_octets: u64) {
        self.out_app.fetch_add(app_octets, Relaxed);
        self.out_messages.fetch_add(1, Relaxed);
    }

    /// Count a received data frame: its full wire octets and payload size.
    pub fn received_frame(&self, wire_octets: u64, payload_octets: u64) {
        self.in_frames.fetch_add(1, Relaxed);
        self.in_wire.fetch_add(wire_octets, Relaxed);
        self.in_ws.fetch_add(payload_octets, Relaxed);
    }

    /// Count a received application message's (uncompressed) payload.
    pub fn received_message(&self, app_octets: u64) {
        self.in_app.fetch_add(app_octets, Relaxed);
        self.in_messages.fetch_add(1, Relaxed);
    }

    /// A point-in-time copy of the counters.
    fn snapshot(&self) -> Self {
        let copy = |a: &AtomicU64| AtomicU64::new(a.load(Relaxed));
        Self {
            out_wire: copy(&self.out_wire),
            out_ws: copy(&self.out_ws),
            out_app: copy(&self.out_app),
            out_frames: copy(&self.out_frames),
            out_messages: copy(&self.out_messages),
            in_wire: copy(&self.in_wire),
            in_ws: copy(&self.in_ws),
            in_app: copy(&self.in_app),
            in_frames: copy(&self.in_frames),
            in_messages: copy(&self.in_messages),
        }
    }
}

#[pymethods]
impl TrafficStats {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.snapshot()
    }

    /// The counters as a dict, with derived compression ratios
    /// (compressed/uncompressed payload) and protocol overhead (framing vs.
    /// payload), for the report's `trafficStats`.
    #[allow(clippy::cast_precision_loss)]
    fn __json__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let out_wire = self.out_wire.load(Relaxed);
        let out_ws = self.out_ws.load(Relaxed);
        let out_app = self.out_app.load(Relaxed);
        let in_wire = self.in_wire.load(Relaxed);
        let in_ws = self.in_ws.load(Relaxed);
        let in_app = self.in_app.load(Relaxed);

        let ratio = |num: u64, den: u64| (den > 0).then(|| num as f64 / den as f64);

        let k = Constants::get(py);
        let dict = PyDict::new(py);
        dict.set_item(&k.outgoing_octets_wire_level, out_wire)?;
        dict.set_item(&k.outgoing_octets_websocket_level, out_ws)?;
        dict.set_item(&k.outgoing_octets_app_level, out_app)?;
        dict.set_item(&k.outgoing_compression_ratio, ratio(out_ws, out_app))?;
        dict.set_item(
            &k.outgoing_websocket_overhead,
            ratio(out_wire - out_ws, out_ws),
        )?;
        dict.set_item(&k.outgoing_websocket_frames, self.out_frames.load(Relaxed))?;
        dict.set_item(
            &k.outgoing_websocket_messages,
            self.out_messages.load(Relaxed),
        )?;
        dict.set_item(&k.preopen_outgoing_octets_wire_level, 0)?;
        dict.set_item(&k.incoming_octets_wire_level, in_wire)?;
        dict.set_item(&k.incoming_octets_websocket_level, in_ws)?;
        dict.set_item(&k.incoming_octets_app_level, in_app)?;
        dict.set_item(&k.incoming_compression_ratio, ratio(in_ws, in_app))?;
        dict.set_item(
            &k.incoming_websocket_overhead,
            ratio(in_wire - in_ws, in_ws),
        )?;
        dict.set_item(&k.incoming_websocket_frames, self.in_frames.load(Relaxed))?;
        dict.set_item(
            &k.incoming_websocket_messages,
            self.in_messages.load(Relaxed),
        )?;
        dict.set_item(&k.preopen_incoming_octets_wire_level, 0)?;
        Ok(dict)
    }
}
