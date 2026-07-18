# -*- coding: utf-8 -*-
"""shot.py — capture the running app's webview to a PNG via CDP Page.captureScreenshot."""
import base64
import json
import sys
from cdp_client import cdp_connect

out = sys.argv[1] if len(sys.argv) > 1 else "_shot.png"
ws = cdp_connect()
ws._id += 1
mid = ws._id
ws.sock.settimeout(30)
ws._send_raw(0x1, json.dumps({"id": mid, "method": "Page.enable"}).encode())
# drain until ack
while True:
    op, t = ws._recv()
    if op in (0x1, 0x2):
        m = json.loads(t)
        if m.get("id") == mid:
            break
ws._id += 1
mid = ws._id
ws._send_raw(0x1, json.dumps({"id": mid, "method": "Page.captureScreenshot",
                              "params": {"format": "png", "captureBeyondViewport": False}}).encode())
data = None
while True:
    op, t = ws._recv()
    if op in (0x1, 0x2):
        m = json.loads(t)
        if m.get("id") == mid:
            data = m.get("result", {}).get("data")
            break
open(out, "wb").write(base64.b64decode(data))
print("wrote", out, len(data), "b64 chars")
ws.close()
