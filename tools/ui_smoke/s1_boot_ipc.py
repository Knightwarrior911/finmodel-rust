# -*- coding: utf-8 -*-
"""s1_boot_ipc.py — live smoke over CDP: app boot + real IPC bridge + 1.5/7.2 UI.

Non-destructive: only reads (load_settings) and opens the settings dialog; never
writes the user's real settings. Run with the app already on :9222.
"""
import json
import sys
from cdp_client import cdp_connect, wait_js, js_ok

ws = cdp_connect()
fails = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail else ""))
    if not cond:
        fails.append(name + (": " + detail if detail else ""))


# 1. App shell actually rendered in the real webview.
shell = ws.eval_json(
    "JSON.stringify({app: !!document.getElementById('app'),"
    " btn: !!document.getElementById('settingsBtn'), title: document.title})"
)
check("app shell renders", bool(shell) and shell.get("app") and shell.get("btn"), str(shell))

# 2. Real IPC: load_settings round-trips through the actual Rust bridge (1.5 backend live).
ls = ws.eval("window.__TAURI__.core.invoke('load_settings')")
if not js_ok(ls):
    check("load_settings IPC", False, str(ls))
else:
    try:
        obj = json.loads(ls)
    except Exception:
        obj = None
    check("load_settings returns JSON", obj is not None, str(ls)[:200])
    if obj is not None:
        print("     keys:", sorted(obj.keys()))
        check("load_settings exposes model_profiles (1.5 backend)", "model_profiles" in obj)

# 3. Open settings via the real button; role-profile picker inputs exist (1.5 UI live).
r = ws.eval("void document.getElementById('settingsBtn').click()")
check("settingsBtn click (no JS error)", js_ok(r), str(r))
opened = wait_js(ws, "!document.getElementById('settingsModal').hidden")
check("settings modal opens", opened)
picker = ws.eval_json(
    "JSON.stringify({worker: !!document.getElementById('workerModel'),"
    " verifier: !!document.getElementById('verifierModel'),"
    " cred: !!document.getElementById('workerCredentialRef')})"
)
check(
    "role-profile picker inputs render (1.5 UI)",
    bool(picker) and picker.get("worker") and picker.get("verifier") and picker.get("cred"),
    str(picker),
)

# 4. Skills + memory management containers render (7.2 UI + their bridge calls).
lists = ws.eval_json(
    "JSON.stringify({skills: !!document.getElementById('skillsList'),"
    " mem: !!document.getElementById('memoryList')})"
)
check(
    "skills + memory lists render (7.2 UI)",
    bool(lists) and lists.get("skills") and lists.get("mem"),
    str(lists),
)

print("")
if fails:
    print("SMOKE FAIL (%d):" % len(fails))
    for f in fails:
        print("  -", f)
    ws.close()
    sys.exit(1)
print("SMOKE PASS — app boots, IPC bridge works, 1.5 picker + 7.2 lists render live")
ws.close()
