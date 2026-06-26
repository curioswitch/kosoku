"""fuzzingserver mode: kosoku is the server, a websockets testee drives cases.

The mirror of the client tests. Each case runs server-side on its own
short-lived fuzzingserver against a compliant `websockets` echo testee; cases
are parametrized from the same `kosoku.cases` index, with the heavy 12/13 cases
marked `slow`. A separate `slow` test exercises the whole suite in one `kosoku`
CLI invocation.
"""

from __future__ import annotations

import asyncio
import json
import os
import re
import shutil
from html.parser import HTMLParser
from pathlib import Path

import pytest
import websockets
from websockets.extensions.permessage_deflate import ClientPerMessageDeflateFactory

from kosoku import run_fuzzingserver
from kosoku.cases import CASES

from ._util import case_params

# Cases the websockets library itself documents as a known failure (not a server bug).
_TESTEE_XFAIL = {"7.1.5"}


@pytest.mark.parametrize("case_id", case_params())
async def test_case(case_id: str) -> None:
    if case_id in _TESTEE_XFAIL:
        pytest.xfail(
            "websockets documents 7.1.5 as a known failure: it sends 1002 "
            "on close-during-fragmentation; the case expects 1000"
        )
    async with run_fuzzingserver([case_id], host="127.0.0.1", port=0) as server:
        await run_testee(server.port)
        await asyncio.wait_for(server.get_result(), timeout=120)


async def test_case_glob_expands() -> None:
    expected = {c for c in CASES if c.startswith("1.1.")}
    assert len(expected) > 1, "expected several 1.1.x cases to exist"

    async with run_fuzzingserver(["1.1.*"], host="127.0.0.1", port=0) as server:
        await run_testee(server.port)
        results = await server.get_result()
    assert {r.case_id for r in results} == expected


async def test_exclude_cases() -> None:
    selected = {c for c in CASES if c.startswith("1.1.")}
    expected = selected - {"1.1.1"}
    assert expected and "1.1.1" in selected

    async with run_fuzzingserver(
        ["1.1.*"], ["1.1.1"], host="127.0.0.1", port=0
    ) as server:
        await run_testee(server.port)
        results = await server.get_result()
    assert {r.case_id for r in results} == expected


@pytest.mark.slow
async def test_cli(tmp_path: Path) -> None:
    kosoku = shutil.which("kosoku")
    assert kosoku, "kosoku binary not on PATH (run `uv run poe build`)"

    # Serve the whole suite on an ephemeral port, excluding the one case the
    # websockets testee is known to fail (the run exits non-zero on any failure).
    outdir = tmp_path / "reports"
    proc = await asyncio.create_subprocess_exec(
        kosoku,
        "--mode",
        "fuzzingserver",
        "127.0.0.1:0",
        "-x",
        "7.1.5",
        "-o",
        str(outdir),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )

    # The server prints its bound address; learn the port and drive the testee.
    assert proc.stderr is not None
    port = None
    while port is None:
        line = await asyncio.wait_for(proc.stderr.readline(), timeout=60)
        assert line, "server exited before reporting a listening address"
        match = re.search(rb"ws://127\.0\.0\.1:(\d+)", line)
        if match:
            port = int(match.group(1))
    await run_testee(port)

    stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=600)
    assert proc.returncode == 0, (
        f"run failed (rc={proc.returncode}):\n{stdout.decode()}{stderr.decode()}"
    )

    # The run wrote Autobahn's index.json + per-case JSON/HTML layout.
    index = json.loads((outdir / "index.json").read_text())
    assert len(index) == 1, "one agent (the testee)"
    [agent] = index
    assert {"1.1.1", "7.3.1", "12.1.1"} <= set(index[agent])
    summary = index[agent]["1.1.1"]
    assert set(summary) == {
        "behavior",
        "behaviorClose",
        "remoteCloseCode",
        "duration",
        "reportfile",
    }
    assert summary["behavior"] == "OK"

    # The per-case detail file carries the full Autobahn field set, including a
    # populated wire-log and (for 12.1.1) compression traffic stats.
    detail = json.loads((outdir / summary["reportfile"]).read_text())
    for key in (
        "case",
        "id",
        "expected",
        "received",
        "wirelog",
        "httpRequest",
        "txFrameStats",
        "isServer",
        "wasClean",
        "trafficStats",
    ):
        assert key in detail, key
    assert detail["id"] == "1.1.1"
    assert detail["isServer"] is True
    assert detail["wirelog"], "wirelog should be captured"

    deflate = json.loads((outdir / index[agent]["12.1.1"]["reportfile"]).read_text())
    assert deflate["reportCompressionRatio"] is True
    assert deflate["trafficStats"]["incomingCompressionRatio"] is not None

    # HTML reports: a master index.html (the full case matrix) + per-case detail
    # pages, all well-formed and carrying the expected sections.
    master = (outdir / "index.html").read_text()
    for marker in ('id="agent_case_results"', "Case 1.1.1", "case_ok"):
        assert marker in master, marker
    detail_html = (outdir / summary["reportfile"].replace(".json", ".html")).read_text()
    for marker in (
        "Wire Log",
        'id="wirelog"',
        "Closing Behavior",
        "Opening Handshake",
    ):
        assert marker in detail_html, marker
    HTMLParser().feed(master)
    HTMLParser().feed(detail_html)


async def run_testee(port: int) -> None:
    base = f"ws://127.0.0.1:{port}"

    # max_size=None: 9.x sends multi-MB messages; the default 1 MiB cap would
    # make the testee reject them.
    count = None
    for _ in range(200):  # wait (briefly) for the server to bind
        try:
            async with websockets.connect(f"{base}/getCaseCount", max_size=None) as ws:
                count = int(await ws.recv())
            break
        except OSError:
            await asyncio.sleep(0.01)
    assert count is not None, f"fuzzingserver never came up on :{port}"

    sem = asyncio.Semaphore(os.cpu_count() or 1)

    async def run_case(i: int) -> None:
        async with sem:
            try:
                async with websockets.connect(
                    f"{base}/runCase?case={i}&agent=ws",
                    max_size=None,
                    extensions=_DEFLATE,
                ) as ws:
                    async for message in ws:
                        await ws.send(message)  # echo
            except Exception:
                pass  # protocol error / close is expected for many cases

    await asyncio.gather(*(run_case(i) for i in range(1, count + 1)))

    try:
        async with websockets.connect(f"{base}/updateReports?agent=ws", max_size=None):
            pass
    except Exception:
        pass


# Offer permessage-deflate advertising client_no_context_takeover and
# client_max_window_bits, so the server-side cases whose SERVER_ACCEPT requires
# those parameters (13.2/13.5/13.6 etc.) can negotiate. The client always
# resetting its compressor is safe regardless of what the server decoder uses.
_DEFLATE = [
    ClientPerMessageDeflateFactory(
        client_no_context_takeover=True,
        server_no_context_takeover=False,
        client_max_window_bits=True,
    )
]
