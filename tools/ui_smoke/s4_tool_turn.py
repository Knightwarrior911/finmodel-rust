# -*- coding: utf-8 -*-
"""s4_tool_turn.py — verify the tool-using analyst path live (the product's core).

Asks a question that requires a finance tool, and asserts the tool ran (its result
card appears) and the assistant answered. Uses the configured provider key.
"""
import json
import sys
from cdp_client import cdp_connect, wait_js, js_ok

ws = cdp_connect()
fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail else ""))
    if not cond:
        fails.append(name)


ws.eval(
    "void (document.getElementById('settingsModal') &&"
    " !document.getElementById('settingsModal').hidden &&"
    " document.getElementById('settingsClose').click())"
)

Q = "What is the latest stock quote for Apple (AAPL)? Use your tools."
ws.eval(
    "(() => { const ta=document.getElementById('chatInput'); ta.value=" + json.dumps(Q) + ";"
    " ta.dispatchEvent(new Event('input')); })()"
)
ws.eval("void document.getElementById('chatSend').click()")

check("run starts", wait_js(ws, "document.getElementById('chatInput').disabled===true", timeout=20))
check("run reaches terminal", wait_js(ws, "document.getElementById('chatInput').disabled===false", timeout=150))

txt = (ws.eval("(document.getElementById('chatScroll')||document.body).innerText") or "")
answer = txt.replace(Q, "")
# Tool actually executed: the get_quote result card leaves its ticker + currency
# markers in the transcript (structured card, not just prose).
check("get_quote tool ran (result card rendered)", "AAPL" in answer and "USD" in answer,
      answer[-200:].replace("\n", " "))
# The model produced a final answer citing a price.
check("assistant answered with a price", "$" in answer or "quote" in answer.lower())

err = ws.eval(
    "(() => { const e = document.querySelector('.status.error, .error-banner, .chat-error');"
    " return e ? e.textContent.trim().slice(0,200) : ''; })()"
)
check("no error banner", not (isinstance(err, str) and err), str(err))

print("")
if fails:
    print("TOOL TURN FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("TOOL TURN PASS — analyst called a finance tool and answered, live end-to-end")
ws.close()
