"""Tie-out runner — the autoresearch metric emitter.

For each company:
  1. Ensure ONE pinned source PDF (immutable; skip company if unobtainable).
  2. Ensure immutable ground truth (two-pass + regex anchors; cached).
  3. Run the IN-SCOPE extraction code on the SAME pinned PDF
     (src.extractor.extract_financials_from_pdf with ticker="" -> cache
     bypassed, so the loop's edits actually execute).
  4. Compare model output vs ground truth, per (statement, key, year),
     EXACT integer at reporting unit (abs() for outflow-magnitude keys).

Metric = 100 * (matched cells) / (ground-truth-trusted cells), aggregated
across every company that produced a measurement. Higher is better; 100 = the
model reproduces every historical face-statement number in all 3 statements.

The model extraction is cached keyed by a hash of the in-scope source files,
so the metric is a DETERMINISTIC function of the code under test: re-running
verify without a code change reuses the cached extraction (fast, no LLM noise);
any edit to the extraction path invalidates the cache and forces re-extraction.

Resilience: per-company checkpoint, retry on claude stall, skip-and-continue.
Exit 0 when >=1 company measured; non-zero only if nothing could be measured
or the harness itself crashed (so autoresearch treats that as an invalid run,
not a real metric).
"""
import argparse
import hashlib
import json
import sys
import traceback
from pathlib import Path

from tieout.config import (BASKET, CANONICAL_BY_SECTOR, ABS_KEYS_BY_SECTOR,
                           EXCLUDE_KEYS_BY_SECTOR, RESULTS_DIR,
                           ticker_filings_dir)
from tieout.groundtruth import build_ground_truth
from tieout.pin_filings import ensure_pinned
from tieout.llm import LLMStall

_SRC = Path(__file__).parent.parent / "src"
_SCOPE_FILES = [_SRC / "extractor.py", _SRC / "reconciler.py",
                _SRC / "fetcher.py"]
_MODELCACHE = RESULTS_DIR / "_modelcache"
_MODELCACHE.mkdir(parents=True, exist_ok=True)


def _scope_fingerprint() -> str:
    # Normalize CRLF->LF so the fingerprint is identical on Windows (working
    # tree may be CRLF under core.autocrlf) and Linux/CI (LF blobs). Without
    # this the guard's fingerprint check fails cross-platform.
    h = hashlib.sha256()
    for f in _SCOPE_FILES:
        data = f.read_bytes() if f.exists() else b"<missing>"
        h.update(data.replace(b"\r\n", b"\n"))
    return h.hexdigest()[:16]


def _model_extract(ticker: str, pdf_path: str, years, fp: str, retries: int):
    """Run in-scope extractor on the pinned PDF; cache by code fingerprint."""
    safe = ticker.replace("/", "_").replace(".", "_")
    cache = _MODELCACHE / f"{fp}_{safe}.json"
    if cache.exists():
        return json.loads(cache.read_text(encoding="utf-8")), True

    periods = [f"{y}A" for y in years]  # oldest-first, aligned to gt years
    last = ""
    for attempt in range(retries + 1):
        try:
            # Imported INSIDE the loop so each iteration picks up the loop's
            # latest edits to the in-scope module.
            import importlib
            import src.extractor as ex
            importlib.reload(ex)
            is_d, bs_d, cfs_d, _notes, yf = ex.extract_financials_from_pdf(
                pdf_path, periods, ticker="")  # ticker="" => bypass cache
            res = {"income_statement": is_d, "balance_sheet": bs_d,
                   "cash_flow_statement": cfs_d, "years_found": yf}
            cache.write_text(json.dumps(res, indent=2, ensure_ascii=False),
                             encoding="utf-8")
            return res, False
        except Exception as e:  # noqa: BLE001
            last = f"{type(e).__name__}: {e}"
            print(f"  [model] attempt {attempt + 1} failed: {last}",
                  file=sys.stderr)
    raise LLMStall(f"model extraction failed: {last}")


