"""Write Autobahn-format reports from a run's results.

Upstream: autobahn-testsuite's ``report.py`` and ``FuzzingFactory.createReports``:
https://github.com/crossbario/autobahn-testsuite/blob/v25.10.1/autobahntestsuite/autobahntestsuite/report.py
"""

from __future__ import annotations

import binascii
import datetime
import json
import os
import textwrap
from collections.abc import Iterable, Sequence
from typing import Any

from kosoku._kosoku import CaseResult
from kosoku._report_assets import (
    CSS_COMMON,
    CSS_DETAIL_REPORT,
    CSS_MASTER_REPORT,
    JS_MASTER_REPORT,
)

# Characters allowed in a report filename — copied verbatim from Autobahn's
# `cleanForFilename`.
_FILENAME_ALLOWED = "abcdefghjiklmnopqrstuvwxyz0123456789"

_JSON_KW: dict[str, Any] = {"sort_keys": True, "indent": 3, "separators": (",", ": ")}


def _clean_for_filename(name: str) -> str:
    cleaned = "".join(
        c if c in _FILENAME_ALLOWED else " " for c in name.strip().lower()
    )
    return cleaned.strip().replace(" ", "_")


def _report_filename(agent: str, case_id: str, ext: str) -> str:
    return f"{_clean_for_filename(agent)}_case_{case_id.replace('.', '_')}.{ext}"


def _bin_log_data(data: bytes, maxlen: int = 64) -> str:
    ellipses = " ..."
    if len(data) > maxlen - len(ellipses):
        return binascii.b2a_hex(data[:maxlen]).decode() + ellipses
    return binascii.b2a_hex(data).decode()


def _ascii_log_data(data: str | bytes, maxlen: int = 64, replace: bool = False) -> str:
    """Render a message/ping/pong payload for the report, as Autobahn does.

    Payloads reach us as ``str`` (text, latin-1 binary) or ``bytes`` (compressed
    messages). Truncate to ``maxlen``; decode bytes as UTF-8, falling back to a
    ``0x``-prefixed hex dump when they aren't valid text.
    """
    ellipses = " ..."
    if isinstance(data, str):
        return data[:maxlen] + ellipses if len(data) > maxlen - len(ellipses) else data
    chunk = data[:maxlen]
    if len(data) > maxlen - len(ellipses):
        chunk = chunk + ellipses.encode()
    try:
        return chunk.decode("utf8", "replace" if replace else "strict")
    except Exception:
        return "0x" + _bin_log_data(data, maxlen)


def _clean_bin(events: Iterable[Sequence[Any]]) -> list[tuple[Any, ...]]:
    cleaned: list[tuple[Any, ...]] = []
    for event in events:
        kind = event[0]
        if kind == "message":
            cleaned.append((kind, _ascii_log_data(event[1]), event[2]))
        elif kind in ("ping", "pong"):
            cleaned.append((kind, _ascii_log_data(event[1])))
        else:  # "timeout" and any other markers pass through unchanged
            cleaned.append(tuple(event))
    return cleaned


def case_to_dict(result: CaseResult) -> dict[str, Any]:
    """Translate a `CaseResult` to Autobahn's per-case report dict."""
    expected = {k: _clean_bin(v) for k, v in (result.expected or {}).items()}
    traffic = result.traffic_stats
    return {
        "case": result.case_index,
        "id": result.case_id,
        "description": result.description,
        "expectation": result.expectation,
        "agent": result.agent,
        "started": result.started,
        "duration": result.duration,
        "reportTime": result.report_time,
        "reportCompressionRatio": result.report_compression_ratio,
        "behavior": result.behavior.value,
        "behaviorClose": result.behavior_close.value,
        "expected": expected,
        "expectedClose": result.expected_close or {},
        "received": _clean_bin(result.received or []),
        "result": result.result,
        "resultClose": result.result_close,
        "wirelog": result.wirelog,
        "createWirelog": result.create_wirelog,
        "closedByMe": result.closed_by_me,
        "failedByMe": result.failed_by_me,
        "droppedByMe": result.dropped_by_me,
        "wasClean": result.was_clean,
        "wasNotCleanReason": result.was_not_clean_reason,
        "wasServerConnectionDropTimeout": result.was_server_connection_drop_timeout,
        "wasOpenHandshakeTimeout": result.was_open_handshake_timeout,
        "wasCloseHandshakeTimeout": result.was_close_handshake_timeout,
        "localCloseCode": result.local_close_code,
        "localCloseReason": result.local_close_reason,
        "remoteCloseCode": result.remote_close_code,
        "remoteCloseReason": result.remote_close_reason,
        "isServer": result.is_server,
        "createStats": result.create_stats,
        "rxOctetStats": result.rx_octet_stats,
        "rxFrameStats": result.rx_frame_stats,
        "txOctetStats": result.tx_octet_stats,
        "txFrameStats": result.tx_frame_stats,
        "httpRequest": result.http_request,
        "httpResponse": result.http_response,
        "trafficStats": traffic.__json__() if traffic is not None else None,
    }


