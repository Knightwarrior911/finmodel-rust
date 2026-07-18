# -*- coding: utf-8 -*-
"""s3_memory_edit.py — verify 7.2 memory edit end-to-end in the running app.

Creates a memory via a real 'remember:' turn, edits it through the real bridge,
confirms the change persisted, and checks the Edit affordance renders.
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

# 1. Create a memory via a real 'remember:' turn (manual capture path).
MEM = "I prefer concise one-paragraph answers"
ws.eval(
    "(() => { const ta=document.getElementById('chatInput');"
    " ta.value=" + repr("remember: " + MEM) + ";"
    " ta.dispatchEvent(new Event('input')); })()"
)
ws.eval("void document.getElementById('chatSend').click()")
wait_js(ws, "document.getElementById('chatInput').disabled === true", timeout=20)
done = wait_js(ws, "document.getElementById('chatInput').disabled === false", timeout=120)
check("remember turn completes", done)

# 2. memory_list shows the new memory (live bridge).
ml = ws.eval("window.__TAURI__.core.invoke('memory_list')")
rows = json.loads(ml) if (js_ok(ml) and isinstance(ml, str)) else []
mem = next((r for r in rows if MEM.lower() in (r.get("content") or "").lower()), None)
check("memory created + listed", mem is not None, str([r.get("content") for r in rows])[:200])

if mem:
    mid = mem["id"]
    # 3. Edit it through the real bridge.
    NEW = "I prefer detailed answers with sources"
    ed = ws.eval(
        "window.__TAURI__.core.invoke('memory_edit', {id: %d, value: %s})" % (mid, repr(NEW))
    )
    check("memory_edit IPC ok", js_ok(ed), str(ed))
    # 4. Confirm the edit persisted.
    ml2 = ws.eval("window.__TAURI__.core.invoke('memory_list')")
    rows2 = json.loads(ml2) if (js_ok(ml2) and isinstance(ml2, str)) else []
    edited = next((r for r in rows2 if r.get("id") == mid), None)
    check(
        "edit persisted (content changed)",
        edited is not None and edited.get("content") == NEW,
        str(edited),
    )

# 5. The Edit affordance renders in the settings memory list.
ws.eval("void document.getElementById('settingsBtn').click()")
wait_js(ws, "!document.getElementById('settingsModal').hidden", timeout=10)
has_edit = wait_js(ws, "!!document.querySelector('#memoryList button[data-edit]')", timeout=10)
check("Edit button renders in memory list", has_edit)

print("")
if fails:
    print("MEMORY EDIT FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("MEMORY EDIT PASS — create -> edit -> persist verified live end-to-end")
ws.close()
