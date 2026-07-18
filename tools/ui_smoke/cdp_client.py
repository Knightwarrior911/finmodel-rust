# -*- coding: utf-8 -*-
"""
cdp_client.py — zero-dependency CDP client for UI smoke tests.

Python stdlib only (socket/base64/struct/json/urllib). No pip installs.
Proven pattern: same client that drives the pdf-panda-tauri smoke suites.

Usage:
    from cdp_client import cdp_connect, wait_js, js_ok
    ws = cdp_connect()                        # app already exposing :9222
    r  = ws.eval("void openSomePanel()")      # act
    assert js_ok(r), r                        # JS exception check
    ok = wait_js(ws, "!!document.querySelector('#panel:not(.hidden)')")  # assert

Entire public API: cdp_connect, WS.eval, WS.eval_json, WS.close, wait_js, js_ok.

eval() always sets returnByValue=true + awaitPromise=true, so Promise-returning
expressions resolve to their value. JS exceptions come back as
{"__js_error__": "<description>"} instead of raising.
"""
import base64
import json
import os
import socket
import struct
import sys
import time
import urllib.request

# Windows consoles default to cp1252; app output often has U+2212 etc.
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

CDP_HTTP = "http://127.0.0.1:9222"


class WS:
    """Minimal RFC-6455 WebSocket client (client->server frames masked)."""

    def __init__(self, url):
        assert url.startswith("ws://")
        hostport, self.path = url[5:].split("/", 1)
        self.path = "/" + self.path
        host, port = hostport.split(":")
        self.sock = socket.create_connection((host, int(port)), timeout=40)
        key = base64.b64encode(os.urandom(16)).decode()
        req = (
            "GET %s HTTP/1.1\r\nHost: %s:%s\r\nUpgrade: websocket\r\n"
            "Connection: Upgrade\r\nSec-WebSocket-Key: %s\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n" % (self.path, host, port, key)
        )
        self.sock.sendall(req.encode())
        resp = b""
        while b"\r\n\r\n" not in resp:
            resp += self.sock.recv(4096)
        if b" 101 " not in resp.split(b"\r\n", 1)[0]:
            raise ConnectionError(
                "WebSocket handshake rejected (%r) — missing "
                "--remote-allow-origins=* on the target?" % resp[:80]
            )
        self._id = 0

    def _send_raw(self, opcode, payload):
        header = bytearray([0x80 | opcode])
        mask = os.urandom(4)
        n = len(payload)
        if n < 126:
            header.append(0x80 | n)
        elif n < 65536:
            header.append(0x80 | 126)
            header += struct.pack(">H", n)
        else:
            header.append(0x80 | 127)
            header += struct.pack(">Q", n)
        header += mask
        self.sock.sendall(
            bytes(header) + bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        )

    def _recv(self):
        def rd(n):
            buf = b""
            while len(buf) < n:
                c = self.sock.recv(n - len(buf))
                if not c:
                    raise EOFError
                buf += c
            return buf

        b0, b1 = rd(2)
        opcode = b0 & 0x0F
        ln = b1 & 0x7F
        if ln == 126:
            ln = struct.unpack(">H", rd(2))[0]
        elif ln == 127:
            ln = struct.unpack(">Q", rd(8))[0]
        payload = rd(ln) if ln else b""
        if opcode == 0x9:                      # ping -> pong
            self._send_raw(0xA, payload)
            return self._recv()
        if opcode == 0x8:
            raise EOFError("ws closed")
        return opcode, payload.decode("utf-8", "replace")

    def eval(self, expr, timeout=60):
        """Runtime.evaluate `expr`; return its value, or {"__js_error__":...}."""
        self._id += 1
        mid = self._id
        msg = {
            "id": mid,
            "method": "Runtime.evaluate",
            "params": {
                "expression": expr,
                "returnByValue": True,
                "awaitPromise": True,
            },
        }
        self.sock.settimeout(timeout)
        self._send_raw(0x1, json.dumps(msg).encode())
        while True:
            opcode, text = self._recv()
            if opcode not in (0x1, 0x2):
                continue
            try:
                m = json.loads(text)
            except ValueError:
                continue
            if m.get("id") == mid:
                r = m.get("result", {})
                if "exceptionDetails" in r:
                    ex = r["exceptionDetails"]
                    return {
                        "__js_error__": ex.get("exception", {}).get("description")
                        or ex.get("text")
                        or json.dumps(ex)[:300]
                    }
                return r.get("result", {}).get("value")

    def eval_json(self, expr, timeout=60):
        """eval() an expression ending in JSON.stringify(...); parse the result."""
        raw = self.eval(expr, timeout=timeout)
        if isinstance(raw, str):
            try:
                return json.loads(raw)
            except ValueError:
                pass
        return raw

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass


def cdp_connect(http_base=CDP_HTTP, wait=15):
    """Poll /json/list until a page target exists; return a connected WS."""
    deadline = time.time() + wait
    last = None
    while time.time() < deadline:
        try:
            data = json.load(
                urllib.request.urlopen(http_base + "/json/list", timeout=5)
            )
            for t in data:
                if t.get("type") == "page":
                    return WS(t["webSocketDebuggerUrl"])
            last = "no page-type target yet"
        except Exception as e:                 # endpoint not up yet
            last = e
        time.sleep(1.0)
    raise SystemExit(
        "No CDP page target on %s (%s). Is the app running WITH the port "
        "exposed? See cdp-ui-testing.md Step 1 — on Tauri the port must be "
        "baked into the build; kill stale msedgewebview2.exe first." % (http_base, last)
    )


def js_ok(result):
    """True unless eval() returned a JS exception ({"__js_error__": ...})."""
    return not (isinstance(result, dict) and "__js_error__" in result)


def wait_js(ws, condition, timeout=10, interval=0.25):
    """Poll a JS boolean expression until truthy. Returns True/False (no raise)."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        v = ws.eval("!!(%s)" % condition, timeout=15)
        if v is True:
            return True
        time.sleep(interval)
    return False


if __name__ == "__main__":
    ws = cdp_connect()
    print("connected:", ws.eval("document.title"))
    ws.close()