def _norm(key, v, abs_keys):
    if v is None:
        return None
    try:
        f = float(v)
    except (TypeError, ValueError):
        return None
    if key in abs_keys:
        f = abs(f)
    return round(f)


def _compare(gt: dict, model: dict):
    years = gt["years"]
    sector = gt.get("sector", "industrial")
    canonical = CANONICAL_BY_SECTOR[sector]
    abs_keys = ABS_KEYS_BY_SECTOR[sector]
    exclude_keys = EXCLUDE_KEYS_BY_SECTOR[sector]
    rows, denom, matched = [], 0, 0
    per_stmt = {}
    for stmt, keys in canonical.items():
        s_d = s_m = 0
        gvals = gt["values"].get(stmt, {})
        mvals = model.get(stmt, {}) or {}
        for key in keys:
            if key in exclude_keys:
                continue
            gk = gvals.get(key, {})
            if not gk:
                continue
            mlist = mvals.get(key)
            for y in years:
                gv = gk.get(str(y))
                if gv is None:
                    continue
                denom += 1
                s_d += 1
                mv = None
                if isinstance(mlist, list):
                    idx = years.index(y)
                    if idx < len(mlist):
                        mv = _norm(key, mlist[idx], abs_keys)
                ok = (mv is not None and mv == int(gv))
                if ok:
                    matched += 1
                    s_m += 1
                else:
                    rows.append({
                        "statement": stmt, "key": key, "year": y,
                        "ground_truth": int(gv), "model": mv,
                        "page": gt.get("citations", {}).get(stmt),
                    })
        per_stmt[stmt] = {"trusted": s_d, "matched": s_m,
                          "pct": round(100 * s_m / s_d, 2) if s_d else None}
    pct = round(100 * matched / denom, 2) if denom else None
    return pct, denom, matched, per_stmt, rows


