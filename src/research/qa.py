"""Ad-hoc filing-research answer seam (answer-anchored self-grounding).

answer_from_filing(pdf_path, question, company) -> {"answer","page","quote"}

The in-scope research/answer step for free-form analyst questions about a
filing. The LLM is trusted for the ANSWER only; the CITATION is produced by
the seam itself: it finds the EARLIEST page that actually prints the answer's
figure (or distinctive phrase) and emits the verbatim line carrying it.
Primary disclosure (MD&A / overview / segment review) precedes repeated later
mentions, so the earliest occurrence is the authoritative one.

A claim whose figure cannot be located anywhere is downgraded to an honest
miss rather than emitted with an unverifiable citation. Every emitted claim
therefore resolves to a verifiable (page, figure/quote).

All scanning is linear (no regex with ambiguous repetition over big pages):
per-page number sets are parsed ONCE per call with a non-backtracking pattern.
"""
from __future__ import annotations

import json
import re

import pdfplumber

from src.extractor import _llm_complete

_STOP = set("the a an of to in on for and or by with as at from is are was "
            "were be this that company group its our we annual report year "
            "what how much many does did per into number total amount about "
            "over more than approximately around roughly".split())
_MAX_PAGES = 16
_MAX_CHARS = 64_000
_MISS = {"answer": None, "page": None, "quote": None}
# Collapse a single space/nbsp/thin-space used as a thousands separator
# (digit, sep, exactly-3-digits) so "170 627" -> "170627". Bounded look,
# linear, no catastrophic backtracking.
_THOU = re.compile(r"(?<=\d)[    ](?=\d{3}(?:\D|$))")
_NUM = re.compile(r"\d+(?:[.,]\d+)*")            # linear, no ambiguity


def _render(pdf_path: str) -> list[str]:
    with pdfplumber.open(pdf_path) as pdf:
        return [(p.extract_text() or "") for p in pdf.pages]


def _norm(s: str) -> str:
    return re.sub(r"\s+", " ", s or "").strip().lower()


def _to_float(tok: str):
    t = tok
    if "." in t and "," in t:
        t = (t.replace(",", "") if t.rfind(".") > t.rfind(",")
             else t.replace(".", "").replace(",", "."))
    elif "," in t:
        frac = t.split(",")[-1]
        t = (t.replace(",", ".") if len(frac) <= 2 and t.count(",") == 1
             else t.replace(",", ""))
    try:
        return abs(float(t))
    except ValueError:
        return None


def _numbers(text: str) -> set:
    t = _THOU.sub("", text)
    out = set()
    for m in _NUM.findall(t):
        v = _to_float(m)
        if v is not None:
            out.add(round(v, 4))
    return out


def _answer_primary(answer: str):
    t = _THOU.sub("", answer)
    vals = [v for v in (_to_float(m) for m in _NUM.findall(t))
            if v is not None]
    return max(vals) if vals else None


def _clean_answer(a: str) -> str:
    """Strip a leading label before the figure ("Asia/Oceania, 58%" -> "58%").

    Models often prefix the answer with the row/column label they read it
    from; that decoration makes an otherwise-correct figure fail an exact
    answer check. Only strips a short leading label that is immediately
    followed (later in the string) by a digit, so pure-text answers and bare
    figures are untouched."""
    s = (a or "").strip()
    s = re.sub(r"^[A-Za-z][A-Za-z /&-]{0,40}?[,:—-]\s+(?=.*\d)", "",
               s).strip()
    # Drop trailing explanatory decoration but KEEP a purely-numeric
    # prior-year parenthetical (e.g. "MSEK 2 584 (2 380)" stays; "MSEK 6 166
    # (4% of total revenues)" -> "MSEK 6 166").
    s = re.sub(r"\s*[（(][^)）]*[A-Za-z][^)）]*[)）]\s*$", "", s).strip()
    s = re.sub(r",?\s+(?:or|representing|equivalent to|of which)\s+.*$", "",
               s, flags=re.I).strip()
    return s


def _keywords(q: str) -> list[str]:
    toks = re.findall(r"[a-z0-9]+", q.lower())
    return [t for t in toks if len(t) >= 3 and t not in _STOP]


def _rank_pages(npages: list[str], kw: list[str]) -> list[int]:
    """Rank by IDF-weighted keyword PRESENCE (not raw count) plus a mild
    front-of-document prior.

    Raw-count scoring favours dense later note pages that merely repeat
    common terms; the answers to these analyst questions live in the
    front-half MD&A / overview / segment review. Down-weighting common
    tokens (inverse page-frequency) and presence-not-frequency surfaces the
    discriminative page; the early prior breaks ties toward the primary
    disclosure.
    """
    kw = [k for k in dict.fromkeys(kw) if k]
    if not kw:
        return []
    n = len(npages) or 1
    import math
    df = {k: sum(1 for low in npages if k in low) or 1 for k in kw}
    idf = {k: math.log(1 + n / df[k]) for k in kw}
    scored = []
    for i, low in enumerate(npages):
        if not low:
            continue
        s = sum(idf[k] for k in kw if k in low)
        if s:
            s += 0.15 * s * (1.0 - i / n)        # mild early-page prior
            scored.append((s, -i, i))
    scored.sort(reverse=True)
    return [i for _s, _ni, i in scored[:_MAX_PAGES]]


