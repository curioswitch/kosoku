"""Fake ``autobahntestsuite`` package: only present so the 12.x/13.x cases can
locate their bundled test corpora via
``pkg_resources.resource_filename("autobahntestsuite", "testdata/...")``.
The real package (Twisted-based) is not installed.
"""

from __future__ import annotations