def run(only=None, retries=2, quiet=False):
    fp = _scope_fingerprint()
    summary = {"fingerprint": fp, "companies": {}, "skipped": {},
               "total_trusted": 0, "total_matched": 0}
    for row in BASKET:
        tk = row["ticker"]
        if only and tk != only:
            continue
        try:
            pdf = ensure_pinned(row)
            if not pdf:
                summary["skipped"][tk] = "no pinned PDF (discovery failed)"
                print(f"[skip] {tk}: no pinned PDF", file=sys.stderr)
                continue
            gt = build_ground_truth(tk, row["company"], row["currency"],
                                    str(pdf), sector=row.get("sector", "industrial"),
                                    start_page=row.get("gt_start_page", 0))
            if gt["coverage"]["trusted"] == 0:
                # Ground truth could not be established (e.g. unusual report
                # layout). Skip rather than emit a meaningless 0/0 — it must
                # not poison the aggregate or crash the summary.
                summary["skipped"][tk] = "ground truth empty (0 trusted cells)"
                print(f"[skip] {tk}: ground truth empty", file=sys.stderr)
                continue
            model, cached = _model_extract(tk, str(pdf), gt["years"], fp,
                                           retries)
            pct, denom, matched, per_stmt, rows = _compare(gt, model)
            ck = {
                "ticker": tk, "company": row["company"],
                "years": gt["years"],
                "currency_reported": gt.get("currency_reported"),
                "pct": pct, "trusted": denom, "matched": matched,
                "per_statement": per_stmt,
                "gt_unverifiable": gt["coverage"]["unverifiable"],
                "model_cached": cached, "mismatches": rows,
            }
            (RESULTS_DIR / f"{tk.replace('/', '_').replace('.', '_')}.json"
             ).write_text(json.dumps(ck, indent=2, ensure_ascii=False),
                          encoding="utf-8")
            summary["companies"][tk] = {
                "pct": pct, "trusted": denom, "matched": matched,
                "years": gt["years"], "per_statement": per_stmt}
            summary["total_trusted"] += denom
            summary["total_matched"] += matched
            print(f"[ok] {tk}: {pct}%  ({matched}/{denom})"
                  f"{' [cached]' if cached else ''}", file=sys.stderr)
        except (LLMStall, AssertionError, ValueError) as e:
            summary["skipped"][tk] = f"{type(e).__name__}: {e}"
            print(f"[skip] {tk}: {e}", file=sys.stderr)
            continue
        except Exception as e:  # noqa: BLE001 - never let one company kill run
            summary["skipped"][tk] = f"UNEXPECTED {type(e).__name__}: {e}"
            traceback.print_exc()
            continue

    tt, tm = summary["total_trusted"], summary["total_matched"]
    agg = round(100 * tm / tt, 2) if tt else None
    summary["aggregate_pct"] = agg
    summary["measured_companies"] = len(summary["companies"])

    # Don't overwrite _summary.json when zero companies were measured — a
    # failed run (e.g. missing LLM key) must not clobber the good summary that
    # the no-regression guard test relies on.
    # Only clobber _summary.json when we actually measured something.
    if summary["measured_companies"] > 0:
        (RESULTS_DIR / "_summary.json").write_text(
            json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    _write_report(summary)


    if not quiet:
        for tk, c in summary["companies"].items():
            pv = c["pct"] if c["pct"] is not None else 0.0
            print(f"  {tk:12s} {pv:6.2f}%  {c['matched']}/{c['trusted']}",
                  file=sys.stderr)
        for tk, why in summary["skipped"].items():
            print(f"  {tk:12s} SKIPPED  {why}", file=sys.stderr)
        print(f"AGGREGATE {agg}%  over {len(summary['companies'])} companies",
              file=sys.stderr)

    if agg is None:
        print("0.0")          # nothing measured -> invalid run
        return 3
    print(f"{agg}")            # <-- BARE METRIC, last stdout line
    return 0


def _write_report(summary: dict):
    lines = ["# Filing Tie-Out Report",
             "",
             f"Code fingerprint: `{summary['fingerprint']}`  ",
             f"Aggregate: **{summary.get('aggregate_pct')}%** over "
             f"{summary.get('measured_companies')} companies "
             f"({summary['total_matched']}/{summary['total_trusted']} cells)",
             ""]
    for tk in summary["companies"]:
        ckp = RESULTS_DIR / f"{tk.replace('/', '_').replace('.', '_')}.json"
        if not ckp.exists():
            continue
        c = json.loads(ckp.read_text(encoding="utf-8"))
        lines += [f"## {tk} — {c['company']}  ({c['pct']}%)",
                  f"Years {c['years']} · reported {c.get('currency_reported')}"
                  f" · GT unverifiable {c['gt_unverifiable']}", ""]
        for s, v in c["per_statement"].items():
            lines.append(f"- {s}: {v['matched']}/{v['trusted']} "
                         f"({v['pct']}%)")
        if c["mismatches"]:
            lines += ["", "| statement | line | year | filing | model | pg |",
                      "|---|---|---|---:|---:|---:|"]
            for m in c["mismatches"][:120]:
                lines.append(
                    f"| {m['statement']} | {m['key']} | {m['year']} "
                    f"| {m['ground_truth']} | {m['model']} | {m['page']} |")
        lines.append("")
    if summary["skipped"]:
        lines += ["## Skipped", ""]
        for tk, why in summary["skipped"].items():
            lines.append(f"- **{tk}**: {why}")
    (RESULTS_DIR / "_report.md").write_text("\n".join(lines),
                                             encoding="utf-8")


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", help="single ticker, e.g. ATCO-B.ST")
    ap.add_argument("--retries", type=int, default=2)
    ap.add_argument("--quiet", action="store_true")
    a = ap.parse_args()
    sys.exit(run(only=a.only, retries=a.retries, quiet=a.quiet))


if __name__ == "__main__":
    main()
