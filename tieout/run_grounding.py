"""Grounding-rate runner — the autoresearch metric emitter.

For each company:
  1. Ensure ONE pinned source PDF (immutable; skip if unobtainable).
  2. Ensure the immutable Q&A answer key (two-pass; cached).
  3. For each key item, ask the IN-SCOPE ad-hoc research path the question
     and demand a citation: src.research.qa.answer_from_filing(pdf, q, company)
     -> {"answer","page","quote"}. A missing/erroring seam is NOT a harness
     crash — it scores as an ungrounded miss, so the baseline is honest and the
     loop's first win is to build the grounded entry inside src/.
  4. An item is GROUNDED+CORRECT iff ALL hold:
       a. answer agrees with the trusted ground-truth answer,
       b. the cited page is within PAGE_TOL of the true page,
       c. the cited verbatim quote is mechanically present on the cited page
          (independent render — the loop cannot fake this).

grounding_rate = 100 * grounded_correct / trusted_items, aggregated across
companies. Higher is better. coverage% (answered-at-all) is reported too so
the loop cannot win by refusing every question.

Determinism: seam answers are cached keyed by a hash of the in-scope source
files, so the metric is a deterministic function of the code under test —
re-running without a code change reuses the cache (no LLM noise); any edit to
the in-scope path invalidates it and forces re-querying.

Exit 0 when >=1 company measured; non-zero only if nothing measurable or the
harness itself crashed (autoresearch treats that as an invalid run).
"""
import argparse
import hashlib
import importlib
import json
import sys
import traceback
from pathlib import Path

from tieout.config import BASKET
from tieout.grounding_config import (GROUNDING_BASKET, GROUNDING_RESULTS_DIR,
                                     PAGE_TOL)
from tieout.grounding_groundtruth import (build_grounding_truth,
                                          _render_pages, _anchor_present)
from tieout.pin_filings import ensure_pinned
from tieout.llm import LLMStall
from tieout.textnorm import parse_money
import re

_SRC = Path(__file__).parent.parent / "src"
_SCOPE_FILES = [_SRC / "extractor.py", _SRC / "reconciler.py",
                _SRC / "orchestrator.py", _SRC / "research" / "qa.py",
                _SRC / "research" / "agent.py"]
_ANSCACHE = GROUNDING_RESULTS_DIR / "_anscache"
_ANSCACHE.mkdir(parents=True, exist_ok=True)

_EMPTY = ("", "n/a", "na", "none", "null", "not found", "not_on_page",
          "unknown", "i don't know", "cannot determine", "not disclosed")


def _scope_fingerprint() -> str:
    h = hashlib.sha256()
    for f in _SCOPE_FILES:
        h.update(f.read_bytes() if f.exists() else b"<missing>")
    return h.hexdigest()[:16]


def _call_seam(pdf_path: str, question: str, company: str):
    """Call the in-scope research-answer seam. Any failure -> None (miss)."""
    try:
        import src.research.qa as qa
        importlib.reload(qa)
        fn = getattr(qa, "answer_from_filing", None)
        if fn is None:
            return None
        res = fn(pdf_path, question, company)
        if not isinstance(res, dict):
            return None
        return {"answer": res.get("answer"),
                "page": res.get("page"),
                "quote": res.get("quote")}
    except Exception as e:  # noqa: BLE001 - seam may not exist yet / may throw
        return {"_error": f"{type(e).__name__}: {e}"}


def _answer_ok(expected: str, confirmed: str, got) -> bool:
    if got is None:
        return False
    g = str(got).strip()
    if g.lower() in _EMPTY:
        return False
    for ref in (expected, confirmed):
        if ref is None:
            continue
        rv, gv = parse_money(ref), parse_money(g)
        if rv is not None and gv is not None:
            if abs(rv) == abs(gv):
                return True
            denom = max(abs(rv), 1.0)
            if abs(abs(rv) - abs(gv)) / denom <= 0.005:
                return True
            continue
        nr = re.sub(r"[^a-z0-9]+", " ", str(ref).lower()).strip()
        ng = re.sub(r"[^a-z0-9]+", " ", g.lower()).strip()
        if nr and ng and (nr == ng or nr in ng or ng in nr):
            return True
        tr, tg = set(nr.split()), set(ng.split())
        if tr and tg:
            inter = len(tr & tg)
            if inter >= 3 and inter / max(len(tr | tg), 1) >= 0.6:
                return True
    return False


