"""
Output validator (per shared/SPEC_spreadsheet_engineering Section 6).

Checks:
  - Font colors match cell type (blue inputs / black local / green cross-sheet)
  - Two-hop violations (Assumptions! mixed with arithmetic) — FAIL
  - Formula errors (#REF!, #VALUE!, #DIV/0!, #NAME?)  — FAIL
  - Blue cells missing citation comments               — warning
  - Color mismatches                                   — warning

Returns ValidationReport: list of failures + warnings + cell stats.
"""
from dataclasses import dataclass, field


@dataclass
class ValidationReport:
    status: str = "success"            # "success" | "fail"
    failures: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    counts: dict = field(default_factory=dict)


_FORMULA_ERRORS = {"#REF!", "#VALUE!", "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!"}

# RGB hex codes for spec font colors (case-insensitive comparisons)
_BLUE  = {"FF0000FF", "0000FF"}
_BLACK = {"FF000000", "000000", "FF0F1632", "0F1632", "FF595959", "595959"}   # ink + label gray
_GREEN = {"FF008000", "008000"}
_WHITE = {"FFFFFFFF", "FFFFFF"}   # total/highlight rows — exempt from color rules

# Tabs that are documentation / metadata, not financial calculations
_SKIP_COLOR_SHEETS = {"Sources", "Cover"}


def _normalize_color(c) -> str | None:
    if c is None:
        return None
    val = getattr(c, "rgb", None) or c
    if isinstance(val, str):
        return val.upper().lstrip("#")
    return None


def _is_blue(color: str | None) -> bool:
    return color is not None and color in _BLUE


def _is_black(color: str | None) -> bool:
    return color is not None and color in _BLACK


def _is_green(color: str | None) -> bool:
    return color is not None and color in _GREEN


def _is_two_hop(formula: str) -> bool:
    """Return True if formula mixes Assumptions! cross-ref with arithmetic."""
    if "Assumptions!" not in formula:
        return False
    has_arith = any(op in formula for op in ["*", "+", "-", "/"]) and len(formula) > 1
    if not has_arith:
        return False
    cleaned = formula[1:].strip()
    # Pure lookup: =Assumptions!X1  (no arithmetic after the ref)
    if cleaned.startswith("Assumptions!"):
        after = cleaned[len("Assumptions!"):]
        if not any(op in after for op in ["*", "+", "/"]):
            return False
    # Multiple Assumptions! refs or arithmetic mixed in
    return True


def validate_xlsx(path: str) -> ValidationReport:
    """Open xlsx and run spec checks. Returns ValidationReport."""
    import openpyxl
    wb = openpyxl.load_workbook(path, data_only=False)
    rpt = ValidationReport()
    blue = black = green = formulas = errors = 0
    missing_comments = 0

    for sheet in wb.sheetnames:
        ws = wb[sheet]
        skip_color = sheet in _SKIP_COLOR_SHEETS

        # Build comment set for this sheet
        commented_cells: set[str] = set()
        for row in ws.iter_rows(values_only=False):
            for cell in row:
                if getattr(cell, "comment", None) is not None:
                    commented_cells.add(cell.coordinate)

        for row in ws.iter_rows(values_only=False):
            for cell in row:
                v = cell.value
                if v is None:
                    continue
                font_color = _normalize_color(getattr(cell.font, "color", None))
                is_formula = isinstance(v, str) and v.startswith("=")

                # Formula errors → FAIL (even on skipped sheets)
                if isinstance(v, str) and v.upper() in _FORMULA_ERRORS:
                    errors += 1
                    rpt.failures.append(
                        f"[{sheet}] {cell.coordinate}: formula error '{v}'"
                    )
                    continue

                # White font = total/highlight row — exempt from color rules
                if font_color in _WHITE:
                    if is_formula:
                        formulas += 1
                    continue

                # Documentation/metadata sheets — skip color and comment checks
                if skip_color:
                    continue

                if is_formula:
                    formulas += 1
                    has_xref = "!" in v

                    # Two-hop violation → FAIL (blocking per SPEC_spreadsheet_engineering §4)
                    if _is_two_hop(v):
                        rpt.failures.append(
                            f"[{sheet}] {cell.coordinate}: two-hop violation "
                            f"(Assumptions! in arithmetic): {v[:80]}"
                        )

                    # Color expectation
                    if has_xref and not _is_green(font_color):
                        rpt.warnings.append(
                            f"[{sheet}] {cell.coordinate}: cross-sheet formula not green"
                        )
                        green += 1
                    elif has_xref:
                        green += 1
                    elif not _is_black(font_color):
                        rpt.warnings.append(
                            f"[{sheet}] {cell.coordinate}: same-sheet formula not black"
                        )
                        black += 1
                    else:
                        black += 1

                elif isinstance(v, (int, float)):
                    if _is_blue(font_color):
                        blue += 1
                        # Citation comment required on every blue (hardcoded) input
                        if cell.coordinate not in commented_cells:
                            missing_comments += 1
                    else:
                        # Hardcoded number not blue → warning only (may be label/header numeric)
                        rpt.warnings.append(
                            f"[{sheet}] {cell.coordinate}: hardcoded number not blue"
                        )

    if missing_comments:
        rpt.warnings.append(
            f"Citation comments missing on {missing_comments} blue input cell(s). "
            "Per SPEC_spreadsheet_engineering §5 every hardcoded input requires "
            "a cell comment with cite:{{citationId}}."
        )

    rpt.counts = {
        "blue_inputs": blue, "black_formulas": black, "green_xrefs": green,
        "total_formulas": formulas, "formula_errors": errors,
        "missing_comments": missing_comments,
        "warnings": len(rpt.warnings), "failures": len(rpt.failures),
    }
    if rpt.failures:
        rpt.status = "fail"
    return rpt
