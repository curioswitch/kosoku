from __future__ import annotations


async def app(scope, receive, send):
    """Minimal ASGI WebSocket echo: echo every frame back verbatim."""
    assert scope["type"] == "websocket"
    await send({"type": "websocket.accept"})
    while True:
        event = await receive()
        if event["type"] == "websocket.disconnect":
            break
        if event["type"] == "websocket.receive":
            if event.get("bytes") is not None:
                await send({"type": "websocket.send", "bytes": event["bytes"]})
            else:
                await send({"type": "websocket.send", "text": event["text"]})
