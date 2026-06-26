"""Console-script entrypoint."""

from __future__ import annotations

import asyncio
import sys
from argparse import Action, ArgumentDefaultsHelpFormatter, ArgumentParser
from pathlib import Path
from typing import Literal, get_args
from urllib.parse import urlsplit

from kosoku import FailureError, run_fuzzingclient, run_fuzzingserver

from ._report import write_reports

Mode = Literal["fuzzingclient", "fuzzingserver"]

_SCHEME_PORTS = {"ws": 80, "wss": 443, "http": 80, "https": 443}


class _HelpFormatter(ArgumentDefaultsHelpFormatter):
    """Append `(default: ...)` only when there is a default worth showing."""

    def _get_help_string(self, action: Action) -> str | None:
        if action.default is None:
            return action.help
        return super()._get_help_string(action)


class CLIArgs:
    mode: Mode
    address: str
    include: list[str] | None
    exclude: list[str] | None
    parallelism: int
    spec: str | None
    outdir: str | None


def _build_parser() -> ArgumentParser:
    parser = ArgumentParser(
        prog="kosoku",
        description="WebSocket protocol-compliance test suite",
        formatter_class=_HelpFormatter,
    )
    parser.add_argument(
        "-m",
        "--mode",
        choices=get_args(Mode),
        default="fuzzingclient",
        help="drive a server under test, or be the server a client connects to",
    )
    parser.add_argument(
        "address",
        nargs="?",
        default="127.0.0.1:9001",
        help="if fuzzingclient, address of the server under test. If fuzzingserver, address to bind to (host:port)",
    )
    parser.add_argument(
        "-i",
        "--include",
        action="append",
        metavar="CASE",
        help="case id or `*` glob to run (e.g. 1.1.1 or 9.*). (default: run the whole suite) (repeatable)",
    )
    parser.add_argument(
        "-x",
        "--exclude",
        action="append",
        metavar="CASE",
        help="case id or `*` glob to exclude from the selection (repeatable)",
    )
    parser.add_argument(
        "-p",
        "--parallelism",
        type=int,
        default=1,
        help="max cases to run in parallel (fuzzingclient mode)",
    )
    parser.add_argument(
        "-s",
        "--spec",
        type=str,
        default=None,
        help="Autobahn JSON config file (not yet supported)",
    )
    parser.add_argument(
        "-o",
        "--outdir",
        type=str,
        default=None,
        help="report output directory (default: reports/clients or reports/servers)",
    )
    return parser


async def amain() -> None:
    parser = _build_parser()
    args = parser.parse_args(namespace=CLIArgs())

    if args.spec is not None:
        parser.error("config-file (--spec) support is not implemented yet")

    is_client = args.mode == "fuzzingclient"
    outdir = args.outdir or (
        Path("reports") / "clients" if is_client else Path("reports") / "servers"
    )

    # The entrypoints return the per-case results, or raise FailureError
    # (which carries those results) if any case failed. We write the
    # report either way, then map a failure to the exit code.
    failed = False
    try:
        if is_client:
            results = await run_fuzzingclient(
                args.address, args.include, args.exclude, concurrency=args.parallelism
            )
        else:
            url = urlsplit(
                args.address if "://" in args.address else f"//{args.address}"
            )
            try:
                host, port = url.hostname, url.port
            except ValueError:
                host = port = None
            if port is None and url.scheme:
                port = _SCHEME_PORTS.get(url.scheme)
            if not host or port is None:
                return parser.error(
                    f"invalid bind address {args.address!r} (expected host:port)"
                )
            async with run_fuzzingserver(
                args.include, args.exclude, host=host, port=port
            ) as server:
                print(
                    f"fuzzingserver listening on ws://{server.address}:{server.port}",
                    file=sys.stderr,
                )
                results = await server.get_result()
    except FailureError as failure:
        print(failure, file=sys.stderr)
        results = failure.results
        failed = True

    if results:
        write_reports(results, str(outdir))
        print(f"wrote reports for {len(results)} cases to {outdir}/")

    if failed:
        raise SystemExit(1)
    print(f"{len(results)} cases passed")


def main() -> None:
    asyncio.run(amain())
