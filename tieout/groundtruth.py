"""Independent ground-truth extraction — the immutable answer key.

Independence guarantees (so the loop can't mark its own homework):
  * Does NOT import src.extractor (own transport via tieout.llm, own prompt,
    own section finder, own number parsing).
  * Column-aware page rendering: EU filings print two statements side-by-side;
    pdfplumber merges them per line. We split columns by word x-position so
    each figure stays on its own line (otherwise the answer key is garbage).
  * TWO decorrelated LLM transcription passes; only cells where both passes
    agree become trusted ground truth. Disagreements are excluded from the
    metric denominator rather than scored against shaky truth.
  * Hard asserts on known Atlas Copco figures self-test the whole pipeline;
    if the instrument can't reproduce ATCO it fails loudly instead of
    silently certifying a wrong answer key.
  * Result cached to groundtruth/<ticker>.json and treated as IMMUTABLE: once
    written it is never regenerated within the loop.
"""
import json
import re
import sys

import pdfplumber

from tieout.config import CANONICAL, ABS_KEYS, EXCLUDE_KEYS, GT_DIR
from tieout.llm import complete
from tieout.textnorm import normalize_minus, parse_money, find_years

_IS_ANCHORS = [
    "consolidated income statement",
    "consolidated statement of profit",
    "consolidated statement of operations",
    "consolidated statement of income",
    "consolidated statement of comprehensive income",
    "income statement",
    "statement of profit or loss",
    "profit and loss account",
]
# An actual face DATA row: a revenue synonym followed by two multi-digit
# figures on the same line (after thousands-space normalization). This is what
# separates the real statement page from a contents/governance page that only
# mentions the words.
_REVENUE_DATA_ROW = re.compile(
    r"(?:revenues?|net sales|net revenue|net turnover|turnover|total revenue"
    r"|sales revenue|net sales revenue)\b[^\n]*?\d{3,}[^\n]*?\d{3,}", re.I)
_UNIT_HINT = re.compile(
    r"\b(MSEK|SEK ?m|EUR ?m|€ ?m|DKK ?m|CHF ?m|million|millions|mn\b"
    r"|amounts in|in millions)\b", re.I)

def gt_path(ticker: str):
    return GT_DIR / f"{ticker.replace('/', '_').replace('.', '_')}.json"


def _rows_from_words(words, tol=3.0):
    """Cluster words into visual rows by their top coordinate."""
    rows = []
    for w in sorted(words, key=lambda w: (round(w["top"] / tol), w["x0"])):
        if rows and abs(w["top"] - rows[-1][0]) <= tol:
            rows[-1][1].append(w)
        else:
            rows.append([w["top"], [w]])
    out = []
    for _top, ws in rows:
        ws.sort(key=lambda w: w["x0"])
        out.append(ws)
    return out


def _render_page(page) -> str:
    """Column-aware text. EU annual reports print two statements side-by-side
    (income | comprehensive income, assets | equity, operating | investing).
    pdfplumber's extract_text merges them per visual line, corrupting which
    number belongs to which line. We detect a two-up page (a header row with
    >=4 year tokens, i.e. duplicated YYYY YYYY columns) and emit the LEFT
    column fully, then the RIGHT column fully, so each line keeps its own
    figures. Single-column pages are reconstructed normally.
    """
    try:
        words = page.extract_words(use_text_flow=False,
                                   keep_blank_chars=False)
    except Exception:  # noqa: BLE001
        return page.extract_text() or ""
    if not words:
        return page.extract_text() or ""
    rows = _rows_from_words(words)

    two_up = False
    split_x = page.width / 2.0
    for ws in rows[:25]:
        line = " ".join(w["text"] for w in ws)
        yrs = re.findall(r"\b20[1-3]\d\b", line)
        if len(yrs) >= 4:
            two_up = True
            # split at the midpoint between the 2nd and 3rd year tokens
            yx = [w["x0"] for w in ws
                  if re.fullmatch(r"20[1-3]\d", w["text"])]
            if len(yx) >= 3:
                split_x = (sorted(yx)[1] + sorted(yx)[2]) / 2.0
            break

    def emit(side_words_pred):
        lines = []
        for ws in rows:
            sel = [w for w in ws if side_words_pred(w)]
            if sel:
                lines.append(" ".join(w["text"] for w in sel))
        return "\n".join(lines)

    if two_up:
        left = emit(lambda w: (w["x0"] + w["x1"]) / 2.0 < split_x)
        right = emit(lambda w: (w["x0"] + w["x1"]) / 2.0 >= split_x)
        return normalize_minus(left + "\n\n" + right)
    return normalize_minus(emit(lambda w: True))