def _index_entry(result: CaseResult) -> dict[str, Any]:
    return {
        "behavior": result.behavior.value,
        "behaviorClose": result.behavior_close.value,
        "remoteCloseCode": result.remote_close_code,
        "duration": result.duration,
        "reportfile": _report_filename(result.agent, result.case_id, "json"),
    }


def write_reports(
    results: Iterable[CaseResult], outdir: str, produce_html: bool = True
) -> None:
    """Write the report files into ``outdir``."""
    results = list(results)
    os.makedirs(outdir, exist_ok=True)

    index: dict[str, dict[str, Any]] = {}
    cases: dict[tuple[str, str], dict[str, Any]] = {}
    for result in results:
        case = case_to_dict(result)
        cases[(result.agent, result.case_id)] = case
        with open(
            os.path.join(
                outdir, _report_filename(result.agent, result.case_id, "json")
            ),
            "w",
        ) as f:
            json.dump(case, f, **_JSON_KW)
        index.setdefault(result.agent, {})[result.case_id] = _index_entry(result)
        if produce_html:
            with open(
                os.path.join(
                    outdir, _report_filename(result.agent, result.case_id, "html")
                ),
                "w",
            ) as f:
                f.write(_detail_html(case))

    with open(os.path.join(outdir, "index.json"), "w") as f:
        json.dump(index, f, **_JSON_KW)
    if produce_html:
        with open(os.path.join(outdir, "index.html"), "w") as f:
            f.write(_master_html(index, cases))


_OK, _NON_STRICT, _INFORMATIONAL, _UNIMPLEMENTED = (
    "OK",
    "NON-STRICT",
    "INFORMATIONAL",
    "UNIMPLEMENTED",
)
_NO_CLOSE, _FAILED_BY_CLIENT, _WRONG_CODE, _UNCLEAN = (
    "NO_CLOSE",
    "FAILED BY CLIENT",
    "WRONG CODE",
    "UNCLEAN",
)

MAX_CASE_PICKLE_LEN = 1000

_LOGOS = (
    '      <center><a href="https://autobahn-testsuite.readthedocs.io/" title="Autobahn WebSocket Testsuite">'
    '<img src="https://autobahn-testsuite.readthedocs.io/en/latest/_static/img/ws_protocol_test_report.png" '
    'border="0" width="820" height="46" alt="Autobahn WebSocket Testsuite Report"></img></a></center>\n'
    '      <center><a href="https://autobahn-testsuite.readthedocs.io/" title="Autobahn WebSocket">'
    '<img src="https://autobahn-testsuite.readthedocs.io/en/latest/_static/img/ws_protocol_test_report_autobahn.png" '
    'border="0" width="300" height="68" alt="Autobahn WebSocket"></img></a></center>'
)

_OUTCOME_DESC_TABLE = """
      <table id="case_outcome_desc">
         <tr>
            <td class="case_ok">Pass</td>
            <td class="outcome_desc">Test case was executed and passed successfully.</td>
         </tr>
         <tr>
            <td class="case_non_strict">Non-Strict</td>
            <td class="outcome_desc">Test case was executed and passed non-strictly.
            A non-strict behavior is one that does not adhere to a SHOULD-behavior as described in the protocol specification or
            a well-defined, canonical behavior that appears to be desirable but left open in the protocol specification.
            An implementation with non-strict behavior is still conformant to the protocol specification.</td>
         </tr>
         <tr>
            <td class="case_failed">Fail</td>
            <td class="outcome_desc">Test case was executed and failed. An implementation which fails a test case - other
            than a performance/limits related one - is non-conforming to a MUST-behavior as described in the protocol specification.</td>
         </tr>
         <tr>
            <td class="case_info">Info</td>
            <td class="outcome_desc">Informational test case which detects certain implementation behavior left unspecified by the spec
            but nevertheless potentially interesting to implementors.</td>
         </tr>
         <tr>
            <td class="case_missing">Missing</td>
            <td class="outcome_desc">Test case is missing, either because it was skipped via the test suite configuration
            or deactivated, i.e. because the implementation does not implement the tested feature or breaks during running
            the test case.</td>
         </tr>
      </table>
      """

