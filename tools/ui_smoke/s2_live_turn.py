# -*- coding: utf-8 -*-
"""s2_live_turn.py — drive ONE real agent turn end-to-end over CDP.

Verifies the live driver pipeline (agent_send -> actor -> LiveDriver -> provider
-> events -> reducer -> UI -> terminal) and that this session's live-path edits
(remaining()-guard removal, await_approval park-insert, schema v5, sweep tick)
did not regress the running app. Uses the configured provider key (cheap turn).
"""
import sys
from cdp_client import cdp_connect, wait_js, js_ok

ws = cdp_connect()
fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail else ""))
    if not cond:
        fails.append(name)


# Clean state: close the settings modal if a prior script left it open.
ws.eval("void (document.getElementById('settingsModal') && !document.getElementById('settingsModal').hidden && document.getElementById('settingsClose').click())")

QUESTION = "In one sentence, what does a DCF model estimate?"

# Type into the real composer + click the real Send button.
setv = ws.eval(
    "(() => { const ta = document.getElementById('chatInput');"
    " ta.value = " + repr(QUESTION) + ";"
    " ta.dispatchEvent(new Event('input')); return ta.value.length; })()"
)
check("composer accepts text", isinstance(setv, (int, float)) and setv > 0, str(setv))
click = ws.eval("void document.getElementById('chatSend').click()")
check("Send click (no JS error)", js_ok(click), str(click))

# 1. Streaming begins (agent_send fired -> setStreaming(true) disables input).
started = wait_js(ws, "document.getElementById('chatInput').disabled === true", timeout=20)
check("run starts (streaming state entered)", started)

# 2. Run reaches a terminal state (streaming ends -> input re-enabled) within 120s.
done = wait_js(ws, "document.getElementById('chatInput').disabled === false", timeout=120)
check("run reaches terminal (streaming ends)", done)

# 3. A real assistant answer rendered. The app opens a NEW conversation and swaps
#    the view, so assert on content: the transcript shows the question AND a
#    distinct, substantive assistant answer beyond it.
txt = ws.eval("(document.getElementById('chatScroll')||document.body).innerText") or ""
answer = txt.replace(QUESTION, "").strip()
check(
    "assistant answer rendered (distinct from question)",
    len(answer) >= 20,
    "answer_len=%d" % len(answer),
)

# 4. The turn persisted as a conversation with a non-empty answer preview.
import json as _json
conv = ws.eval("window.__TAURI__.core.invoke('list_conversations')")
preview = ""
if js_ok(conv) and isinstance(conv, str):
    try:
        rows = _json.loads(conv)
        preview = (rows[0].get("preview") or "") if rows else ""
    except Exception:
        pass
check("turn persisted with answer preview", len(preview.strip()) >= 20, "preview=%r" % preview[:80])

# 5. No error banner surfaced.
err = ws.eval(
    "(() => { const e = document.querySelector('.status.error, .error-banner, .chat-error');"
    " return e ? e.textContent.trim().slice(0,200) : ''; })()"
)
check("no error banner", not (isinstance(err, str) and err), str(err))

print("")
if fails:
    print("LIVE TURN FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("LIVE TURN PASS — full agent pipeline ran end-to-end with this session's edits")
ws.close()
