//! End-to-end drive of the pure agent reducer for a NON-USA earnings review
//! (Nestlé, `NESN.SW`), with the operator standing in for the LLM/driver
//! (own-LLM substitution — no provider key or network needed).
//!
//! Exercises the golden `earnings_review` workflow the way the Tauri driver
//! would: plan → parallel read-only tool batch → auto-run immutable memo →
//! export approval gate → synthesis → verification → memory → clean terminal.
//! Also asserts the deterministic workflow router classifies real non-USA
//! analyst asks correctly (it is keyword-based and jurisdiction-agnostic).
//!
//! Run:  cargo run -p fm-agent --example nonusa_agentic_drive

use fm_agent::types::PartKind;
use fm_agent::workflows::{select_workflow, workflow};
use fm_agent::{
    Action, AgentMachine, AgentPhase, ApprovalResponse, EventKind, Input, Policy, Risk, ToolCall,
};

/// A model-requested tool call after the driver's pre-classification. Read-only
/// and new-version writes auto-run; overwrite/export/delete/local-read park.
fn tc(id: &str, name: &str, risk: Risk) -> ToolCall {
    ToolCall {
        tool_call_id: id.to_string(),
        name: name.to_string(),
        risk,
        needs_approval: !risk.auto_runs(),
        args_valid: true,
    }
}

fn main() {
    // ── 1. Router is jurisdiction-agnostic: real non-USA analyst asks ──────
    let router_cases = [
        (
            "NESN.SW just reported — beat/miss versus the prior period, cited variance table.",
            "earnings_review",
        ),
        (
            "Trading comps for the European semicap peers ASML, ASMI, BESI — EV/EBITDA and P/E.",
            "trading_comps",
        ),
        (
            "Quick DCF on Nestlé, base/bull/bear; give me the workbook.",
            "dcf_model",
        ),
        (
            "M&A deal screen: European medtech, min size $1bn, announced since 2023.",
            "ma_screen",
        ),
        (
            "Write me a company brief on Novo Nordisk's diabetes franchise.",
            "company_brief",
        ),
        (
            "Prep the board deck for the LVMH / Tiffany-style acquisition.",
            "pitch_prep",
        ),
    ];
    println!("── Workflow router on non-USA asks ──");
    for (msg, want) in router_cases {
        let got = select_workflow(msg).unwrap_or("<none>");
        println!("  {want:<15} <= {msg}");
        assert_eq!(got, want, "router misfired on {msg:?} -> {got}");
    }

    // ── 2. Golden earnings_review, driven for a non-USA name ──────────────
    let spec = workflow("earnings_review").expect("earnings_review spec");
    assert!(spec.golden, "earnings_review is a golden fixture");
    let plan = spec.initial_plan("Nestlé (NESN.SW) FY-24 earnings review");
    println!("\n── Visible plan ({} steps) ──", plan.steps.len());
    for s in &plan.steps {
        println!("  [{}] {}", s.id, s.label);
    }
    assert!(!plan.is_empty(), "planning must emit a non-empty plan");

    let show = |label: &str, a: &Action| println!("  {label:<27} -> {a:?}");

    let mut m = AgentMachine::new(Policy::WORKFLOW);
    assert_eq!(m.start(), Action::Prepare);

    // Numeric-finance workflow → plan + verification required.
    let a = m.next(Input::Prepared {
        uses_tools: true,
        plan_needed: true,
        needs_verification: true,
    });
    show("Prepared", &a);
    assert_eq!(a, Action::MakePlan);
    let a = m.next(Input::PlanReady);
    show("PlanReady", &a);
    assert_eq!(a, Action::RequestModel);

    // LLM turn 1: fan out the four required read-only tools in one batch.
    let reads = vec![
        tc("t1", "list_filings", Risk::ReadOnly),
        tc("t2", "read_filing", Risk::ReadOnly),
        tc("t3", "get_news", Risk::ReadOnly),
        tc("t4", "get_quote", Risk::ReadOnly),
    ];
    let a = m.next(Input::ModelResponded {
        calls: reads,
        final_answer: false,
        tokens: 1500,
    });
    show("ModelResponded(4 reads)", &a);
    assert_eq!(
        a,
        Action::ScheduleTools {
            batch: vec!["t1".into(), "t2".into(), "t3".into(), "t4".into()]
        },
        "independent read-only calls must schedule as one parallel batch"
    );
    let a = m.next(Input::ToolsCompleted { tokens: 800 });
    show("ToolsCompleted", &a);
    assert_eq!(a, Action::RequestModel);

    // LLM turn 2: draft the earnings note as a NEW immutable version (auto-runs).
    let a = m.next(Input::ModelResponded {
        calls: vec![tc("t5", "draft_memo", Risk::LocalCreate)],
        final_answer: false,
        tokens: 600,
    });
    show("ModelResponded(draft_memo)", &a);
    assert_eq!(
        a,
        Action::ScheduleTools {
            batch: vec!["t5".into()]
        },
        "a new immutable version (LocalCreate) auto-runs — no approval"
    );
    let a = m.next(Input::ToolsCompleted { tokens: 200 });
    show("ToolsCompleted", &a);
    assert_eq!(a, Action::RequestModel);

    // LLM turn 3: export the note outside the output root → approval gate.
    let a = m.next(Input::ModelResponded {
        calls: vec![tc("t6", "export_memo", Risk::Export)],
        final_answer: false,
        tokens: 300,
    });
    show("ModelResponded(export)", &a);
    assert_eq!(
        a,
        Action::RequestApproval {
            tool_call_id: "t6".into()
        },
        "an export must park for approval"
    );
    assert_eq!(m.phase(), AgentPhase::AwaitingApproval);
    let a = m.next(Input::ApprovalResolved {
        response: ApprovalResponse::ApproveOnce,
    });
    show("ApprovalResolved(approve)", &a);
    assert_eq!(
        a,
        Action::ScheduleTools {
            batch: vec!["t6".into()]
        },
        "approve-once executes exactly the approved call"
    );
    let a = m.next(Input::ToolsCompleted { tokens: 100 });
    show("ToolsCompleted", &a);
    assert_eq!(a, Action::RequestModel);

    // LLM turn 4: final answer, no further tools → synthesize.
    let a = m.next(Input::ModelResponded {
        calls: vec![],
        final_answer: true,
        tokens: 500,
    });
    show("ModelResponded(final)", &a);
    assert_eq!(a, Action::Synthesize);
    let a = m.next(Input::Synthesized);
    show("Synthesized", &a);
    assert_eq!(a, Action::Verify, "numeric turn must verify before finishing");
    let a = m.next(Input::Verified { ok: true });
    show("Verified(ok)", &a);
    assert_eq!(a, Action::ExtractMemory);
    let a = m.next(Input::MemoryDone);
    show("MemoryDone", &a);
    match a {
        Action::Emit {
            event,
            stop,
            partial,
        } => {
            println!("\n  TERMINAL: event={event:?} stop={stop:?} partial={partial}");
            assert_eq!(event, EventKind::RunCompleted);
            assert!(!partial, "a verified, in-budget run is not partial");
        }
        other => panic!("expected terminal Emit, got {other:?}"),
    }
    assert!(m.phase().is_terminal(), "run must end in a terminal phase");

    // Completion gate: earnings_review requires a Result table + Sources.
    assert!(
        spec.is_complete(&[PartKind::Result, PartKind::Sources]),
        "both expected parts present ⇒ complete"
    );
    assert!(
        !spec.is_complete(&[PartKind::Result]),
        "missing Sources ⇒ not complete"
    );

    println!("\n✓ non-USA agentic drive passed all assertions");
}
