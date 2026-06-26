//! Incremental UTF-8 validator exposed to the Autobahn cases as
//! `autobahn.websocket.utf8validator.Utf8Validator`.
//!
//! Reimplements [`Utf8Validator`](https://github.com/crossbario/autobahn-python/blob/v0.10.9/autobahn/websocket/utf8validator.py).

use bytes::{Bytes, BytesMut};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAnyMethods;

/// Incremental UTF-8 validator: feed chunks to `validate()` and it tracks
/// validity across calls until `reset()`.
#[pyclass]
pub struct Utf8Validator {
    /// Trailing bytes of an incomplete (but so-far valid) multi-byte sequence
    /// carried over from a previous `validate()` chunk.
    pending: BytesMut,
    /// Once a sequence is rejected the validator stays rejected (sticky).
    rejected: bool,
    /// Total octets consumed across all `validate()` calls since `reset()`.
    total: usize,
}

/// Coerce a `validate()` argument to the bytes it represents: real `bytes` pass
/// through; a `str` (the cases author byte vectors as py2 str literals) maps
/// codepoint→byte (latin-1), matching the rest of the str/bytes bridge.
fn to_bytes(data: &Bound<'_, PyAny>) -> PyResult<Bytes> {
    if let Ok(b) = data.extract::<Bytes>() {
        return Ok(b);
    }
    let s: String = data.extract()?;
    if s.is_ascii() {
        return Ok(Bytes::from(s.into_bytes()));
    }
    s.chars()
        .map(|c| {
            let cp = c as u32;
            u8::try_from(cp).map_err(|_| {
                PyValueError::new_err(format!("codepoint U+{cp:04X} is not a single byte"))
            })
        })
        .collect()
}

#[pymethods]
impl Utf8Validator {
    #[new]
    fn new() -> Self {
        Utf8Validator {
            pending: BytesMut::new(),
            rejected: false,
            total: 0,
        }
    }

    /// Discard carried-over state and start validating a fresh stream.
    fn reset(&mut self) {
        self.pending.clear();
        self.rejected = false;
        self.total = 0;
    }

    /// Validate the next chunk of bytes, continuing from previous calls.
    ///
    /// Args:
    ///     data: The bytes to validate (a `bytes`, or a `str` of code points
    ///         0–255).
    ///
    /// Returns:
    ///     A tuple `(valid, ends_on_codepoint, consumed, total)`: whether the
    ///     stream is still valid UTF-8, whether it ends on a complete code
    ///     point, the bytes consumed from this chunk, and the bytes consumed
    ///     since the last `reset()`.
    #[pyo3(signature = (data: "bytes | str"))]
    fn validate(&mut self, data: &Bound<'_, PyAny>) -> PyResult<(bool, bool, usize, usize)> {
        Ok(self.step(to_bytes(data)?))
    }
}

impl Utf8Validator {
    /// Core incremental validation over raw bytes, separated from the Python
    /// argument coercion above.
    fn step(&mut self, chunk: Bytes) -> (bool, bool, usize, usize) {
        if self.rejected {
            return (false, false, 0, self.total);
        }
        let chunk_len = chunk.len();
        let carried = self.pending.len();

        if self.pending.is_empty() {
            self.pending = BytesMut::from(chunk);
        } else {
            self.pending.extend_from_slice(&chunk);
        }

        match std::str::from_utf8(&self.pending) {
            Ok(_) => {
                self.pending.clear();
                self.total += chunk_len;
                (true, true, chunk_len, self.total)
            }
            Err(e) if e.error_len().is_none() => {
                // Incomplete tail: valid so far, just not on a code-point boundary.
                let valid = e.valid_up_to();
                let n = self.pending.len() - valid;
                debug_assert!(n <= 3, "incomplete UTF-8 tail is at most 3 bytes");
                let mut carry = [0u8; 3];
                carry[..n].copy_from_slice(&self.pending[valid..]);
                self.pending.clear();
                self.pending.extend_from_slice(&carry[..n]);
                self.total += chunk_len;
                (true, false, chunk_len, self.total)
            }
            // Hard error: reject (sticky) at the failing octet.
            Err(e) => {
                self.rejected = true;
                let at_in_chunk = e.valid_up_to().saturating_sub(carried);
                self.total += at_in_chunk;
                (false, false, at_in_chunk, self.total)
            }
        }
    }
}