_CBV = [
    ("isServer", "True, iff I (the fuzzer) am a server, and the peer is a client."),
    (
        "closedByMe",
        "True, iff I have initiated closing handshake (that is, did send close first).",
    ),
    (
        "failedByMe",
        "True, iff I have failed the WS connection (i.e. due to protocol error). Failing can be either by initiating closing handshake or brutal drop TCP.",
    ),
    ("droppedByMe", "True, iff I dropped the TCP connection."),
    (
        "wasClean",
        "True, iff full WebSocket closing handshake was performed (close frame sent and received) _and_ the server dropped the TCP (which is its responsibility).",
    ),
    ("wasNotCleanReason", "When wasClean == False, the reason what happened."),
    (
        "wasServerConnectionDropTimeout",
        "When we are a client, and we expected the server to drop the TCP, but that didn't happen in time, this gets True.",
    ),
    (
        "wasOpenHandshakeTimeout",
        "When performing the opening handshake, but the peer did not finish in time, this gets True.",
    ),
    (
        "wasCloseHandshakeTimeout",
        "When we initiated a closing handshake, but the peer did not respond in time, this gets True.",
    ),
    ("localCloseCode", "The close code I sent in close frame (if any)."),
    ("localCloseReason", "The close reason I sent in close frame (if any)."),
    ("remoteCloseCode", "The close code the peer sent me in close frame (if any)."),
    ("remoteCloseReason", "The close reason the peer sent me in close frame (if any)."),
]


def _utcnow() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _limit_string(
    value: object, limit: int = MAX_CASE_PICKLE_LEN, indicator: str = " ..."
) -> str:
    text = str(value)
    if len(text) > limit - len(indicator):
        return text[: limit - len(indicator)] + indicator
    return text


def _render_wirelog(wirelog: Sequence[Sequence[Any]], emit: Any) -> None:
    for i, t in enumerate(wirelog):
        kind = t[0]
        if kind in ("RO", "TO", "RF", "TF"):
            payload_len = t[1][0]
            lines = textwrap.wrap(t[1][1], 100)
            if kind == "RO":
                prefix, css = "RX OCTETS", "wirelog_rx_octets"
            elif kind == "TO":
                prefix = "TX OCTETS"
                css = "wirelog_tx_octets_sync" if t[2] else "wirelog_tx_octets"
            elif kind == "RF":
                prefix, css = "RX FRAME ", "wirelog_rx_frame"
            else:  # TF
                prefix = "TX FRAME "
                css = (
                    "wirelog_tx_frame_sync"
                    if (t[8] or t[7] is not None)
                    else "wirelog_tx_frame"
                )
            indent = (2 + 4 + len(prefix)) * " "
            if kind in ("RO", "TO"):
                if lines:
                    emit(
                        f'         <pre class="{css}">{i:03d} {prefix}: {lines[0]}</pre>'
                    )
                    for line in lines[1:]:
                        emit(f'         <pre class="{css}">{indent}{line}</pre>')
            else:
                if kind == "RF":
                    mask = t[6] if t[6] else str(t[6])
                    emit(
                        f'         <pre class="{css}">{i:03d} {prefix}: OPCODE={t[2]}, FIN={t[3]}, RSV={t[4]}, PAYLOAD-LEN={payload_len}, MASKED={t[5]}, MASK={mask}</pre>'
                    )
                else:  # TF
                    emit(
                        f'         <pre class="{css}">{i:03d} {prefix}: OPCODE={t[2]}, FIN={t[3]}, RSV={t[4]}, PAYLOAD-LEN={payload_len}, MASK={t[5]}, PAYLOAD-REPEAT-LEN={t[6]}, CHOPSIZE={t[7]}, SYNC={t[8]}</pre>'
                    )
                for line in lines:
                    emit(f'         <pre class="{css}">{indent}{line}</pre>')
        elif kind == "WLM":
            state = "ENABLED" if t[1] else "DISABLED"
            emit(f'         <pre class="wirelog_delay">{i:03d} WIRELOG {state}</pre>')
        elif kind == "CT":
            emit(
                f'         <pre class="wirelog_delay">{i:03d} DELAY {t[1]:f} sec for TAG {t[2]}</pre>'
            )
        elif kind == "CTE":
            emit(
                f'         <pre class="wirelog_delay">{i:03d} DELAY TIMEOUT on TAG {t[1]}</pre>'
            )
        elif kind == "KL":
            emit(
                f'         <pre class="wirelog_kill_after">{i:03d} FAIL CONNECTION AFTER {t[1]:f} sec</pre>'
            )
        elif kind == "KLE":
            emit(
                f'         <pre class="wirelog_kill_after">{i:03d} FAILING CONNECTION</pre>'
            )
        elif kind == "TI":
            emit(
                f'         <pre class="wirelog_kill_after">{i:03d} CLOSE CONNECTION AFTER {t[1]:f} sec</pre>'
            )
        elif kind == "TIE":
            emit(
                f'         <pre class="wirelog_kill_after">{i:03d} CLOSING CONNECTION</pre>'
            )
        else:
            raise ValueError(f"unrecognized wire log row: {t!r}")