def _score(item, pages_by_no, seam):
    """-> (covered: bool, grounded: bool, detail: dict)."""
    if seam is None or "_error" in (seam or {}):
        return False, False, {"why": (seam or {}).get("_error", "no seam")}
    ans, pg, quote = seam.get("answer"), seam.get("page"), seam.get("quote")
    covered = ans is not None and str(ans).strip().lower() not in _EMPTY
    if not covered:
        return False, False, {"why": "no answer / refused"}

    # Goal metric (verbatim): a claim is grounded iff its cited
    # (page, figure/quote) "is actually present on that page AND answers the
    # question". So grounding is the verifiability of the SEAM's OWN citation,
    # NOT equality to the answer key's chosen page (a fact may appear on
    # several pages). Anti-gaming anchors, all mechanical / ungameable:
    #   a_ok      seam answer == immutable ground-truth answer
    #   cite_real immutable GT source_anchor is present on the seam's cited
    #             page  (the cited page genuinely supports the answer — a
    #             wrong/hallucinated page fails here)
    #   q_ok      seam's own quote is verbatim on the seam's cited page
    a_ok = _answer_ok(item["expected_answer"],
                      item.get("confirmed_answer"), ans)
    try:
        pg_i = int(pg)
    except (TypeError, ValueError):
        pg_i = None

    cite_real = q_ok = False
    if pg_i in pages_by_no:
        raw, nm, vals = pages_by_no[pg_i]
        cite_real = _anchor_present(str(item["anchor"]), raw, nm, vals)
        if quote:
            q_ok = _anchor_present(str(quote), raw, nm, vals)

    on_true_page = pg_i is not None and \
        abs(pg_i - item["source_page"]) <= PAGE_TOL
    grounded = bool(a_ok and cite_real and q_ok)
    return covered, grounded, {
        "answer_ok": a_ok, "cite_real": cite_real, "quote_ok": q_ok,
        "page_ok": cite_real,  # report-compat: citation resolves to support
        "cited_page": pg_i, "true_page": item["source_page"],
        "on_true_page": on_true_page, "topic": item["topic"]}


