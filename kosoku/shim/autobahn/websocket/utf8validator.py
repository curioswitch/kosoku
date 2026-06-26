"""Shim for autobahn.websocket.utf8validator.

The Autobahn 6.x case generator imports ``Utf8Validator`` from here to classify
UTF-8 test sequences. Rather than bundle Autobahn's pure-Python DFA, we expose a
Rust implementation from the ``kosoku._kosoku`` extension; this shim re-exports
it so the cases' import path resolves unchanged.
"""

from __future__ import annotations

from kosoku._kosoku import Utf8Validator

__all__ = ["Utf8Validator"]
