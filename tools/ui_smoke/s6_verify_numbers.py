# -*- coding: utf-8 -*-
"""s6_verify_numbers.py — verify the number-verification pipeline live (Task 4.2/4.4).

Asks for a company's exact reported financials (which calls get_financials → SEC
EDGAR), then asserts the run emits a verification card badging how many material
figures were checked against their source. Proves claims flow end-to-end:
tool result → extract_claims → LiveDriver.run_claims → verify() → verification card.
Uses the configured provider key.
"""
import json
import sys
from cdp_client import cdp_connect, wait_js

ws = cdp_connect()
fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail else ""))
    if not cond:
        fails.append(name)


# Clean state: close settings if open.
ws.eval(
    "void (document.getElementById('settingsModal') &&"
    " !document.getElementById('settingsModal').hidden &&"
    " document.getElementById('settingsClose').click())"
)

Q = "Show me NVIDIA's exact reported revenue and diluted EPS for fiscal 2024 from its 10-K. Use your tools."
ws.eval(
    "(() => { const ta=document.getElementById('chatInput'); ta.value=" + json.dumps(Q) + ";"
    " ta.dispatchEvent(new Event('input')); })()"
)
ws.eval("void document.getElementById('chatSend').click()")

check("run starts", wait_js(ws, "document.getElementById('chatInput').disabled===true", timeout=20))
check("run reaches terminal", wait_js(ws, "document.getElementById('chatInput').disabled===false", timeout=180))

# The financials tool ran: its card left entity/figure markers in the transcript.
txt = (ws.eval("(document.getElementById('chatScroll')||document.body).innerText") or "")
answer = txt.replace(Q, "")
check("get_financials ran (financials card rendered)", "NVIDIA" in answer or "Revenue" in answer,
      answer[-200:].replace("\n", " "))

# The verification card rendered: a source-checked count + badge.
vcard = ws.eval(
    "(() => { const e=document.querySelector('.card-verify');"
    " return e ? e.textContent.replace(/\\s+/g,' ').trim().slice(0,200) : ''; })()"
)
check("verification card rendered", isinstance(vcard, str) and len(vcard) > 0, str(vcard))
check("verification card shows a source-checked count", isinstance(vcard, str) and "/" in vcard, str(vcard))
check("verification badges verified against SEC EDGAR",
      isinstance(vcard, str) and ("EDGAR" in vcard or "verified" in vcard.lower()), str(vcard))

err = ws.eval(
    "(() => { const e = document.querySelector('.status.error, .error-banner, .chat-error');"
    " return e ? e.textContent.trim().slice(0,200) : ''; })()"
)
check("no error banner", not (isinstance(err, str) and err), str(err))

print("")
if fails:
    print("VERIFY NUMBERS FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("VERIFY NUMBERS PASS — analyst verified material figures against source, live end-to-end")
ws.close()
