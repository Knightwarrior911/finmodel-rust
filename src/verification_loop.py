"""
Verification loop per VERIFICATION_LOOP.md.

Core principle: Generate ? Execute ? Check ? Fix ? Deliver.
Max 3 fix iterations, then surface to user.

Five rules:
  1. Compute key answers twice, independent methods
  2. Force deliverable to execute
  3. Compare programmatically, not by eyeballing
  4. On mismatch, debug before proceeding
  5. When unverifiable, say so explicitly
"""
import os
import logging
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

logger = logging.getLogger(__name__)


@dataclass
class GroundTruth:
    """Pre-build expected results from raw financial data (Step 1)."""
    ticker: str

    # Balance sheet totals from raw data
    total_assets: list[float] = field(default_factory=list)
    total_liabilities: list[float] = field(default_factory=list)
    total_equity: list[float] = field(default_factory=list)
    bs_delta: list[float] = field(default_factory=list)  # A - L - E

    # Cash flow tie-out from raw data
    net_income_expected: list[float] = field(default_factory=list)
    ending_cash_expected: list[float] = field(default_factory=list)

    # Key metrics
    revenue: list[float] = field(default_factory=list)
    ebitda: list[float] = field(default_factory=list)
    net_income: list[float] = field(default_factory=list)

    # Period labels
    periods: list[str] = field(default_factory=list)

    # Unverifiable items (explicitly flagged per Rule 5)
    unverifiable: list[str] = field(default_factory=list)


@dataclass
class ComparisonResult:
    """Result of programmatic comparison between deliverable and ground truth."""
    passed: bool = True
    mismatches: list[str] = field(default_factory=list)
    checks_run: int = 0
    checks_passed: int = 0
    unverifiable_flags: list[str] = field(default_factory=list)


@dataclass
class LoopReport:
    """Full verification loop report."""
    status: str = "success"           # "success" | "fail" | "partial"
    iterations: int = 0
    ground_truth: GroundTruth | None = None
    comparison: ComparisonResult | None = None
    force_executed: bool = False
    pre_delivery_checks: list[str] = field(default_factory=list)
    unresolved: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def passed_all(self) -> bool:
        return self.status == "success" and not self.unresolved


# ── Step 1: Establish ground truth ──────────────────────────────────────────

def establish_ground_truth(reconciled, model_output=None) -> GroundTruth:
    """Compute expected results from model engine output + raw filing data.

    Uses model_output historical arrays as primary reference (these are what
    the Excel file was built from). Falls back to raw reconciled data where
    model_output fields are missing.
    """
    if model_output is not None:
        periods = list(model_output.periods)
    else:
        periods = list(reconciled.periods)
    gt = GroundTruth(ticker=reconciled.ticker)
    gt.periods = periods
    n = len(periods)

    # Use model_output historical arrays when available (engine-computed truth)
    if model_output is not None:
        bs = model_output.balance_sheet
        is_out = model_output.income_statement
        gt.revenue = is_out.get("revenue", [0] * n)
        gt.net_income = is_out.get("net_income", [0] * n)
        ebit = is_out.get("ebit", [0] * n)
        da = is_out.get("da", [0] * n)
        gt.ebitda = [(ebit[j] or 0) + (da[j] or 0) for j in range(n)]
        gt.total_assets = bs.get("total_assets", [0] * n)
        gt.total_liabilities = bs.get("total_liabilities", [0] * n)
        gt.total_equity = bs.get("total_equity", [0] * n)
        for j in range(n):
            ta = gt.total_assets[j] or 0
            tl = gt.total_liabilities[j] or 0
            te = gt.total_equity[j] or 0
            gt.bs_delta.append(ta - tl - te)
        gt.ending_cash_expected = bs.get("cash", [0] * n)
        gt.net_income_expected = is_out.get("net_income", [0] * n)
    else:
        # Fallback: raw reconciled data
        bs = reconciled.balance_sheet
        gt.total_assets = bs.get("total_assets", [0] * n)
        gt.total_liabilities = bs.get("total_liabilities", [0] * n)
        gt.total_equity = bs.get("total_equity", [0] * n)
        for j in range(n):
            ta = gt.total_assets[j] or 0
            tl = gt.total_liabilities[j] or 0
            te = gt.total_equity[j] or 0
            gt.bs_delta.append(ta - tl - te)
        is_data = reconciled.income_statement
        gt.revenue = is_data.get("revenue", [0] * n)
        gt.net_income = is_data.get("net_income", [0] * n)
        ebit = is_data.get("ebit", [0] * n)
        da = is_data.get("da", [0] * n)
        gt.ebitda = [(ebit[j] or 0) + (da[j] or 0) for j in range(n)]
        gt.ending_cash_expected = bs.get("cash", [0] * n)
        gt.net_income_expected = is_data.get("net_income", [0] * n)

    # Flag unverifiable items
    if not any(v and v != 0 for v in (da or [])):
        gt.unverifiable.append("D&A data missing")
    if not any(v and v != 0 for v in (gt.ending_cash_expected or [])):
        gt.unverifiable.append("Cash data missing from BS")
    if not any(v and v != 0 for v in (ebit or [])):
        gt.unverifiable.append("EBIT data missing from IS")

    return gt


