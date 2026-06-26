"""Tests for the Utf8Validator (kosoku._kosoku).

Covers the behaviour the 6.x case generator relies on: validity classification
and the "ends on a code-point boundary" flag, including incremental validation.
"""

from __future__ import annotations

import pytest

from kosoku._kosoku import Utf8Validator


def fully_valid(data: bytes) -> bool:
    """validate() returns (valid?, endsOnCodePoint?, ...) — both true ⇒ a
    complete, valid UTF-8 string."""
    ok, ends, _, _ = Utf8Validator().validate(data)
    return ok and ends


@pytest.mark.parametrize(
    "data",
    [
        b"\xce\xba\xe1\xbd\xb9\xcf\x83\xce\xbc\xce\xb5",  # "κόσμε"
        b"hello\xc2\xa2world",  # U+00A2
        b"\x00",
        b"\xf4\x8f\xbf\xbf",  # last valid code point U+10FFFF
        b"",
    ],
)
def test_accepts_valid(data: bytes) -> None:
    assert fully_valid(data)


@pytest.mark.parametrize(
    "data",
    [
        b"\x80",  # lone continuation byte
        b"\xed\xa0\x80",  # UTF-16 surrogate
        b"\xf4\x90\x80\x80",  # code point > U+10FFFF
        b"\xc0\xaf",  # overlong encoding
        b"\xfe",  # impossible byte
    ],
)
def test_rejects_invalid(data: bytes) -> None:
    assert not fully_valid(data)


def test_prefixes_track_codepoint_boundaries() -> None:
    vss = b"\xce\xba\xe1\xbd\xb9\xcf\x83\xce\xbc\xce\xb5"
    ends = lambda n: Utf8Validator().validate(vss[:n])[1]  # noqa: E731
    assert ends(2)  # κ complete (2 bytes)
    assert not ends(1)  # mid κ
    assert not ends(3)  # mid 3-byte char
    assert ends(5)  # through the 3-byte char


def test_incremental_across_chunks() -> None:
    v = Utf8Validator()
    assert v.validate(b"\xce") == (True, False, 1, 1)  # incomplete tail
    ok, ends, _, _ = v.validate(b"\xba")  # completes κ
    assert ok and ends


def test_reject_is_sticky() -> None:
    v = Utf8Validator()
    assert not v.validate(b"\x80")[0]
    assert not v.validate(b"a")[0]  # cannot recover once rejected


def test_accepts_str_input() -> None:
    # The cases author byte vectors as str literals; codepoint→byte (latin-1).
    assert fully_valid(b"\xce\xba") == (
        Utf8Validator().validate("\xce\xba")[:2] == (True, True)
    )
