//! Classes for negotiating Websocket compression. The test case is provided the
//! parameters from the server or client and can accept it, possibly with different
//! parameters. While no real logic, keeping them in Rust makes it easier to read out
//! when actually doing the compression.

use pyo3::prelude::*;

/// A client's permessage-deflate offer — the compression parameters the client
/// proposes to the server.
#[pyclass(frozen)]
pub(crate) struct PerMessageDeflateOffer {
    #[pyo3(get, name = "acceptNoContextTakeover")]
    pub(crate) accept_no_context_takeover: bool,
    #[pyo3(get, name = "acceptMaxWindowBits")]
    pub(crate) accept_max_window_bits: bool,
    #[pyo3(get, name = "requestNoContextTakeover")]
    pub(crate) request_no_context_takeover: bool,
    #[pyo3(get, name = "requestMaxWindowBits")]
    pub(crate) request_max_window_bits: u8,
}

#[pymethods]
#[allow(non_snake_case)]
impl PerMessageDeflateOffer {
    #[new]
    #[pyo3(signature = (
        acceptNoContextTakeover = true,
        acceptMaxWindowBits = true,
        requestNoContextTakeover = false,
        requestMaxWindowBits = 0,
    ))]
    fn new(
        acceptNoContextTakeover: bool,
        acceptMaxWindowBits: bool,
        requestNoContextTakeover: bool,
        requestMaxWindowBits: u8,
    ) -> Self {
        Self {
            accept_no_context_takeover: acceptNoContextTakeover,
            accept_max_window_bits: acceptMaxWindowBits,
            request_no_context_takeover: requestNoContextTakeover,
            request_max_window_bits: requestMaxWindowBits,
        }
    }
}

/// A server's acceptance of a client's permessage-deflate offer, with the
/// compression parameters it settles on.
#[pyclass(frozen)]
pub(crate) struct PerMessageDeflateOfferAccept {
    #[pyo3(get)]
    pub(crate) offer: Py<PerMessageDeflateOffer>,
    #[pyo3(get, name = "requestNoContextTakeover")]
    pub(crate) request_no_context_takeover: bool,
    #[pyo3(get, name = "requestMaxWindowBits")]
    pub(crate) request_max_window_bits: u8,
    #[pyo3(get, name = "noContextTakeover")]
    pub(crate) no_context_takeover: Option<bool>,
    #[pyo3(get, name = "windowBits")]
    pub(crate) window_bits: Option<u8>,
    #[pyo3(get, name = "memLevel")]
    pub(crate) mem_level: Option<u8>,
}

#[pymethods]
#[allow(non_snake_case)]
impl PerMessageDeflateOfferAccept {
    #[new]
    #[pyo3(signature = (
        offer,
        requestNoContextTakeover = false,
        requestMaxWindowBits = 0,
        noContextTakeover = None,
        windowBits = None,
        memLevel = None,
    ))]
    fn new(
        offer: Py<PerMessageDeflateOffer>,
        requestNoContextTakeover: bool,
        requestMaxWindowBits: u8,
        noContextTakeover: Option<bool>,
        windowBits: Option<u8>,
        memLevel: Option<u8>,
    ) -> Self {
        Self {
            offer,
            request_no_context_takeover: requestNoContextTakeover,
            request_max_window_bits: requestMaxWindowBits,
            no_context_takeover: noContextTakeover,
            window_bits: windowBits,
            mem_level: memLevel,
        }
    }
}

/// The permessage-deflate parameters the server returned in its handshake
/// response.
#[pyclass(frozen, get_all)]
pub(crate) struct PerMessageDeflateResponse {
    pub(crate) client_max_window_bits: u8,
    pub(crate) client_no_context_takeover: bool,
    pub(crate) server_max_window_bits: u8,
    pub(crate) server_no_context_takeover: bool,
}

#[pymethods]
impl PerMessageDeflateResponse {
    #[new]
    #[pyo3(signature = (
        client_max_window_bits = 0,
        client_no_context_takeover = false,
        server_max_window_bits = 0,
        server_no_context_takeover = false,
    ))]
    fn new(
        client_max_window_bits: u8,
        client_no_context_takeover: bool,
        server_max_window_bits: u8,
        server_no_context_takeover: bool,
    ) -> Self {
        Self {
            client_max_window_bits,
            client_no_context_takeover,
            server_max_window_bits,
            server_no_context_takeover,
        }
    }
}

/// The client's acceptance of the server's permessage-deflate response,
/// optionally overriding `noContextTakeover` / `windowBits`.
#[pyclass(frozen)]
pub(crate) struct PerMessageDeflateResponseAccept {
    #[pyo3(get)]
    pub(crate) response: Py<PerMessageDeflateResponse>,
    #[pyo3(get, name = "noContextTakeover")]
    pub(crate) no_context_takeover: Option<bool>,
    #[pyo3(get, name = "windowBits")]
    pub(crate) window_bits: Option<u8>,
    #[pyo3(get, name = "memLevel")]
    pub(crate) mem_level: Option<u8>,
}

#[pymethods]
#[allow(non_snake_case)]
impl PerMessageDeflateResponseAccept {
    #[new]
    #[pyo3(signature = (response, noContextTakeover = None, windowBits = None, memLevel = None))]
    fn new(
        response: Py<PerMessageDeflateResponse>,
        noContextTakeover: Option<bool>,
        windowBits: Option<u8>,
        memLevel: Option<u8>,
    ) -> Self {
        Self {
            response,
            no_context_takeover: noContextTakeover,
            window_bits: windowBits,
            mem_level: memLevel,
        }
    }
}
