"""Exceptions raised by the runner entrypoints."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from kosoku._kosoku import CaseResult


class FailureError(AssertionError):
    """Raised when a run finishes but one or more cases failed.

    Attributes:
        results: list[CaseResult] -- the results of the failed cases
    """

    def __init__(
        self, message: str = "", results: list[CaseResult] | None = None
    ) -> None:
        super().__init__(message)
        self.results: list[CaseResult] = list(results) if results is not None else []