def _detail_html(case: dict[str, Any]) -> str:
    out: list[str] = []
    emit = out.append
    emit("<!DOCTYPE html>")
    emit("<html>")
    emit("   <head>")
    emit('      <meta charset="utf-8" />')
    emit(f'      <style lang="css">{CSS_COMMON}</style>')
    emit(f'      <style lang="css">{CSS_DETAIL_REPORT}</style>')
    emit("   </head>")
    emit("   <body>")
    emit('      <a name="top"></a>')
    emit("      <br/>")
    emit(_LOGOS)
    emit("      <br/>")

    behavior = case["behavior"]
    if behavior == _OK:
        style, text = "case_ok", "Pass"
    elif behavior == _NON_STRICT:
        style, text = "case_non_strict", "Non-Strict"
    elif behavior == _INFORMATIONAL:
        style, text = "case_info", "Informational"
    else:
        style, text = "case_failed", "Fail"
    emit(
        f'      <p class="case {style}">{case["agent"]} - <span style="font-size: 1.3em;"><b>Case {case["id"]}</b></span> : {text} - <span style="font-size: 0.9em;"><b>{case["duration"]}</b> ms @ {case["started"]}</a></p>'
    )

    emit(
        f'      <p class="case_text_block case_desc"><b>Case Description</b><br/><br/>{case["description"]}</p>'
    )
    emit(
        f'      <p class="case_text_block case_expect"><b>Case Expectation</b><br/><br/>{case["expectation"]}</p>'
    )
    emit(
        '      <p class="case_text_block case_outcome">\n'
        f"         <b>Case Outcome</b><br/><br/>{case.get('result', '')}<br/><br/>\n"
        f'         <i>Expected:</i><br/><span class="case_pickle">{_limit_string(case.get("expected", ""))}</span><br/><br/>\n'
        f'         <i>Observed:</i><br><span class="case_pickle">{_limit_string(case.get("received", ""))}</span>\n'
        "      </p>"
    )
    emit(
        f'      <p class="case_text_block case_closing_beh"><b>Case Closing Behavior</b><br/><br/>{case.get("resultClose", "")} ({case.get("behaviorClose", "")})</p>'
    )
    emit("      <br/><hr/>")

    emit("      <h2>Opening Handshake</h2>")
    emit(f'      <pre class="http_dump">{case["httpRequest"].strip()}</pre>')
    emit(f'      <pre class="http_dump">{case["httpResponse"].strip()}</pre>')
    emit("      <br/><hr/>")

    emit("      <h2>Closing Behavior</h2>")
    emit("      <table>")
    emit(
        '         <tr class="stats_header"><td>Key</td><td class="left">Value</td><td class="left">Description</td></tr>'
    )
    for key, desc in _CBV:
        emit(
            f'         <tr class="stats_row"><td>{key}</td><td class="left">{case[key]}</td><td class="left">{desc}</td></tr>'
        )
    emit("      </table>")
    emit("      <br/><hr/>")

    emit("      <h2>Wire Statistics</h2>")
    if not case["createStats"]:
        emit(
            '      <p style="margin-left: 40px; color: #f00;"><i>Statistics for octets/frames disabled!</i></p>'
        )
    else:
        for label, stats in [
            ("Received", case["rxOctetStats"]),
            ("Transmitted", case["txOctetStats"]),
        ]:
            emit(f"      <h3>Octets {label} by Chop Size</h3>")
            emit("      <table>")
            emit(
                '         <tr class="stats_header"><td>Chop Size</td><td>Count</td><td>Octets</td></tr>'
            )
            total_cnt = total_octets = 0
            for size in sorted(stats, key=int):
                n = int(size)
                emit(
                    f'         <tr class="stats_row"><td>{n}</td><td>{stats[size]}</td><td>{n * stats[size]}</td></tr>'
                )
                total_cnt += stats[size]
                total_octets += n * stats[size]
            emit(
                f'         <tr class="stats_total"><td>Total</td><td>{total_cnt}</td><td>{total_octets}</td></tr>'
            )
            emit("      </table>")
        for label, stats in [
            ("Received", case["rxFrameStats"]),
            ("Transmitted", case["txFrameStats"]),
        ]:
            emit(f"      <h3>Frames {label} by Opcode</h3>")
            emit("      <table>")
            emit('         <tr class="stats_header"><td>Opcode</td><td>Count</td></tr>')
            total_cnt = 0
            for opcode in sorted(stats, key=int):
                emit(
                    f'         <tr class="stats_row"><td>{int(opcode)}</td><td>{stats[opcode]}</td></tr>'
                )
                total_cnt += stats[opcode]
            emit(
                f'         <tr class="stats_total"><td>Total</td><td>{total_cnt}</td></tr>'
            )
            emit("      </table>")
    emit("      <br/><hr/>")

    emit("      <h2>Wire Log</h2>")
    if not case["createWirelog"]:
        emit(
            '      <p style="margin-left: 40px; color: #f00;"><i>Wire log after handshake disabled!</i></p>'
        )
    emit('      <div id="wirelog">')
    _render_wirelog(case["wirelog"], emit)
    n = len(case["wirelog"])
    if case["droppedByMe"]:
        emit(
            f'         <pre class="wirelog_tcp_closed_by_me">{n:03d} TCP DROPPED BY ME</pre>'
        )
    else:
        emit(
            f'         <pre class="wirelog_tcp_closed_by_peer">{n:03d} TCP DROPPED BY PEER</pre>'
        )
    emit("      </div>")
    emit("      <br/><hr/>")

    emit("   </body>")
    emit("</html>")
    return "\n".join(out) + "\n"


