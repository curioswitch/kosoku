"""Initializes the case index read by the runner."""

from __future__ import annotations

import binascii
import importlib
import sys
from pathlib import Path

_INITIALIZED = False


def _case_id(class_name: str) -> str:
    """``Case6_5_1`` -> ``6.5.1``."""
    return class_name.removeprefix("Case").replace("_", ".")


def _sort_key(case_id: str) -> list[int]:
    """Numeric ordering by component (1.2.10 after 1.2.9, not lexicographic)."""
    return [int(p) for p in case_id.split(".")]


def _group(cases: dict[str, type]) -> dict[str, dict[str, list[str]]]:
    """Group case ids ``major -> "major.minor" -> [ids]`` for report rendering."""
    groups: dict[str, dict[str, list[str]]] = {}
    for case_id in cases:
        parts = case_id.split(".")
        groups.setdefault(parts[0], {}).setdefault(".".join(parts[:2]), []).append(
            case_id
        )
    return groups


def initialize_shims() -> None:
    """Prepare the interpreter to import the py2-era Autobahn cases under py3.
    Must run before `build_index` (which imports the cases).
    """
    global _INITIALIZED
    if _INITIALIZED:
        return
    _INITIALIZED = True

    root = Path(__file__).resolve().parent
    for extra in (root / "cases", root / "shim"):
        path = str(extra)
        if path not in sys.path:
            sys.path.insert(0, path)

    # Syntactic py2->py3 differences are handled at vendoring time
    # (scripts/generate.py); binascii.b2a_hex is the one *runtime* difference
    # left. py2's took a str (str == bytes) and indexing bytes yielded a 1-char
    # str; py3 requires bytes / yields an int.
    original = binascii.b2a_hex

    def b2a_hex(data, *args, **kwargs):  # type: ignore[no-untyped-def]
        if isinstance(data, str):
            data = data.encode("latin-1")
        elif isinstance(data, int):
            data = bytes([data & 0xFF])
        return original(data, *args, **kwargs)

    binascii.b2a_hex = b2a_hex


def build_index(
    static_modules: list[str],
    generators: list[tuple[str, str]],
) -> tuple[dict[str, type], dict[str, dict[str, list[str]]]]:
    """Import the vendored cases and return ``(CASES, GROUPS)``.

    Call `initialize_shims` first (it puts the cases/shims on ``sys.path``.
    """
    classes: list[type] = []
    for module_name in static_modules:
        module = importlib.import_module(module_name)
        classes.append(getattr(module, "Case" + module_name.removeprefix("case")))
    for module_name, attribute in generators:
        classes.extend(getattr(importlib.import_module(module_name), attribute))

    cases = {_case_id(cls.__name__): cls for cls in classes}
    cases = dict(sorted(cases.items(), key=lambda kv: _sort_key(kv[0])))
    return cases, _group(cases)


def category_metadata(
    generators: list[tuple[str, str]],
) -> tuple[dict[str, str], dict[str, str]]:
    """Return ``(CaseCategories, CaseSubCategories)`` for report rendering."""
    from kosoku._report_assets import CaseCategories, CaseSubCategories

    subcategories = dict(CaseSubCategories)
    for module_name, attribute in generators:
        module = importlib.import_module(module_name)
        extra = getattr(module, f"{attribute}_CaseSubCategories", None)
        if extra:
            subcategories.update(extra)
    return dict(CaseCategories), subcategories