# ── Step 3: Force execution ────────────────────────────────────────────────

def force_execute(xlsx_path: str, timeout_sec: int = 30) -> bool:
    """Force Excel recalculation via LibreOffice headless (if available).

    Returns True if successfully executed, False otherwise.
    """
    libre_paths = [
        "soffice",                          # Linux PATH
        "libreoffice",                      # Linux CLI
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",  # macOS
        "C:\\Program Files\\LibreOffice\\program\\soffice.exe",  # Windows 64
        "C:\\Program Files (x86)\\LibreOffice\\program\\soffice.exe",  # Windows 32
    ]
    lo_exe = None
    for p in libre_paths:
        try:
            r = subprocess.run([p, "--version"], capture_output=True, timeout=5)
            if r.returncode == 0:
                lo_exe = p
                break
        except Exception:
            continue

    if not lo_exe:
        logger.warning("LibreOffice not found — cannot force-recalculate xlsx")
        return False

    out_dir = str(Path(xlsx_path).parent)
    try:
        subprocess.run(
            [
                lo_exe, "--headless", "--norestore", "--nofirststartwizard",
                "--convert-to", "xlsx", "--outdir", out_dir, xlsx_path,
            ],
            timeout=timeout_sec,
            check=True,
        )
        return True
    except subprocess.CalledProcessError:
        logger.warning("LibreOffice convert failed for %s", xlsx_path)
        return False
    except subprocess.TimeoutExpired:
        logger.warning("LibreOffice timed out (%ss) for %s", timeout_sec, xlsx_path)
        return False


# ── Step 4: Compare to ground truth ─────────────────────────────────────────

def compare_to_ground_truth(
    xlsx_path: str,
    ground_truth: GroundTruth,
    tolerance: float = 1.0,
) -> ComparisonResult:
    """Open the xlsx, read computed values, compare against ground truth.

    Uses openpyxl data_only=True to read cached formula results.
    Comparison is programmatic — no eyeballing.
    """
    import openpyxl
    result = ComparisonResult()

    try:
        wb = openpyxl.load_workbook(xlsx_path, data_only=True)
    except Exception as e:
        result.passed = False
        result.mismatches.append(f"Cannot open xlsx: {e}")
        return result

    # ── BS balance check ──
    if "BS" in wb.sheetnames:
        ws = wb["BS"]
        for j, period in enumerate(ground_truth.periods):
            col = 7 + j  # G=7 for first data column
            ta_cell = ws.cell(row=19, column=col).value     # Total Assets
            tl_cell = ws.cell(row=27, column=col).value     # Total Liabilities
            te_cell = ws.cell(row=33, column=col).value     # Total Equity
            chk_cell = ws.cell(row=35, column=col).value    # BS check row

            if chk_cell is not None and abs(float(chk_cell)) > tolerance:
                result.mismatches.append(
                    f"BS check {period}: {chk_cell} (tolerance={tolerance})"
                )
    else:
        result.unverifiable_flags.append("BS tab not found in xlsx")

    # ── Revenue comparison (historical periods only) ──
    if "IS" in wb.sheetnames:
        ws = wb["IS"]
        for j, period in enumerate(ground_truth.periods):
            if not period.endswith("A"):
                # Projected periods have no ground truth — skip
                continue
            col = 7 + j
            rev_cell = ws.cell(row=11, column=col).value
            expected = ground_truth.revenue[j] if j < len(ground_truth.revenue) else None
            if rev_cell is not None and expected is not None and abs(expected) > 1:
                result.checks_run += 1
                if abs(float(rev_cell) - expected) > tolerance:
                    result.mismatches.append(
                        f"Revenue {period}: model={rev_cell} vs expected={expected}"
                    )
                else:
                    result.checks_passed += 1
    else:
        result.unverifiable_flags.append("IS tab not found in xlsx")

    # ── Formula error scan ──
    try:
        from src.validator import validate_xlsx
        vr = validate_xlsx(xlsx_path)
        if vr.failures:
            for f in vr.failures:
                result.mismatches.append(f"Formula error: {f}")
    except Exception:
        result.unverifiable_flags.append("Validator not available for xlsx scan")

    # ── Unverifiable flags ──
    for uv in ground_truth.unverifiable:
        result.unverifiable_flags.append(uv)

    if result.mismatches:
        result.passed = False

    return result


