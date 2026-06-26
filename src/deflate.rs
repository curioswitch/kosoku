//! permessage-deflate (RFC 7692) compression.
//!
//! Each message is compressed with raw DEFLATE and a `Z_SYNC_FLUSH`, then the
//! trailing empty-block marker `00 00 FF FF` is stripped (and re-appended before
//! inflate). `*_no_context_takeover` resets the relevant stream per message;
//! `*_max_window_bits` sizes its LZ77 window.
//!
//! Roughly reimplements [`PerMessageDeflate`](https://github.com/crossbario/autobahn-python/blob/v0.10.9/autobahn/websocket/compress_deflate.py#L526),
//! though with a single configured compress method instead of separating
//! start/end of compression.

use bytes::{Buf, Bytes, BytesMut};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};

const TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// A negotiated permessage-deflate session.
pub struct Deflate {
    out_no_context_takeover: bool,
    in_no_context_takeover: bool,
    compressor: Compress,
    decompressor: Decompress,
}

fn window_bits(negotiated: u8) -> u8 {
    // 0 means "unspecified" → the DEFLATE maximum; zlib's raw mode floors at 9.
    match negotiated {
        0 => 15,
        n => n.clamp(9, 15),
    }
}

impl Deflate {
    pub fn new(
        out_no_context_takeover: bool,
        out_max_window_bits: u8,
        in_no_context_takeover: bool,
        in_max_window_bits: u8,
    ) -> Self {
        Deflate {
            out_no_context_takeover,
            in_no_context_takeover,
            compressor: Compress::new_with_window_bits(
                Compression::default(),
                false,
                window_bits(out_max_window_bits),
            ),
            decompressor: Decompress::new_with_window_bits(false, window_bits(in_max_window_bits)),
        }
    }

    /// Compress one message's payload.
    pub fn compress(&mut self, mut data: Bytes) -> Bytes {
        if self.out_no_context_takeover {
            self.compressor.reset();
        }
        let mut out = BytesMut::with_capacity(data.len() / 2 + 16);
        let mut buf = [0u8; 8192];
        loop {
            let in0 = self.compressor.total_in();
            let out0 = self.compressor.total_out();
            self.compressor
                .compress(&data, &mut buf, FlushCompress::Sync)
                .expect("deflate compress");
            let consumed = usize::try_from(self.compressor.total_in() - in0)
                .expect("consumed length fits usize");
            let produced = usize::try_from(self.compressor.total_out() - out0)
                .expect("produced length fits usize");
            out.extend_from_slice(&buf[..produced]);
            data.advance(consumed);
            // Flush complete once input is drained and the buffer wasn't filled.
            if data.is_empty() && produced < buf.len() {
                break;
            }
        }
        if out.ends_with(&TAIL) {
            out.truncate(out.len() - TAIL.len());
        }
        out.freeze()
    }

    /// Decompress one received message's payload.
    pub fn decompress(&mut self, data: Bytes) -> std::io::Result<Bytes> {
        if self.in_no_context_takeover {
            self.decompressor.reset(false);
        }
        let len = data.len();
        let mut input = BytesMut::from(data);
        input.extend_from_slice(&TAIL);
        let mut out = BytesMut::with_capacity(len * 2 + 16);
        let mut buf = [0u8; 8192];
        let mut inp = &input[..];
        loop {
            let in0 = self.decompressor.total_in();
            let out0 = self.decompressor.total_out();
            self.decompressor
                .decompress(inp, &mut buf, FlushDecompress::Sync)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            let consumed = usize::try_from(self.decompressor.total_in() - in0)
                .expect("consumed length fits usize");
            let produced = usize::try_from(self.decompressor.total_out() - out0)
                .expect("produced length fits usize");
            out.extend_from_slice(&buf[..produced]);
            inp = &inp[consumed..];
            if inp.is_empty() && produced < buf.len() {
                break;
            }
        }
        Ok(out.freeze())
    }
}

/// The negotiated permessage-deflate parameters from the server's
/// `Sec-WebSocket-Extensions` response.
pub struct ResponseParams {
    /// Whether the client must reset its compressor between messages.
    pub client_no_context_takeover: bool,
    /// The client's compressor window size in bits (0 = unspecified).
    pub client_max_window_bits: u8,
    /// Whether the server must reset its compressor between messages.
    pub server_no_context_takeover: bool,
    /// The server's compressor window size in bits (0 = unspecified).
    pub server_max_window_bits: u8,
}

/// Parse the server's `Sec-WebSocket-Extensions` response value, or `None` if
/// deflate wasn't negotiated.
pub fn parse_response(header: &str) -> Option<ResponseParams> {
    // Take the first extension token; we only negotiate permessage-deflate.
    let ext = header.split(',').next()?.trim();
    let mut parts = ext.split(';').map(str::trim);
    if parts.next()? != "permessage-deflate" {
        return None;
    }
    let mut params = ResponseParams {
        client_no_context_takeover: false,
        client_max_window_bits: 0,
        server_no_context_takeover: false,
        server_max_window_bits: 0,
    };
    for param in parts {
        let (key, value) = match param.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => (param, ""),
        };
        match key {
            "client_no_context_takeover" => params.client_no_context_takeover = true,
            "server_no_context_takeover" => params.server_no_context_takeover = true,
            "client_max_window_bits" => params.client_max_window_bits = value.parse().unwrap_or(0),
            "server_max_window_bits" => params.server_max_window_bits = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    Some(params)
}

/// Parameters of one client `permessage-deflate` offer, as the case's
/// `PerMessageDeflateOffer` exposes them: `accept_*` = what the client offers
/// about its own (client→server) compression; `request_*` = what it asks of the
/// server.
pub struct OfferParams {
    pub accept_no_context_takeover: bool,
    pub accept_max_window_bits: bool,
    pub request_no_context_takeover: bool,
    pub request_max_window_bits: u8,
}

/// Parse a client's `Sec-WebSocket-Extensions` request value into its
/// permessage-deflate offers.
pub fn parse_offers(header: &str) -> Vec<OfferParams> {
    let mut offers = Vec::new();
    for ext in header.split(',') {
        let mut parts = ext.split(';').map(str::trim);
        if parts.next() != Some("permessage-deflate") {
            continue;
        }
        let mut offer = OfferParams {
            accept_no_context_takeover: false,
            accept_max_window_bits: false,
            request_no_context_takeover: false,
            request_max_window_bits: 0,
        };
        for param in parts {
            let (key, value) = match param.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
                None => (param, ""),
            };
            match key {
                "client_no_context_takeover" => offer.accept_no_context_takeover = true,
                "client_max_window_bits" => offer.accept_max_window_bits = true,
                "server_no_context_takeover" => offer.request_no_context_takeover = true,
                "server_max_window_bits" => {
                    offer.request_max_window_bits = value.parse().unwrap_or(0);
                }
                _ => {}
            }
        }
        offers.push(offer);
    }
    offers
}