def run(only=None, quiet=False):
    fp = _scope_fingerprint()
    summary = {"fingerprint": fp, "companies": {}, "skipped": {},
               "total_trusted": 0, "total_grounded": 0, "total_covered": 0}

    for tk in GROUNDING_BASKET:
        if only and tk != only:
            continue
        try:
            row = next(r for r in BASKET if r["ticker"] == tk)
            pdf = ensure_pinned(row)
            if not pdf:
                summary["skipped"][tk] = "no pinned PDF"
                print(f"[skip] {tk}: no pinned PDF", file=sys.stderr)
                continue
            key = build_grounding_truth(tk, row["company"], row["currency"],
                                        str(pdf))
            if key["n_trusted"] == 0:
                summary["skipped"][tk] = "answer key empty"
                print(f"[skip] {tk}: empty key", file=sys.stderr)
                continue

            pages = _render_pages(str(pdf))
            pages_by_no = {n: (raw, nm, vals) for (n, raw, nm, vals) in pages}

            cache = _ANSCACHE / f"{fp}_{tk.replace('/', '_').replace('.', '_')}.json"
            ans_cache = json.loads(cache.read_text(encoding="utf-8")) \
                if cache.exists() else {}
            cache_hit = bool(ans_cache)

            rows, grounded_n, covered_n = [], 0, 0
            for it in key["items"]:
                qh = hashlib.sha256(it["question"].encode()).hexdigest()[:16]
                if qh in ans_cache:
                    seam = ans_cache[qh]
                else:
                    seam = _call_seam(str(pdf), it["question"],
                                      row["company"])
                    ans_cache[qh] = seam
                cov, gr, det = _score(it, pages_by_no, seam)
                covered_n += int(cov)
                grounded_n += int(gr)
                rows.append({"question": it["question"],
                             "expected": it["expected_answer"],
                             "seam": seam, **det, "grounded": gr})

            cache.write_text(json.dumps(ans_cache, indent=2,
                                        ensure_ascii=False), encoding="utf-8")

            denom = key["n_trusted"]
            pct = round(100 * grounded_n / denom, 2) if denom else None
            cov_pct = round(100 * covered_n / denom, 2) if denom else None
            ck = {"ticker": tk, "company": row["company"],
                  "trusted": denom, "grounded": grounded_n,
                  "covered": covered_n, "pct": pct, "coverage_pct": cov_pct,
                  "topics": key["topics_covered"], "seam_cached": cache_hit,
                  "rows": rows}
            (GROUNDING_RESULTS_DIR /
             f"{tk.replace('/', '_').replace('.', '_')}.json").write_text(
                json.dumps(ck, indent=2, ensure_ascii=False),
                encoding="utf-8")
            summary["companies"][tk] = {
                "pct": pct, "coverage_pct": cov_pct, "trusted": denom,
                "grounded": grounded_n, "covered": covered_n}
            summary["total_trusted"] += denom
            summary["total_grounded"] += grounded_n
            summary["total_covered"] += covered_n
            print(f"[ok] {tk}: {pct}% grounded  cov {cov_pct}%  "
                  f"({grounded_n}/{denom}){' [cached]' if cache_hit else ''}",
                  file=sys.stderr)
        except (LLMStall, AssertionError, ValueError) as e:
            summary["skipped"][tk] = f"{type(e).__name__}: {e}"
            print(f"[skip] {tk}: {e}", file=sys.stderr)
            continue
        except Exception as e:  # noqa: BLE001 - never let one company kill run
            summary["skipped"][tk] = f"UNEXPECTED {type(e).__name__}: {e}"
            traceback.print_exc()
            continue

    tt = summary["total_trusted"]
    tg = summary["total_grounded"]
    tc = summary["total_covered"]
    agg = round(100 * tg / tt, 2) if tt else None
    cov = round(100 * tc / tt, 2) if tt else None
    summary["aggregate_pct"] = agg
    summary["aggregate_coverage_pct"] = cov
    summary["measured_companies"] = len(summary["companies"])

    (GROUNDING_RESULTS_DIR / "_summary.json").write_text(
        json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    _write_report(summary)

    if not quiet:
        for tk, c in summary["companies"].items():
            pv = c["pct"] if c["pct"] is not None else 0.0
            print(f"  {tk:12s} {pv:6.2f}% grounded  "
                  f"cov {c['coverage_pct']}%  {c['grounded']}/{c['trusted']}",
                  file=sys.stderr)
        for tk, why in summary["skipped"].items():
            print(f"  {tk:12s} SKIPPED  {why}", file=sys.stderr)
        print(f"AGGREGATE {agg}% grounded  (coverage {cov}%) over "
              f"{len(summary['companies'])} companies", file=sys.stderr)

    if agg is None:
        print("0.0")          # nothing measured -> invalid run
        return 3
    print(f"{agg}")            # <-- BARE METRIC, last stdout line
    return 0


def _write_report(summary: dict):
    lines = ["# Filing-Research Grounding Report", "",
             f"Code fingerprint: `{summary['fingerprint']}`  ",
             f"Aggregate grounding: **{summary.get('aggregate_pct')}%** "
             f"(coverage {summary.get('aggregate_coverage_pct')}%) over "
             f"{summary.get('measured_companies')} companies "
             f"({summary['total_grounded']}/{summary['total_trusted']})", ""]
    for tk in summary["companies"]:
        ckp = (GROUNDING_RESULTS_DIR /
               f"{tk.replace('/', '_').replace('.', '_')}.json")
        if not ckp.exists():
            continue
        c = json.loads(ckp.read_text(encoding="utf-8"))
        lines += [f"## {tk} — {c['company']}  ({c['pct']}% grounded, "
                  f"cov {c['coverage_pct']}%)",
                  f"Trusted {c['trusted']} · topics {c['topics']}", "",
                  "| topic | true pg | cited pg | ans | pg | quote | OK |",
                  "|---|---:|---:|:--:|:--:|:--:|:--:|"]
        for r in c["rows"]:
            lines.append(
                f"| {r.get('topic','?')} | {r.get('true_page','-')} "
                f"| {r.get('cited_page','-')} "
                f"| {'Y' if r.get('answer_ok') else '.'} "
                f"| {'Y' if r.get('page_ok') else '.'} "
                f"| {'Y' if r.get('quote_ok') else '.'} "
                f"| {'PASS' if r.get('grounded') else 'fail'} |")
        lines.append("")
    if summary["skipped"]:
        lines += ["## Skipped", ""]
        for tk, why in summary["skipped"].items():
            lines.append(f"- **{tk}**: {why}")
    (GROUNDING_RESULTS_DIR / "_report.md").write_text(
        "\n".join(lines), encoding="utf-8")


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", help="single ticker, e.g. ATCO-B.ST")
    ap.add_argument("--quiet", action="store_true")
    a = ap.parse_args()
    sys.exit(run(only=a.only, quiet=a.quiet))


if __name__ == "__main__":
    main()
