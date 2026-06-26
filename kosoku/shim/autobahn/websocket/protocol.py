"""Minimal twisted-free shim for autobahn.websocket.protocol.

The bundled Autobahn case files import ``WebSocketProtocol`` only for class-level
constants (verified: STATE_OPEN, MESSAGE_TYPE_TEXT, CLOSE_STATUS_CODE_*). The
real class drags in twisted; this shim provides just the constants. The Rust
protocol object that backs ``self.p`` reports the same numeric values.

``TrafficStats`` is implemented natively in the ``kosoku._kosoku`` extension
(the driver tallies the counters directly); it is re-exported here so the cases'
``self.p.trafficStats`` snapshots resolve unchanged.
"""

from kosoku._kosoku import TrafficStats

__all__ = ["WebSocketProtocol", "TrafficStats"]


class WebSocketProtocol(object):
    # Connection states (must match the values reported by the Rust protocol).
    STATE_CLOSED = 0
    STATE_CONNECTING = 1
    STATE_CLOSING = 2
    STATE_OPEN = 3
    STATE_PROXY_CONNECTING = 4

    # Message types.
    MESSAGE_TYPE_TEXT = 1
    MESSAGE_TYPE_BINARY = 2

    # Close status codes (RFC 6455).
    CLOSE_STATUS_CODE_NORMAL = 1000
    CLOSE_STATUS_CODE_GOING_AWAY = 1001
    CLOSE_STATUS_CODE_PROTOCOL_ERROR = 1002
    CLOSE_STATUS_CODE_UNSUPPORTED_DATA = 1003
    CLOSE_STATUS_CODE_RESERVED1 = 1004
    CLOSE_STATUS_CODE_NULL = 1005
    CLOSE_STATUS_CODE_ABNORMAL_CLOSE = 1006
    CLOSE_STATUS_CODE_INVALID_PAYLOAD = 1007
    CLOSE_STATUS_CODE_POLICY_VIOLATION = 1008
    CLOSE_STATUS_CODE_MESSAGE_TOO_BIG = 1009
    CLOSE_STATUS_CODE_MANDATORY_EXTENSION = 1010
    CLOSE_STATUS_CODE_INTERNAL_ERROR = 1011
    CLOSE_STATUS_CODE_TLS_HANDSHAKE_FAILED = 1015