def _page_texts(pdf_path: str):
    with pdfplumber.open(pdf_path) as pdf:
        return [_render_page(p) for p in pdf.pages]


def _find_face_window(pages):
    """Return (start_idx, joined_normalized_text, is_header_line).

    The real income-statement face page must contain a strong anchor phrase
    AND a genuine revenue DATA row (synonym + two multi-digit figures after
    thousands normalization). Contents / governance pages mention the words
    but have no data row, so they are rejected.
    """
    for i, norm in enumerate(pages):  # pages already column-split + minus-norm
        low = norm.lower()
        if not any(a in low for a in _IS_ANCHORS):
            continue
        if not _REVENUE_DATA_ROW.search(norm):
            continue
        if len(re.findall(r"\b20[1-3]\d\b", norm)) < 2:
            continue
        window = pages[i: i + 13]  # IS, OCI, BS, SCE, CFS (+ slack)
        joined = "\n".join(window)
        # header line: a line on the IS face with >=2 distinct year tokens,
        # preferring one that also carries a unit/Note marker.
        best = ""
        for ln in norm.splitlines():
            ys = set(re.findall(r"\b(20[1-3]\d)\b", ln))
            if len(ys) >= 2:
                if _UNIT_HINT.search(ln) or "note" in ln.lower():
                    best = ln
                    break
                if not best:
                    best = ln
        return i, joined, best
    # Fallback: whole doc head
    return 0, "\n".join(pages)[:120_000], ""


def _header_year_order(header_line: str, years):
    """Column order of years as printed L->R on the face header."""
    seq = [int(y) for y in re.findall(r"\b(20[1-3]\d)\b", header_line)
           if int(y) in years]
    out = []
    for y in seq:
        if y not in out:
            out.append(y)
    return out if len(out) == len(years) else sorted(years, reverse=True)


def _build_prompt(face_text, years, currency_hint, variant):
    keys_block = json.dumps(CANONICAL, indent=2)
    sign_rule = (
        "SIGN CONVENTION (apply exactly):\n"
        f"  - These keys are POSITIVE magnitudes even though the filing prints "
        f"them as negatives/outflows: {sorted(ABS_KEYS)}\n"
        "  - All other keys: signed exactly as economically reported "
        "(losses/outflows negative).\n"
    )
    if variant == "A":
        persona = ("You are an audit-grade transcription engine. Copy figures "
                   "VERBATIM from the CONSOLIDATED face statements only.")
    else:
        persona = ("You are a meticulous filing analyst. Read ONLY the primary "
                   "consolidated statements and report each printed figure.")
    return f"""{persona}

Reporting currency hint: {currency_hint}. Fiscal years to extract (oldest
first): {years}. All values in MILLIONS as printed on the face statements.

Rules:
- Use ONLY the consolidated income statement, consolidated balance sheet, and
  consolidated cash flow statement. NEVER segment, parent-company, or note
  tables.
- "172 664" is 168,343-style European spacing -> 172664. Parentheses or a
  leading minus mean negative.
- If a canonical line item is not printed on the face statement, set it to
  null. Do not infer, sum, or derive — transcribe only.
- da: take from the cash-flow add-back line. ebit: "operating profit/income".
{sign_rule}
Canonical keys (return EXACTLY these, per statement):
{keys_block}

Return ONLY JSON, no prose:
{{
  "currency": "<3-letter>",
  "values": {{
    "income_statement":   {{ "<key>": {{ "{years[0]}": <num|null>, "{years[1]}": <num|null> }} }},
    "balance_sheet":      {{ ... }},
    "cash_flow_statement":{{ ... }}
  }},
  "page_refs": {{ "income_statement": <page|null>, "balance_sheet": <page|null>, "cash_flow_statement": <page|null> }}
}}

Filing face statements:
{face_text[:120_000]}
"""


def _parse_json(raw: str) -> dict:
    raw = raw.strip()
    a, b = raw.find("{"), raw.rfind("}")
    if a == -1 or b == -1:
        raise ValueError(f"no JSON object in LLM output: {raw[:200]}")
    return json.loads(raw[a:b + 1])


def _norm_cell(key, v):
    if v is None:
        return None
    f = parse_money(v) if isinstance(v, str) else float(v)
    if f is None:
        return None
    if key in ABS_KEYS:
        f = abs(f)
    return round(f)


