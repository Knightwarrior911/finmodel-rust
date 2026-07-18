# -*- coding: utf-8 -*-
"""s5_write_approval.py — verify the write-risk approval gate live (Task 4.3).

An MSFT model already exists, so re-building it is an OVERWRITE. Asserts the
approval card appears (the gate now fires), then DENIES and confirms the existing
file is untouched (the fail-closed safety property) and the run ends.
"""
import os
import sys
from cdp_client import cdp_connect, wait_js, js_ok

TARGET = r"C:/Users/vinit/OneDrive/Documents/finmodel/MSFT_model.xlsx"
ws = cdp_connect()
fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail else ""))
    if not cond:
        fails.append(name)


st0 = os.stat(TARGET)
print("existing MSFT model: size=%d mtime=%d" % (st0.st_size, int(st0.st_mtime)))

ws.eval(
    "void (document.getElementById('settingsModal') &&"
    " !document.getElementById('settingsModal').hidden &&"
    " document.getElementById('settingsClose').click())"
)

Q = "Build a DCF model for MSFT (skip the review, build it now)."
ws.eval(
    "(() => { const ta=document.getElementById('chatInput'); ta.value=" + repr(Q) + ";"
    " ta.dispatchEvent(new Event('input')); })()"
)
ws.eval("void document.getElementById('chatSend').click()")
check("run starts", wait_js(ws, "document.getElementById('chatInput').disabled===true", timeout=20))

# The overwrite must surface an approval card BEFORE any write (Task 4.3 gate).
appeared = wait_js(ws, "!!document.querySelector('.part-approval')", timeout=45)
check("approval card appears for overwrite (gate fires)", appeared)

if appeared:
    # Fail-closed: DENY and confirm the file is untouched + the run ends.
    denied = ws.eval(
        "(() => { const box=document.querySelector('.part-approval'); if(!box) return 'no-box';"
        " const btns=[...box.querySelectorAll('button')];"
        " const deny=btns.find(b=>/deny/i.test(b.textContent))||btns[btns.length-1];"
        " if(!deny) return 'no-btn'; deny.click(); return 'clicked'; })()"
    )
    check("deny button clicked", denied == "clicked", str(denied))
    ended = wait_js(ws, "document.getElementById('chatInput').disabled===false", timeout=60)
    check("run ends after deny", ended)
    st1 = os.stat(TARGET)
    check(
        "denied overwrite left the file UNTOUCHED (fail-closed)",
        st1.st_size == st0.st_size and int(st1.st_mtime) == int(st0.st_mtime),
        "size %d->%d mtime %d->%d" % (st0.st_size, st1.st_size, int(st0.st_mtime), int(st1.st_mtime)),
    )

print("")
if fails:
    print("WRITE-APPROVAL FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("WRITE-APPROVAL PASS — overwrite gated on approval; deny left the file intact (4.3 live)")
ws.close()
