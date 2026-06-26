# kosoku

`kosoku` (pronounced kōsoku) is a fast and modern WebSocket protocol-compliance fuzzing client.
Kosoku is the Japanese word for highway and is inspired by the
[Autobahn TestSuite](https://github.com/crossbario/autobahn-testsuite).

kosoku drives the Autobahn TestSuite's own case files against a server or client under test,
but replaces Autobahn's test runner with one written in Rust, using PyO3 to drive actual Autobahn
Python case files. While they cannot be used 100% as-is due to Python2/3 differences
we need to adjust for, we can auto-translate the few fixes needed for cases that
are still structured identically. This ensures the same test coverage for this
new implementation, which would otherwise be difficult to verify if porting everything to Rust (or Go, etc).

## Usage

The runner is published as native wheels to PyPI. For example, you can try it with `uvx`.

```shellsession
uvx kosoku -h
```

For Python users, the exported Python API is likely more convenient.

```python
import asyncio
from kosoku import run_fuzzingclient, run_fuzzingserver

def test_client():
    async with run_fuzzingserver() as server:
        await run_client_under_test(server.address)
        await asyncio.wait_for(server.get_result(), timeout=120)

def test_server():
    # Or more likely a pytest fixture
    server = asyncio.create_task(run_server_under_test())
    await run_fuzzingclient("localhost:9001")
    server.cancel()
```

## Why another runner?

Autobahn TestSuite has excellent coverage of the WebSocket spec and is the de-facto
conformance suite across almost, if not all of WebSocket implementations. However,
it is showing its age, only running with Python 2. Given the challenge of setting up
Python 2 in modern times, this effectively means having to use Docker to run tests,
which is tedious and slow. This is especially slow on modern macOS laptops because
there aren't any arm64 images available.

The test suite is too important to have such a legacy setup, so this project brings a new
modern runner, fully compatible, but with the performance of concurrent Rust and
binaries published for a wide variety of platforms.