def _line_with(primary: float, raw: str):
    """First line on a page whose own numbers include `primary` (<=200c)."""
    for ln in raw.splitlines():
        if not ln.strip():
            continue
        if primary in _numbers(ln):
            s = ln.strip()
            return s[:200]
    return None


def _line_with_phrase(answer_norm: str, tok0: str, raw: str):
    for ln in raw.splitlines():
        low = _norm(ln)
        if answer_norm in low or (tok0 and tok0 in low):
            s = ln.strip()
            if s:
                return s[:200]
    return None


def _locate(answer: str, pages, npages, pagenums, kw):
    """(page_1based, verbatim_line) for the page+line that prints the answer
    AND best matches the question.

    A reported figure (esp. a small %, ratio or count) recurs on dozens of
    pages; document position alone mis-attributes it. Scoring the candidate
    LINE itself by question-keyword overlap pins the figure to the disclosure
    the question is actually about (offline-probed 15/19 ceiling vs 11/19 for
    earliest-page)."""
    kws = set(kw)

    def _best(cands):
        # cands: list of (page_idx, verbatim_line). Most question-relevant
        # line wins; ties broken toward the earliest page.
        best = None
        for i, ln in cands:
            low = _norm(ln)
            sc = sum(1 for k in kws if k and k in low)
            key = (-sc, i)
            if best is None or key < best[0]:
                best = (key, i, ln)
        return (best[1] + 1, best[2]) if best else (None, None)

    primary = _answer_primary(answer)
    if primary is not None:
        cands = []
        for i, nums in enumerate(pagenums):
            if primary not in nums and not any(
                    n and abs(n - primary) / max(primary, 1.0) <= 0.005
                    for n in nums):
                continue
            for ln in pages[i].splitlines():
                if ln.strip() and primary in _numbers(ln):
                    cands.append((i, ln.strip()[:200]))
                    break
        return _best(cands)
    na = _norm(answer)
    toks = [t for t in re.findall(r"[a-z0-9]+", na) if t not in _STOP]
    if not toks:
        return None, None
    tok0 = toks[0]
    cands = []
    for i, low in enumerate(npages):
        if na in low or (len(toks) >= 2 and all(t in low for t in toks)):
            ln = _line_with_phrase(na, tok0, pages[i])
            if ln:
                cands.append((i, ln))
    return _best(cands)


_SYSTEM = """You answer one analyst question about a company using ONLY the
filing pages provided. Pages are delimited by lines like
"===== PDF PAGE 137 =====".

Use ONLY facts explicitly printed on these pages. Never infer, sum, compute,
or use outside knowledge. Give the answer exactly as printed (keep the
figure's magnitude/unit). If the answer is not explicitly on these pages,
return {"answer":"NOT_FOUND"}.

Return ONLY JSON, no prose:
{"answer":"<concise exact answer, figure as printed>"}"""


def answer_from_filing(pdf_path: str, question: str,
                       company: str = "") -> dict:
    try:
        pages = _render(pdf_path)
    except Exception:  # noqa: BLE001 - unreadable PDF -> honest miss
        return dict(_MISS)
    if not pages:
        return dict(_MISS)
    npages = [_norm(p) for p in pages]
    pagenums = [_numbers(p) for p in pages]   # once per call, linear

    kw = _keywords(question)
    idxs = _rank_pages(npages, kw) or list(range(min(_MAX_PAGES, len(pages))))
    idxs = sorted(idxs)

    blocks, total = [], 0
    for i in idxs:
        chunk = f"\n===== PDF PAGE {i + 1} =====\n{pages[i]}\n"
        if total + len(chunk) > _MAX_CHARS:
            break
        blocks.append(chunk)
        total += len(chunk)

    user = (f"Company: {company}\nQuestion: {question}\n\n"
            f"Filing pages:\n{''.join(blocks)}")
    try:
        raw = _llm_complete(_SYSTEM, user, max_tokens=400)
    except Exception:  # noqa: BLE001 - provider failure -> honest miss
        return dict(_MISS)

    a, b = raw.find("{"), raw.rfind("}")
    if a == -1 or b == -1:
        return dict(_MISS)
    try:
        obj = json.loads(raw[a:b + 1])
    except json.JSONDecodeError:
        return dict(_MISS)
    ans = obj.get("answer")
    if not isinstance(ans, str) or ans.strip().upper() == "NOT_FOUND":
        return dict(_MISS)
    ans = _clean_answer(ans)
    if not ans:
        return dict(_MISS)

    page, quote = _locate(ans, pages, npages, pagenums, kw)
    if page is None:
        return dict(_MISS)  # unverifiable -> honest miss, never fabricate
    return {"answer": ans, "page": page, "quote": quote}