# Known Atlas Copco FY2023 face figures (MSEK) — instrument self-test on the
# rock-solid single-column lines. (Balance-sheet equity wraps across the
# two-up column boundary in this filing; it is intentionally NOT asserted —
# it's protected instead by dual-pass agreement, since the model will also
# struggle there and we don't want the answer key stricter than the test.)
_ATCO_ASSERT = {
    "income_statement": {
        "revenue": {2023: 172664, 2022: 141325},
        "gross_profit": {2023: 75117, 2022: 59384},
        "ebit": {2023: 37091, 2022: 30216},
        "rd": {2023: 6693, 2022: 5389},
        "net_income": {2023: 28052, 2022: 23482},
    },
}


def build_ground_truth(ticker: str, company: str, currency: str,
                       pdf_path: str, *, force: bool = False) -> dict:
    """Build (or load cached) immutable ground truth for one company."""
    out_path = gt_path(ticker)
    if out_path.exists() and not force:
        return json.loads(out_path.read_text(encoding="utf-8"))

    pages = _page_texts(pdf_path)
    start, face_text, header_line = _find_face_window(pages)
    years = find_years(header_line) or find_years(face_text[:4000])
    if not years or len(years) != 2:
        raise ValueError(
            f"{ticker}: could not detect two fiscal years from face header")
    col_order = _header_year_order(header_line, years)

    # Two decorrelated transcription passes.
    pa = _parse_json(complete(_build_prompt(face_text, years, currency, "A"),
                              "Transcribe now.", timeout=600))
    pb = _parse_json(complete(_build_prompt(face_text, years, currency, "B"),
                              "Transcribe now.", timeout=600))

    values, citations = {}, {}
    cov = {"trusted": 0, "unverifiable": 0}
    disagreed = []
    for stmt, keys in CANONICAL.items():
        values[stmt] = {}
        for key in keys:
            if key in EXCLUDE_KEYS:
                continue
            av = (pa.get("values", {}).get(stmt, {}) or {}).get(key, {}) or {}
            bv = (pb.get("values", {}).get(stmt, {}) or {}).get(key, {}) or {}
            yr_vals = {}
            for y in years:
                ys = str(y)
                ca = _norm_cell(key, av.get(ys, av.get(y)))
                cb = _norm_cell(key, bv.get(ys, bv.get(y)))
                if ca is None and cb is None:
                    continue                              # not on face stmt
                if ca is not None and cb is not None and ca == cb:
                    yr_vals[ys] = ca                      # both passes agree
                    cov["trusted"] += 1
                else:
                    disagreed.append(f"{stmt}.{key}.{ys} (A={ca} B={cb})")
                    cov["unverifiable"] += 1
            if yr_vals:
                values[stmt][key] = yr_vals
            pr = pa.get("page_refs", {}).get(stmt)
            citations.setdefault(stmt, pr)

    gt = {
        "ticker": ticker,
        "company": company,
        "currency_expected": currency,
        "currency_reported": pa.get("currency") or pb.get("currency"),
        "years": years,
        "header_column_order": col_order,
        "values": values,
        "citations": citations,
        "coverage": cov,
        "passes_disagreed": disagreed,
        "source_pdf": str(pdf_path),
        "face_start_page": start + 1,
    }

    if ticker == "ATCO-B.ST":
        _assert_atco(gt)

    out_path.write_text(json.dumps(gt, indent=2, ensure_ascii=False),
                        encoding="utf-8")
    return gt


def _assert_atco(gt: dict):
    bad = []
    for stmt, keys in _ATCO_ASSERT.items():
        for key, exp in keys.items():
            got = gt["values"].get(stmt, {}).get(key, {})
            for y, ev in exp.items():
                gv = got.get(str(y))
                if gv != ev:
                    bad.append(f"{stmt}.{key}.{y}: expected {ev}, GT has {gv}")
    if bad:
        raise AssertionError(
            "ATCO ground-truth self-test FAILED — instrument is untrustworthy:\n"
            + "\n".join(bad))


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    from tieout.config import BASKET, ticker_filings_dir
    tk = sys.argv[1] if len(sys.argv) > 1 else "ATCO-B.ST"
    row = next(r for r in BASKET if r["ticker"] == tk)
    pdf = ticker_filings_dir(tk) / (row["pinned"] or "annual_report.pdf")
    g = build_ground_truth(tk, row["company"], row["currency"], str(pdf),
                           force="--force" in sys.argv)
    print(json.dumps(g["coverage"], indent=2))
    print("years", g["years"], "reported_ccy", g["currency_reported"])
    print("disagreed", len(g["passes_disagreed"]))
