"""fuzzingclient mode: drive the Autobahn cases against a live echo server.

An echo server runs for the session. Each case
is run individually through the programmatic entrypoint
(`_kosoku.run_fuzzingclient`), parametrized from the `kosoku.cases` index so a
failure points at the exact case. The heavy 12/13 permessage-deflate cases
(large payloads) are marked `slow`; run them with `pytest --full`. A separate
`slow` test exercises the whole suite in one `kosoku` CLI invocation.
"""

from __future__ import annotations

import asyncio
import json
import os
import shutil
from collections.abc import AsyncIterator
from html.parser import HTMLParser
from pathlib import Path

import pytest
from kosoku import run_fuzzingclient
from kosoku.cases import CASES
from pyvoy import PyvoyServer

from ._util import case_params


@pytest.fixture(scope="module")
async def server_url() -> AsyncIterator[str]:
    async with PyvoyServer("tests.apps.echo", websockets=True) as server:
        yield f"ws://{server.listener_address}:{server.listener_port}"


@pytest.mark.parametrize("case_id", case_params())
async def test_case(server_url: str, case_id: str) -> None:
    await run_fuzzingclient(server_url, [case_id], concurrency=os.cpu_count() or 1)


async def test_case_glob_expands(server_url: str) -> None:

    expected = {c for c in CASES if c.startswith("1.1.")}
    assert len(expected) > 1, "expected several 1.1.x cases to exist"

    results = await run_fuzzingclient(server_url, ["1.1.*"])
    assert {r.case_id for r in results} == expected


async def test_exclude_cases(server_url: str) -> None:

    selected = {c for c in CASES if c.startswith("1.1.")}
    expected = selected - {"1.1.1"}
    assert expected and "1.1.1" in selected

    results = await run_fuzzingclient(server_url, ["1.1.*"], ["1.1.1"])
    assert {r.case_id for r in results} == expected


@pytest.mark.slow
async def test_cli(server_url: str, tmp_path: Path) -> None:
    kosoku = shutil.which("kosoku")
    assert kosoku, "kosoku binary not on PATH (run `uv run poe build`)"

    outdir = tmp_path / "reports"
    proc = await asyncio.create_subprocess_exec(
        kosoku,
        server_url,
        "-p",
        str(os.cpu_count() or 1),
        "-o",
        str(outdir),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=600)
    output = stdout.decode()
    assert proc.returncode == 0, f"suite failed (rc={proc.returncode}):\n{output}"

    # The run wrote Autobahn's index.json + per-case JSON/HTML layout.
    index = json.loads((outdir / "index.json").read_text())
    assert len(index) == 1, "one agent (the echo server)"
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
        "TCP DROPPED",
    ):
        assert marker in detail_html, marker
    HTMLParser().feed(master)
    HTMLParser().feed(detail_html)
