"""Shared test helpers: per-case parametrization and a websockets testee.

`case_params()` turns the `kosoku.cases` index into a `pytest.param` per case,
marking the heavy 12/13 permessage-deflate cases (large payloads) `slow` so they
are deselected unless `--full` is given (see conftest.py).
"""

from __future__ import annotations

import kosoku.cases
import pytest

# Payload size (bytes) above which a 12/13 compression case counts as "slow".
_SLOW_PAYLOAD = 1024


def case_params() -> list:
    """One `pytest.param` per case (from the index); large 12/13 marked slow."""
    params = []
    for case_id, case_cls in kosoku.cases.CASES.items():
        major = int(case_id.split(".", 1)[0])
        payload_len = getattr(case_cls, "LEN", 0)
        is_slow = major in (12, 13) and payload_len > _SLOW_PAYLOAD
        marks = [pytest.mark.slow] if is_slow else []
        params.append(pytest.param(case_id, id=case_id, marks=marks))
    return params
