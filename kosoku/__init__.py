"""kosoku: a WebSocket protocol-compliance fuzzing client and server."""

from __future__ import annotations

__all__ = ["FuzzingServer", "TestFailure", "run_fuzzingclient", "run_fuzzingserver"]

from ._errors import TestFailure
from ._kosoku import FuzzingServer, run_fuzzingclient, run_fuzzingserver
