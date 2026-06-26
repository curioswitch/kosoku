"""kosoku: a WebSocket protocol-compliance fuzzing client and server."""

from __future__ import annotations

__all__ = ["FuzzingServer", "FailureError", "run_fuzzingclient", "run_fuzzingserver"]

from ._errors import FailureError
from ._kosoku import FuzzingServer, run_fuzzingclient, run_fuzzingserver