def _master_case_cells(case: dict[str, Any], agent: str) -> str:
    behavior = case["behavior"]
    if behavior == _UNIMPLEMENTED:
        return '            <td class="case_unimplemented close_flex" colspan="2">Unimplemented</td>'

    report_file = _report_filename(agent, case["id"], "html")
    if behavior == _OK:
        td_text, td_class = "Pass", "case_ok"
    elif behavior == _NON_STRICT:
        td_text, td_class = "Non-Strict", "case_non_strict"
    elif behavior == _NO_CLOSE:
        td_text, td_class = "No Close", "case_no_close"
    elif behavior == _INFORMATIONAL:
        td_text, td_class = "Info", "case_info"
    else:
        td_text, td_class = "Fail", "case_failed"

    close = case["behaviorClose"]
    remote = str(case["remoteCloseCode"])
    if close == _OK:
        ctd_text, ctd_class = remote, "case_ok"
    elif close == _FAILED_BY_CLIENT:
        ctd_text, ctd_class = remote, "case_almost"
    elif close == _WRONG_CODE:
        ctd_text, ctd_class = remote, "case_non_strict"
    elif close == _UNCLEAN:
        ctd_text, ctd_class = "Unclean", "case_failed"
    elif close == _INFORMATIONAL:
        ctd_text, ctd_class = remote, "case_info"
    else:
        ctd_text, ctd_class = "Fail", "case_failed"

    detail = ""
    if case["reportTime"]:
        detail += f"{case['duration']} ms"
    if case["reportCompressionRatio"] and case["trafficStats"] is not None:
        cr_in = case["trafficStats"]["incomingCompressionRatio"]
        cr_out = case["trafficStats"]["outgoingCompressionRatio"]
        cr_in_s = f"{cr_in:.3f}" if cr_in is not None else "-"
        cr_out_s = f"{cr_out:.3f}" if cr_out is not None else "-"
        detail += f" [{cr_in_s}/{cr_out_s}]"

    if detail:
        return (
            f'            <td class="{td_class}"><a href="{report_file}">{td_text}</a><br/><span class="case_duration">{detail}</span></td>'
            f'<td class="close close_hide {ctd_class}"><span class="close_code">{ctd_text}</span></td>'
        )
    return (
        f'            <td class="{td_class}"><a href="{report_file}">{td_text}</a></td>'
        f'<td class="close close_hide {ctd_class}"><span class="close_code">{ctd_text}</span></td>'
    )


