from collections.abc import Sequence
from typing import Any, Final, final

@final
class Behavior:
    """
    Verdict for a case's message exchange. `value` is the Autobahn label.
    """

    ERROR: Final[Behavior]
    """
    The case could not be run to a verdict (a connection failure or a crash).
    """
    FAILED: Final[Behavior]
    """
    Non-conforming: the peer violated a MUST-level requirement.
    """
    INFORMATIONAL: Final[Behavior]
    """
    Informational only: the case probes behavior the spec leaves unspecified.
    """
    NON_STRICT: Final[Behavior]
    """
    Acceptable, but diverges from a SHOULD-level requirement (still conformant).
    """
    OK: Final[Behavior]
    """
    The peer behaved exactly as the case requires.
    """
    UNIMPLEMENTED: Final[Behavior]
    """
    The peer does not implement the feature the case exercises.
    """
    def __eq__(self, /, other: object) -> bool: ...
    def __int__(self, /) -> int: ...
    def __ne__(self, /, other: object) -> bool: ...
    def __repr__(self, /) -> str: ...
    @property
    def value(self, /) -> str:
        """
        The verdict's string form, e.g. `"NON-STRICT"`.
        """

@final
class BehaviorClose:
    """
    Verdict for a case's closing handshake. `value` is the Autobahn label.
    """

    FAILED: Final[BehaviorClose]
    """
    The connection was failed by the wrong endpoint.
    """
    FAILED_BY_CLIENT: Final[BehaviorClose]
    """
    The client closed the TCP connection where the server should have.
    """
    INFORMATIONAL: Final[BehaviorClose]
    """
    Informational only: the closing behavior is left unspecified.
    """
    OK: Final[BehaviorClose]
    """
    The closing handshake completed as expected.
    """
    UNCLEAN: Final[BehaviorClose]
    """
    The connection was not closed cleanly where the spec requires it.
    """
    WRONG_CODE: Final[BehaviorClose]
    """
    The peer sent an unexpected close code.
    """
    def __eq__(self, /, other: object) -> bool: ...
    def __int__(self, /) -> int: ...
    def __ne__(self, /, other: object) -> bool: ...
    def __repr__(self, /) -> str: ...
    @property
    def value(self, /) -> str:
        """
        The verdict's string form, e.g. `"WRONG CODE"`.
        """

@final
class CaseResult:
    """
    The result of one Autobahn test case.
    """
    def __repr__(self, /) -> str: ...
    @property
    def agent(self, /) -> str:
        """
        Name of the peer under test — from its `Server` header (client mode) or
        the `agent` it reported (server mode).
        """
    @property
    def behavior(self, /) -> Behavior:
        """
        Verdict for the message exchange.
        """
    @property
    def behavior_close(self, /) -> BehaviorClose:
        """
        Verdict for the closing handshake.
        """
    @property
    def case_id(self, /) -> str:
        """
        Case id, e.g. `"1.1.1"`.
        """
    @property
    def case_index(self, /) -> int:
        """
        1-based position of this case in the run.
        """
    @property
    def closed_by_me(self, /) -> bool:
        """
        Whether kosoku sent the close frame first.
        """
    @property
    def create_stats(self, /) -> bool:
        """
        Whether octet and frame statistics were collected.
        """
    @property
    def create_wirelog(self, /) -> bool:
        """
        Whether the wire log was captured (disabled for very large payloads).
        """
    @property
    def description(self, /) -> str:
        """
        The case's `DESCRIPTION` text.
        """
    @property
    def dropped_by_me(self, /) -> bool:
        """
        Whether kosoku dropped the TCP connection.
        """
    @property
    def duration(self, /) -> int:
        """
        Case run time, in milliseconds.
        """
    @property
    def expectation(self, /) -> str:
        """
        The case's `EXPECTATION` text — what a conformant peer should do.
        """
    @property
    def expected(self, /) -> Any:
        """
        Accepted event sequences, keyed by the verdict each would yield.
        """
    @property
    def expected_close(self, /) -> Any:
        """
        Expected close parameters: who closes, whether a clean close is required,
        and the acceptable close codes.
        """
    @property
    def failed_by_me(self, /) -> bool:
        """
        Whether kosoku failed the connection on a protocol error (by sending a
        close or dropping the TCP).
        """
    @property
    def http_request(self, /) -> str:
        """
        The opening-handshake request, verbatim.
        """
    @property
    def http_response(self, /) -> str:
        """
        The opening-handshake response, verbatim.
        """
    @property
    def is_server(self, /) -> bool:
        """
        Whether kosoku acted as the server (the peer under test is the client).
        """
    @property
    def local_close_code(self, /) -> int | None:
        """
        The close code kosoku sent, if any.
        """
    @property
    def local_close_reason(self, /) -> str | None:
        """
        The close reason kosoku sent, if any.
        """
    @property
    def passed(self, /) -> bool:
        """
        Whether the case passed — both `behavior` and `behavior_close` are
        acceptable (`OK`, `NON-STRICT`, or `INFORMATIONAL`).
        """
    @property
    def received(self, /) -> Any:
        """
        The events actually observed: messages, pings, and pongs.
        """
    @property
    def remote_close_code(self, /) -> int | None:
        """
        The close code the peer sent, if any.
        """
    @property
    def remote_close_reason(self, /) -> str | None:
        """
        The close reason the peer sent, if any.
        """
    @property
    def report_compression_ratio(self, /) -> bool:
        """
        Whether the report shows the compression ratio (set by the 12.x cases).
        """
    @property
    def report_time(self, /) -> bool:
        """
        Whether the report shows `duration` (set by the timing cases, 9.x/12.x).
        """
    @property
    def result(self, /) -> str:
        """
        Prose explaining `behavior`, e.g. "Actual events match at least one
        expected." or "Actual events differ from any expected."
        """
    @property
    def result_close(self, /) -> str:
        """
        Prose explaining `behavior_close`, e.g. "Connection was properly closed"
        or "The close code should have been ...".
        """
    @property
    def rx_frame_stats(self, /) -> dict:
        """
        Count of received frames by opcode.
        """
    @property
    def rx_octet_stats(self, /) -> dict:
        """
        Count of socket reads by size.
        """
    @property
    def started(self, /) -> str:
        """
        ISO-8601 UTC timestamp of when the case started.
        """
    @property
    def traffic_stats(self, /) -> TrafficStats | None:
        """
        Traffic and compression counters for the case.
        """
    @property
    def tx_frame_stats(self, /) -> dict:
        """
        Count of sent frames by opcode.
        """
    @property
    def tx_octet_stats(self, /) -> dict:
        """
        Count of socket writes by size.
        """
    @property
    def was_clean(self, /) -> bool:
        """
        Whether the closing handshake completed cleanly — close sent and
        received, then the responsible side dropped the connection.
        """
    @property
    def was_close_handshake_timeout(self, /) -> bool:
        """
        Whether the closing handshake timed out.
        """
    @property
    def was_not_clean_reason(self, /) -> str | None:
        """
        Why the close was not clean, when `was_clean` is false.
        """
    @property
    def was_open_handshake_timeout(self, /) -> bool:
        """
        Whether the opening handshake timed out.
        """
    @property
    def was_server_connection_drop_timeout(self, /) -> bool:
        """
        Whether kosoku expected the server to drop the connection but it did not
        in time (client mode).
        """
    @property
    def wirelog(self, /) -> list:
        """
        Ordered wire-log trace of octets, frames, and timer events.
        """

