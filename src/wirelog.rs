//! The wire log: an ordered trace of everything that crosses the connection —
//! octets read/written, frames sent/received, and timer events.
//!
//! Reimplements the log methods (`logRxOctets`/`logTxOctets`/`logRxFrame`/
//! `logTxFrame`) and the `binLogData`/`asciiLogData` encoders in
//! [`fuzzing.py`](https://github.com/crossbario/autobahn-testsuite/blob/v25.10.1/autobahntestsuite/autobahntestsuite/fuzzing.py#L196).

use faster_hex::hex_string;
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::constants::Constants;

/// Cap matching Autobahn's `maxlen` default; `ellipses` is appended past it.
const MAXLEN: usize = 64;
const ELLIPSES: &str = " ...";

/// Hex-encode up to `MAXLEN` octets, appending " ..." when truncated. Mirrors
/// Autobahn's `binLogData`.
pub fn binlog(data: &[u8]) -> String {
    let take = data.len().min(MAXLEN);
    let mut s = hex_string(&data[..take]);
    if data.len() > MAXLEN - ELLIPSES.len() {
        s.push_str(ELLIPSES);
    }
    s
}

/// Decode up to `MAXLEN` octets as UTF-8 text (appending " ..." when truncated),
/// falling back to `0x`-prefixed hex if the slice is not valid UTF-8. Mirrors
/// Autobahn's `asciiLogData` (default `replace=False`).
pub fn asciilog(data: &[u8]) -> String {
    let take = data.len().min(MAXLEN);
    let mut slice = data[..take].to_vec();
    if data.len() > MAXLEN - ELLIPSES.len() {
        slice.extend_from_slice(ELLIPSES.as_bytes());
    }
    match std::str::from_utf8(&slice) {
        Ok(s) => s.to_string(),
        Err(_) => format!("0x{}", binlog(data)),
    }
}

/// Hex of a mask key (`binascii.b2a_hex(mask)` in Autobahn), or `None` if unmasked.
pub fn mask_hex(mask: Option<[u8; 4]>) -> Option<String> {
    mask.map(|m| hex_string(&m))
}

/// The 3-bit RSV field value Autobahn logs (RSV1=4, RSV2=2, RSV3=1).
pub fn rsv_bits(rsv1: bool, rsv2: bool, rsv3: bool) -> u8 {
    (u8::from(rsv1) << 2) | (u8::from(rsv2) << 1) | u8::from(rsv3)
}

/// One wire-log entry. Variants map 1:1 to Autobahn's log tuples; see the
/// `IntoPyObject` impl.
pub enum WireEntry {
    /// `("WLM", enable)` — wire-log mode toggled (`enableWirelog`).
    WirelogMode(bool),
    /// `("RO", (len, hex))` — octets received off the socket.
    RxOctets { len: usize, data: String },
    /// `("TO", (len, hex), sync)` — octets written to the socket.
    TxOctets {
        len: usize,
        data: String,
        sync: bool,
    },
    /// `("RF", (len, ascii), opcode, fin, rsv, masked, maskhex)` — frame received.
    RxFrame {
        len: usize,
        data: String,
        opcode: u8,
        fin: bool,
        rsv: u8,
        masked: bool,
        mask: Option<String>,
    },
    /// `("TF", (len, ascii), opcode, fin, rsv, maskhex, repeatLength, chopsize, sync)`.
    TxFrame {
        len: usize,
        data: String,
        opcode: u8,
        fin: bool,
        rsv: u8,
        mask: Option<String>,
        repeat_length: Option<usize>,
        chopsize: Option<usize>,
        sync: bool,
    },
    /// `("CT", delay, tag)` — `continueLater` scheduled.
    ContinueScheduled { delay: f64, tag: Option<String> },
    /// `("CTE", tag)` — scheduled `continueLater` fired.
    ContinueExecuted { tag: Option<String> },
    /// `("KL", delay)` — `killAfter` scheduled.
    KillScheduled { delay: f64 },
    /// `("KLE",)` — scheduled `killAfter` fired.
    KillExecuted,
    /// `("TI", delay)` — `closeAfter` scheduled.
    CloseScheduled { delay: f64 },
    /// `("TIE",)` — scheduled `closeAfter` fired.
    CloseExecuted,
}

/// Build the Autobahn-shaped tuple for this entry (a JSON array once dumped), so
/// a `Vec<WireEntry>` converts straight to the list of log tuples.
impl<'py> IntoPyObject<'py> for &WireEntry {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        let k = Constants::get(py);
        match self {
            WireEntry::WirelogMode(enable) => (&k.wirelog_mode, enable).into_bound_py_any(py),
            WireEntry::RxOctets { len, data } => (&k.rx_octets, (len, data)).into_bound_py_any(py),
            WireEntry::TxOctets { len, data, sync } => {
                (&k.tx_octets, (len, data), sync).into_bound_py_any(py)
            }
            WireEntry::RxFrame {
                len,
                data,
                opcode,
                fin,
                rsv,
                masked,
                mask,
            } => (&k.rx_frame, (len, data), opcode, fin, rsv, masked, mask).into_bound_py_any(py),
            WireEntry::TxFrame {
                len,
                data,
                opcode,
                fin,
                rsv,
                mask,
                repeat_length,
                chopsize,
                sync,
            } => (
                &k.tx_frame,
                (len, data),
                opcode,
                fin,
                rsv,
                mask,
                repeat_length,
                chopsize,
                sync,
            )
                .into_bound_py_any(py),
            WireEntry::ContinueScheduled { delay, tag } => {
                (&k.continue_scheduled, delay, tag).into_bound_py_any(py)
            }
            WireEntry::ContinueExecuted { tag } => {
                (&k.continue_executed, tag).into_bound_py_any(py)
            }
            WireEntry::KillScheduled { delay } => (&k.kill_scheduled, delay).into_bound_py_any(py),
            WireEntry::KillExecuted => (&k.kill_executed,).into_bound_py_any(py),
            WireEntry::CloseScheduled { delay } => {
                (&k.close_scheduled, delay).into_bound_py_any(py)
            }
            WireEntry::CloseExecuted => (&k.close_executed,).into_bound_py_any(py),
        }
    }
}