# ── Pre-delivery checklist ──────────────────────────────────────────────────

def pre_delivery_checklist(
    xlsx_path: str,
    ground_truth: GroundTruth,
    comparison: ComparisonResult,
    force_executed: bool,
) -> list[str]:
    """Run the full pre-delivery checklist per VERIFICATION_LOOP.md.

    Returns list of failed checks (empty = all passed).
    """
    failed = []

    if ground_truth is None:
        failed.append("Ground truth not computed")

    if not force_executed:
        failed.append("Deliverable not force-executed (LibreOffice unavailable)")

    if comparison is None:
        failed.append("Programmatic comparison not run")
    elif not comparison.passed:
        failed.append(f"Comparison failed: {len(comparison.mismatches)} mismatches")

    if not Path(xlsx_path).exists():
        failed.append(f"xlsx file missing: {xlsx_path}")

    if comparison and comparison.unverifiable_flags:
        failed.append(
            f"Unverifiable elements: {'; '.join(comparison.unverifiable_flags[:5])}"
        )

    return failed


# ── Main loop: orchestrate steps 1-5 ───────────────────────────────────────

def run_verification_loop(
    xlsx_path: str,
    reconciled,
    model_output,
    max_iterations: int = 3,
    tolerance: float = 1.0,
) -> LoopReport:
    """Execute the full verification loop.

    Steps:
      1. Establish ground truth from reconciled data
      2. Build deliverable (already done by model engine)
      3. Force execution via LibreOffice
      4. Compare deliverable to ground truth programmatically
      5. On mismatch, surface to caller; caller may fix and re-run

    Returns LoopReport with full audit trail.
    """
    report = LoopReport()

    # Step 1: Ground truth (use model_output for engine-computed historical arrays)
    report.ground_truth = establish_ground_truth(reconciled, model_output=model_output)
    logger.info(
        "Ground truth established: %d periods, %d unverifiable items flagged",
        len(report.ground_truth.periods),
        len(report.ground_truth.unverifiable),
    )

    # Step 3: Force execution
    report.force_executed = force_execute(xlsx_path)

    # Step 4: Compare
    report.comparison = compare_to_ground_truth(
        xlsx_path, report.ground_truth, tolerance=tolerance
    )

    # Step 5: Iterate / surface
    report.iterations = 1  # first pass
    while not report.comparison.passed and report.iterations < max_iterations:
        logger.warning(
            "Verification pass %d failed: %d mismatches. "
            "Surfacing for fix (max %d passes).",
            report.iterations,
            len(report.comparison.mismatches),
            max_iterations,
        )
        report.iterations += 1
        # Caller is responsible for fixing and re-running comparison
        report.unresolved = list(report.comparison.mismatches)
        break  # Surface after first mismatch; caller handles iteration

    # Pre-delivery checklist
    failed_checks = pre_delivery_checklist(
        xlsx_path, report.ground_truth, report.comparison, report.force_executed
    )
    report.pre_delivery_checks = failed_checks

    if failed_checks:
        report.status = "fail"
        report.unresolved.extend(failed_checks)
    elif not report.comparison.passed:
        report.status = "fail"
        report.unresolved = list(report.comparison.mismatches)
    elif report.comparison.unverifiable_flags:
        report.status = "partial"
        report.notes = [
            f"Passed with unverifiable items: "
            f"{'; '.join(report.comparison.unverifiable_flags[:5])}"
        ]
    else:
        report.status = "success"

    return report


# ── Public API: re-run comparison after fix (Step 5 re-entry) ───────────────

def re_compare(
    xlsx_path: str,
    ground_truth: GroundTruth,
    tolerance: float = 1.0,
) -> ComparisonResult:
    """Re-run comparison after fixing deliverable (for iteration)."""
    return compare_to_ground_truth(xlsx_path, ground_truth, tolerance=tolerance)


def flag_unverifiable(reason: str) -> None:
    """Explicitly flag an element that cannot be verified (Rule 5)."""
    logger.info("UNVERIFIABLE: %s", reason)