@final
class FuzzingServer:
    """
    A server that runs the Autobahn cases for a WebSocket client under test.

    Created by `run_fuzzingserver` and used as an async context manager:
    entering starts serving, `address`/`port` give the bound endpoint, and
    `get_result()` returns the per-case results once the client has finished.
    """
    async def __aenter__(self, /) -> FuzzingServer:
        """
        Start serving and return the server, now bound to `address` and `port`.
        """
    async def __aexit__(
        self,
        /,
        _exc_type: Any | None = None,
        _exc_value: Any | None = None,
        _traceback: Any | None = None,
    ) -> bool:
        """
        Stop the server, closing it if the run has not already finished.
        """
    @property
    def address(self, /) -> str:
        """
        The bound IP address. Available once serving has started.
        """
    async def get_result(self, /) -> "list[CaseResult]":
        """
        Wait for the run to finish and return the per-case results.

        The client under test ends the run once it has driven every case.

        Returns:
            The result of each case, in case order.

        Raises:
            FailureError: If any case did not pass. Its `results` attribute holds
                the same results that would otherwise be returned.
        """
    @property
    def port(self, /) -> int:
        """
        The bound TCP port — the real port chosen by the OS when `0` was
        requested. Available once serving has started.
        """

@final
class PerMessageDeflateOffer:
    """
    A client's permessage-deflate offer — the compression parameters the client
    proposes to the server.
    """
    def __new__(
        cls,
        /,
        acceptNoContextTakeover: bool = True,
        acceptMaxWindowBits: bool = True,
        requestNoContextTakeover: bool = False,
        requestMaxWindowBits: int = 0,
    ) -> PerMessageDeflateOffer: ...
    @property
    def acceptMaxWindowBits(self, /) -> bool: ...
    @property
    def acceptNoContextTakeover(self, /) -> bool: ...
    @property
    def requestMaxWindowBits(self, /) -> int: ...
    @property
    def requestNoContextTakeover(self, /) -> bool: ...

@final
class PerMessageDeflateOfferAccept:
    """
    A server's acceptance of a client's permessage-deflate offer, with the
    compression parameters it settles on.
    """
    def __new__(
        cls,
        /,
        offer: PerMessageDeflateOffer,
        requestNoContextTakeover: bool = False,
        requestMaxWindowBits: int = 0,
        noContextTakeover: bool | None = None,
        windowBits: int | None = None,
        memLevel: int | None = None,
    ) -> PerMessageDeflateOfferAccept: ...
    @property
    def memLevel(self, /) -> int | None: ...
    @property
    def noContextTakeover(self, /) -> bool | None: ...
    @property
    def offer(self, /) -> PerMessageDeflateOffer: ...
    @property
    def requestMaxWindowBits(self, /) -> int: ...
    @property
    def requestNoContextTakeover(self, /) -> bool: ...
    @property
    def windowBits(self, /) -> int | None: ...

