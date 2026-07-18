# -*- coding: utf-8 -*-
"""s7_plan_panel.py — verify the live mission Plan panel (Task 2.3 / 3.2).

Drives a workflow-selecting turn (earnings review) and asserts a Plan panel
renders with named steps that reach a done state — proving PlanUpdated events flow
from the actor pump through agent_event into the live UI (not just the transitional
chat_tool path). Uses the configured provider key.
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


ws.eval(
    "void (document.getElementById('settingsModal') &&"
    " !document.getElementById('settingsModal').hidden &&"
    " document.getElementById('settingsClose').click())"
)
# Start a fresh conversation so plan/mission/verify reflect only this run (no
# stale cards from a prior turn in the same conversation).
ws.eval("void document.getElementById('newChatBtn').click()")
import time as _t
_t.sleep(0.3)

Q = "Do an earnings review for NVDA."
ws.eval(
    "(() => { const ta=document.getElementById('chatInput'); ta.value=" + json.dumps(Q) + ";"
    " ta.dispatchEvent(new Event('input')); })()"
)
ws.eval("void document.getElementById('chatSend').click()")

check("run starts", wait_js(ws, "document.getElementById('chatInput').disabled===true", timeout=20))
# The plan panel appears while the mission runs (before terminal).
check(
    "plan panel renders with steps",
    wait_js(ws, "!!document.querySelector('.plan-panel .plan-step')", timeout=60),
    "",
)
# Capture the plan's step labels + statuses.
plan = ws.eval(
    "(() => { const steps=[...document.querySelectorAll('.plan-panel .plan-step')];"
    " return steps.map(s => (s.className.match(/status-(\\w+)/)||[])[1] + ':' +"
    " (s.querySelector('.plan-step-label')||{}).textContent).join(' | '); })()"
)
check("plan has named steps", isinstance(plan, str) and ":" in plan, str(plan))

check("run reaches terminal", wait_js(ws, "document.getElementById('chatInput').disabled===false", timeout=180))

# After completion the plan shows at least one done step (transition-driven).
done_n = ws.eval("document.querySelectorAll('.plan-panel .plan-step.status-done').length")
check("at least one plan step reached done", isinstance(done_n, (int, float)) and done_n >= 1, f"done={done_n}")
# The mission header (Task 2.2) surfaces the workflow + plan progress + verified
# badge, live from the same agent_event stream.
mission = ws.eval(
    "(() => { const h=document.getElementById('missionHeader');"
    " if (!h || h.hidden) return 'HIDDEN';"
    " const t=(s)=>{const e=h.querySelector(s); return e&&!e.hidden?e.textContent.trim():'';};"
    " return JSON.stringify({wf:t('.mission-workflow'), plan:t('.mission-plan'), verify:t('.mission-verify')}); })()"
)
check("mission header shows the workflow", isinstance(mission, str) and "Earnings review" in mission, str(mission))
# The verification badge only appears when the mission fetched a claim-producing
# figure (get_financials); an earnings review may answer from filing text alone.
# Assert consistency: IF a verification card rendered this run, the badge shows.
had_verify = ws.eval(
    "(() => { const cards=[...document.querySelectorAll('.card-verify')];"
    " return cards.length>0; })()"
)
if had_verify:
    check("mission verify badge matches the verification card", isinstance(mission, str) and "Verified" in mission, str(mission))
else:
    print("SKIP mission verify badge — no verification card produced this run (model answered from filing text)")

err = ws.eval(
    "(() => { const e = document.querySelector('.status.error, .error-banner, .chat-error');"
    " return e ? e.textContent.trim().slice(0,200) : ''; })()"
)
check("no error banner", not (isinstance(err, str) and err), str(err))

print("")
print("PLAN:", plan)
if fails:
    print("PLAN PANEL FAIL (%d): %s" % (len(fails), ", ".join(fails)))
    ws.close()
    sys.exit(1)
print("PLAN PANEL PASS — live mission plan renders + progresses via agent_event")
ws.close()
