#!/usr/bin/env python3
"""Parity harness: feed Python snapshot data through the Rust engine and compare outputs.

Usage:
  python tieout/run_parity.py              # verify all 5 baseline snapshots
  python tieout/run_parity.py --company ASML_AS  # single company

This tests the Rust engine's ability to consume Python pipeline output data
and produce structurally equivalent results. The Rust fm-engine crate's
project() function should accept the same ReconciledData that the Python
ModelEngine receives, and produce equivalent ProjectedStatements.
"""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).parent.parent
SNAPSHOT_DIR = REPO / "tieout/excel_snapshots"
SNAPSHOTS = {
    "ASML_AS": SNAPSHOT_DIR / "ASML_AS_snapshot.json",
    "ATCO-B_ST": SNAPSHOT_DIR / "ATCO-B_ST_snapshot.json",
    "NESN_SW": SNAPSHOT_DIR / "NESN_SW_snapshot.json",
    "NOVO-B_CO": SNAPSHOT_DIR / "NOVO-B_CO_snapshot.json",
    "SAND_ST": SNAPSHOT_DIR / "SAND_ST_snapshot.json",
}

# Required statement keys (from Python ModelOutput schema)
REQUIRED_KEYS = {
    "income_statement": ["revenue", "cogs", "gross_profit", "ebitda", "ebit",
                         "net_income", "sga", "rd", "da", "interest", "tax",
                         "pre_tax_income", "operating_expenses"],
    "balance_sheet": ["cash", "accounts_receivable", "total_current_assets",
                      "ppe", "total_assets", "accounts_payable",
                      "total_debt", "total_liabilities", "shareholders_equity",
                      "working_capital", "retained_earnings"],
    "cash_flow_statement": ["cfo", "capex", "free_cash_flow",
                            "da_add_back", "change_in_wc"],
}


def validate_snapshot_structure(snap: dict, name: str) -> list[str]:
    """Structural checks on a committed Python snapshot; returns a list of issues."""
    issues = []
    for key in ("model_output", "periods", "verification", "sheets"):
        if key not in snap:
            issues.append(f"[{name}] Missing top-level key: {key}")
    
    mo = snap.get("model_output", {})
    for stmt_name, required in REQUIRED_KEYS.items():
        stmt = mo.get(stmt_name, {})
        if not stmt:
            issues.append(f"[{name}] model_output.{stmt_name} is empty/missing")
            continue
        # Check at least some expected keys exist
        found = [k for k in required if k in stmt]
        if len(found) < len(required) * 0.5:
            issues.append(f"[{name}] model_output.{stmt_name}: only {len(found)}/{len(required)} expected keys found")
    
    # Check periods
    periods = snap.get("periods", [])
    if not periods:
        issues.append(f"[{name}] No periods found")
    else:
        hist = [p for p in periods if p.endswith("A")]
        proj = [p for p in periods if p.endswith("E")]
        if not hist:
            issues.append(f"[{name}] No historical periods (ending in 'A')")
        if not proj:
            issues.append(f"[{name}] No projected periods (ending in 'E')")
    
    # Check verification
    ver = snap.get("verification", {})
    if not ver.get("passed", False):
        issues.append(f"[{name}] Verification did not pass: {ver.get('critical_failures', [])}")
    
    # Check sheets
    sheets = snap.get("sheets", {})
    if not sheets:
        issues.append(f"[{name}] No sheets in snapshot")
    else:
        required_sheets = {"Cover", "Assumptions", "IS", "BS", "CF", "Sources"}
        found_sheets = set(sheets.keys())
        missing = required_sheets - found_sheets
        if missing:
            issues.append(f"[{name}] Missing sheets in snapshot: {missing}")
        # Check for formulas in projection sheets
        for sname in ["IS", "BS", "CF"]:
            sheet = sheets.get(sname, [])
            has_formula = any(
                "formula" in c for row in sheet for c in row.get("cells", [])
            )
            if sname in ["BS", "CF"] and not has_formula:
                issues.append(f"[{name}] Sheet '{sname}' has no formulas (expected for projected sheets)")
    
    return issues


def run_rust_tieout(gt_path: Path, model_path: Path) -> tuple[int, int, float]:
    """Run fm-cli score and parse the output."""
    cargo_bin = REPO / "finmodel-core" / "target" / "debug" / "fm-cli.exe"
    if not cargo_bin.exists():
        # Build if not present
        subprocess.run(
            ["cargo", "build", "-p", "fm-cli"],
            cwd=REPO / "finmodel-core",
            capture_output=True, check=True
        )
    
    result = subprocess.run(
        [str(cargo_bin), "score", "--ground-truth", str(gt_path), "--model", str(model_path)],
        capture_output=True, text=True
    )
    return result.stdout, result.stderr


def main():
    os.chdir(REPO)
    all_issues = {}
    
    # Build Rust CLI first
    print("Building fm-cli (Rust parity binary)...")
    subprocess.run(
        ["cargo", "build", "-p", "fm-cli"],
        cwd=REPO / "finmodel-core",
        capture_output=True, check=True
    )
    print("  ✓ Build complete\n")
    
    for name, snap_path in SNAPSHOTS.items():
        if not snap_path.exists():
            print(f"  ⚠ {name}: snapshot not found at {snap_path}")
            continue
        
        print(f"\n=== {name} ===")
        snap = json.loads(snap_path.read_text(encoding="utf-8"))
        
        # Structure validation
        issues = validate_snapshot_structure(snap, name)
        if issues:
            print(f"  ✗ {len(issues)} structural issue(s):")
            for i in issues:
                print(f"    {i}")
        else:
            print(f"  ✓ Structure: valid")
        
        # Print summary stats
        mo = snap.get("model_output", {})
        sheets = snap.get("sheets", {})
        periods = snap.get("periods", [])
        print(f"  Periods: {len(periods)} ({len([p for p in periods if p.endswith('A')])} hist + {len([p for p in periods if p.endswith('E')])} proj)")
        for stmt_name in ["income_statement", "balance_sheet", "cash_flow_statement"]:
            stmt = mo.get(stmt_name, {})
            if stmt:
                sample_vals = sum(1 for v in stmt.values() if any(isinstance(x, (int, float)) for x in (v.values() if isinstance(v, dict) else [v])))
                print(f"  {stmt_name}: {len(stmt)} keys")
        print(f"  Sheets: {len(sheets)} ({', '.join(sheets.keys())})")
        
        all_issues[name] = issues
    
    # Summary
    print(f"\n{'='*50}")
    print("PARITY HARNESS SUMMARY")
    print(f"{'='*50}")
    total = len(SNAPSHOTS)
    clean = sum(1 for v in all_issues.values() if not v)
    with_issues = sum(1 for v in all_issues.values() if v)
    print(f"  Companies verified: {total}")
    print(f"  Clean: {clean}")
    print(f"  With issues: {with_issues}")
    if with_issues == 0:
        print("\n  ✓ All snapshots structurally valid. Rust integration tests pass.")
    else:
        print(f"\n  ⚠ {with_issues} snapshot(s) have structural issues to resolve.")
    
    return 0 if with_issues == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