@final
class PerMessageDeflateResponse:
    """
    The permessage-deflate parameters the server returned in its handshake
    response.
    """
    def __new__(
        cls,
        /,
        client_max_window_bits: int = 0,
        client_no_context_takeover: bool = False,
        server_max_window_bits: int = 0,
        server_no_context_takeover: bool = False,
    ) -> PerMessageDeflateResponse: ...
    @property
    def client_max_window_bits(self, /) -> int: ...
    @property
    def client_no_context_takeover(self, /) -> bool: ...
    @property
    def server_max_window_bits(self, /) -> int: ...
    @property
    def server_no_context_takeover(self, /) -> bool: ...

@final
class PerMessageDeflateResponseAccept:
    """
    The client's acceptance of the server's permessage-deflate response,
    optionally overriding `noContextTakeover` / `windowBits`.
    """
    def __new__(
        cls,
        /,
        response: PerMessageDeflateResponse,
        noContextTakeover: bool | None = None,
        windowBits: int | None = None,
        memLevel: int | None = None,
    ) -> PerMessageDeflateResponseAccept: ...
    @property
    def memLevel(self, /) -> int | None: ...
    @property
    def noContextTakeover(self, /) -> bool | None: ...
    @property
    def response(self, /) -> PerMessageDeflateResponse: ...
    @property
    def windowBits(self, /) -> int | None: ...

@final
class TrafficStats:
    """
    Per-case octet, frame, and message counters for sent and received traffic.
    """
    def __deepcopy__(self, /, _memo: Any) -> TrafficStats: ...
    def __json__(self, /) -> dict:
        """
        The counters as a dict, with derived compression ratios
        (compressed/uncompressed payload) and protocol overhead (framing vs.
        payload), for the report's `trafficStats`.
        """
    def __new__(cls, /) -> TrafficStats: ...

@final
class Utf8Validator:
    """
    Incremental UTF-8 validator: feed chunks to `validate()` and it tracks
    validity across calls until `reset()`.
    """
    def __new__(cls, /) -> Utf8Validator: ...
    def reset(self, /) -> None:
        """
        Discard carried-over state and start validating a fresh stream.
        """
    def validate(self, /, data: "bytes | str") -> tuple[bool, bool, int, int]:
        """
        Validate the next chunk of bytes, continuing from previous calls.

        Args:
            data: The bytes to validate (a `bytes`, or a `str` of code points
                0–255).

        Returns:
            A tuple `(valid, ends_on_codepoint, consumed, total)`: whether the
            stream is still valid UTF-8, whether it ends on a complete code
            point, the bytes consumed from this chunk, and the bytes consumed
            since the last `reset()`.
        """

async def run_fuzzingclient(
    url: str,
    cases: Sequence[str] | None = None,
    exclude_cases: Sequence[str] | None = None,
    concurrency: int = 1,
) -> list[CaseResult]:
    """
    Run the Autobahn test cases against a WebSocket server.

    Opens one connection per case to the server under test and runs the selected
    cases, returning their results in case order.

    Args:
        url: WebSocket URL of the server under test, e.g. `ws://localhost:9001`.
        cases: Case ids or `*` globs to run, e.g. `["1.*", "9.7.1"]`. The whole
            suite runs when omitted.
        exclude_cases: Case ids or `*` globs to remove from the selection.
        concurrency: How many cases to run in parallel.

    Returns:
        The result of each case, in case order.

    Raises:
        FailureError: If any case did not pass. Its `results` attribute holds the
            same results that would otherwise be returned.
    """

def run_fuzzingserver(
    cases: Sequence[str] | None = None,
    exclude_cases: Sequence[str] | None = None,
    *,
    host: str | None = None,
    port: int = 9001,
) -> FuzzingServer:
    """
    Serve the Autobahn test cases to a WebSocket client under test.

    Returns a server to use as an async context manager: entering it starts
    accepting connections, `address` and `port` give the endpoint to point the
    client at, and `get_result()` yields the outcome once the client has run
    every case.

    Example:
        async with run_fuzzingserver(port=9001) as server:
            results = await server.get_result()

    Args:
        cases: Case ids or `*` globs to serve, e.g. `["1.*", "9.7.1"]`. The whole
            suite is served when omitted.
        exclude_cases: Case ids or `*` globs to remove from the selection.
        host: Address to bind. Defaults to `127.0.0.1`.
        port: Port to bind. Pass `0` for an ephemeral port and read the chosen
            one back from `port`.

    Returns:
        A `FuzzingServer` to use as an `async with` context manager.
    """
