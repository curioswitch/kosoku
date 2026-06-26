"""Shim for autobahn.websocket.compress — permessage-deflate negotiation only.

The 12.x/13.x cases import these classes to express compression offers and
accept the server's response. They are implemented as native classes in the
``kosoku._kosoku`` extension (the driver constructs and reads them directly);
this shim re-exports them so the cases' import path resolves unchanged.
"""

from __future__ import annotations

from kosoku._kosoku import (
    PerMessageDeflateOffer,
    PerMessageDeflateOfferAccept,
    PerMessageDeflateResponse,
    PerMessageDeflateResponseAccept,
)

__all__ = [
    "PerMessageDeflateOffer",
    "PerMessageDeflateOfferAccept",
    "PerMessageDeflateResponse",
    "PerMessageDeflateResponseAccept",
]