def _master_html(
    index: dict[str, dict[str, Any]], cases: dict[tuple[str, str], dict[str, Any]]
) -> str:
    # Importing kosoku.cases builds the index (and puts the case/shim dirs on
    # sys.path), so the category lookup below can import the generator modules.
    from kosoku._caseindex import category_metadata
    from kosoku.cases import CASES, GENERATORS

    categories, subcategories = category_metadata(GENERATORS)
    agents = sorted(index)

    out: list[str] = []
    emit = out.append
    emit("<!DOCTYPE html>")
    emit("<html>")
    emit("   <head>")
    emit('      <meta charset="utf-8" />')
    emit(f'      <style lang="css">{CSS_COMMON}</style>')
    emit(f'      <style lang="css">{CSS_MASTER_REPORT}</style>')
    script = JS_MASTER_REPORT % {"agents_cnt": len(agents)}
    emit(f'      <script language="javascript">{script}</script>')
    emit("   </head>")
    emit("   <body>")
    emit(
        '      <a href="#"><div id="toggle_button" class="unselectable" onclick="toggleClose();">Toggle Details</div></a>'
    )
    emit('      <a name="top"></a>')
    emit("      <br/>")
    emit(_LOGOS)
    emit('      <div id="master_report_header" class="block">')
    emit(
        f'         <p id="intro">Summary report generated on {_utcnow()} (UTC) by <a href="https://autobahn-testsuite.readthedocs.io/">Autobahn WebSocket Testsuite</a> cases (via kosoku).</p>'
    )
    emit(_OUTCOME_DESC_TABLE)
    emit("      </div>")

    emit('      <table id="agent_case_results">')
    case_list = list(CASES)
    last_category = last_subcategory = None
    for case_id in case_list:
        category_index = case_id.split(".")[0]
        category = categories.get(category_index, "Misc")
        subcategory_index = ".".join(case_id.split(".")[:2])
        subcategory = subcategories.get(subcategory_index, None)

        # repeatAgentRowPerSubcategory=True: repeat the agent header per subcategory.
        if category != last_category or subcategory != last_subcategory:
            emit('         <tr class="case_category_row">')
            emit(
                f'            <td class="case_category">{category_index} {category}</td>'
            )
            for agent in agents:
                emit(
                    f'            <td class="agent close_flex" colspan="2">{agent}</td>'
                )
            emit("         </tr>")
            last_category = category
            last_subcategory = None

        if subcategory != last_subcategory:
            emit('         <tr class="case_subcategory_row">')
            emit(
                f'            <td class="case_subcategory" colspan="{len(agents) * 2 + 1}">{subcategory_index} {subcategory}</td>'
            )
            emit("         </tr>")
            last_subcategory = subcategory

        emit('         <tr class="agent_case_result_row">')
        emit(
            f'            <td class="case"><a href="#case_desc_{case_id.replace(".", "_")}">Case {case_id}</a></td>'
        )
        for agent in agents:
            case = cases.get((agent, case_id))
            if case is not None:
                emit(_master_case_cells(case, agent))
            else:
                emit(
                    '            <td class="case_missing close_flex" colspan="2">Missing</td>'
                )
        emit("         </tr>")
    emit("      </table>")
    emit("      <br/><hr/>")

    emit('      <div id="test_case_descriptions">')
    for case_id in case_list:
        case_class = CASES[case_id]
        emit("      <br/>")
        emit(f'      <a name="case_desc_{case_id.replace(".", "_")}"></a>')
        emit(f"      <h2>Case {case_id}</h2>")
        emit('      <a class="up" href="#top">Up</a>')
        emit(
            f'      <p class="case_text_block case_desc"><b>Case Description</b><br/><br/>{getattr(case_class, "DESCRIPTION", "")}</p>'
        )
        emit(
            f'      <p class="case_text_block case_expect"><b>Case Expectation</b><br/><br/>{getattr(case_class, "EXPECTATION", "")}</p>'
        )
    emit("      </div>")
    emit("      <br/><hr/>")

    emit("   </body>")
    emit("</html>")
    return "\n".join(out) + "\n"
