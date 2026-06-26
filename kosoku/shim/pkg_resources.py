"""Minimal pkg_resources shim — just ``resource_filename``.

The 12.x/13.x compression cases call
``pkg_resources.resource_filename("autobahntestsuite", "testdata/<file>")`` to
locate their test corpora. The real pkg_resources (setuptools) is heavy and not
needed; this reproduces the only behaviour the cases use: resolve a resource to
a filesystem path under its (here, vendored) package directory. Our shim sits
ahead of site-packages on sys.path, so this is what the cases import.
"""

from __future__ import annotations

import importlib
from pathlib import Path


def resource_filename(package: str, resource: str) -> str:
    module = importlib.import_module(package)
    assert module.__file__ is not None
    return str(Path(module.__file__).parent / resource)
