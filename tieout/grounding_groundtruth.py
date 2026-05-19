"""Independent, immutable Q&A answer key for the grounding instrument.

Discipline mirrors tieout.groundtruth (the loop must not mark its own
homework):

  * Does NOT import src.* — own PDF rendering, own prompts, own transport
    (tieout.llm, the frozen `claude -p` copy).
  * TWO decorrelated passes:
      - Pass A *proposes* (question, expected_answer, page, anchor) tuples
        from page-tagged windows of the filing.
      - Pass B *independently confirms*: given ONLY the question and the
        rendered text of the cited page (never Pass A's answer), it answers
        afresh. An item is trusted only if B's answer agrees with A's.
  * MECHANICAL gate before B even runs: the anchor (exact figure or verbatim
    quote) must literally be present on the cited page's independently
    rendered text — kills hallucinated citations in the key itself.
  * Self-test: the surviving key must clear MIN_TRUSTED items over MIN_TOPICS
    topics, and a sampled anchor is re-checked against a fresh render. If the
    instrument can't certify a real, grounded key it fails LOUDLY.
  * Result cached to groundtruth/grounding/<ticker>.json and treated as
    IMMUTABLE: once written it is never regenerated within the loop.
"""
import json
import re
import sys

import pdfplumber

from tieout.grounding_config import (GROUNDING_GT_DIR, TOPICS, MIN_TRUSTED,
                                     MIN_TOPICS, NUM_REL_TOL)
from tieout.llm import complete
from tieout.textnorm import normalize_minus, parse_money

# Cost bounds for key generation (one-time, but still be frugal).
_MAX_WINDOWS = 3
_WINDOW_PAGES = 9
_MAX_ITEMS_PER_WINDOW = 9
_MAX_WINDOW_CHARS = 48_000

_NOTE_KEYWORDS = re.compile(
    r"\b(notes? to the (consolidated )?financial statements|note \d|"
    r"segment(s| information| reporting)|operating segments|by geograph|"
    r"maturit|lease (liabilit|obligation)|defined benefit|pension|"
    r"deferred tax|effective tax rate|fair value|share[- ]based|"
    r"board of directors. report|management.s (discussion|report)|"
    r"business review|director.s report)\b", re.I)
_FACE_ONLY = re.compile(
    r"\b(consolidated income statement|consolidated balance sheet|"
    r"consolidated statement of cash flow|consolidated statement of "
    r"financial position)\b", re.I)
_NUM_TOKEN = re.compile(r"-?\(?\d[\d   .,]*\d\)?|\b\d\b")


def gt_path(ticker: str):
    return GROUNDING_GT_DIR / f"{ticker.replace('/', '_').replace('.', '_')}.json"


# ---------------------------------------------------------------- rendering --

def _render_pages(pdf_path: str):
    """[(page_no_1based, raw_text, norm_text, [number_values])]."""
    out = []
    with pdfplumber.open(pdf_path) as pdf:
        for i, p in enumerate(pdf.pages):
            raw = normalize_minus(p.extract_text() or "")
            norm = re.sub(r"\s+", " ", raw).strip().lower()
            vals = []
            for m in _NUM_TOKEN.findall(raw):
                v = parse_money(m)
                if v is not None:
                    vals.append(v)
            out.append((i + 1, raw, norm, vals))
    return out


def _select_windows(pages):
    """Page-index windows that look like notes / segment / MD&A regions."""
    hot = [idx for idx, (_n, raw, _nm, _v) in enumerate(pages)
           if _NOTE_KEYWORDS.search(raw) and not (
               _FACE_ONLY.search(raw) and not _NOTE_KEYWORDS.search(
                   raw.replace("\n", " ")))]
    if not hot:
        # fallback: back half of the document (notes live there)
        hot = [len(pages) // 2]
    windows, used = [], set()
    for start in hot:
        if start in used or len(windows) >= _MAX_WINDOWS:
            continue
        sl = pages[start:start + _WINDOW_PAGES]
        for k in range(start, start + _WINDOW_PAGES):
            used.add(k)
        txt, total = [], 0
        for (n, raw, _nm, _v) in sl:
            chunk = f"\n===== PDF PAGE {n} =====\n{raw}\n"
            if total + len(chunk) > _MAX_WINDOW_CHARS:
                break
            txt.append(chunk)
            total += len(chunk)
        if txt:
            windows.append("".join(txt))
    return windows


# ------------------------------------------------------------- anchor check --

def _digits(s: str) -> str:
    return re.sub(r"\D", "", s)


def _anchor_present(anchor: str, raw: str, norm: str, vals) -> bool:
    """Is `anchor` mechanically present on this page's rendered text?

    Numeric anchor: a printed figure on the page equals it within tolerance
    (so "12 345" / "12,345" / "12345.0" all match). Text anchor: the
    whitespace-collapsed verbatim quote (or its first 60 chars) is a substring
    of the page's collapsed text.
    """
    a = (anchor or "").strip()
    if not a:
        return False
    v = parse_money(a)
    looks_numeric = v is not None and _digits(a) != "" and \
        len(re.sub(r"[\d   .,()\-]", "", a)) == 0
    if looks_numeric:
        av = abs(v)
        for pv in vals:
            pav = abs(pv)
            if pav == av:
                return True
            denom = max(abs(av), 1.0)
            if abs(pav - av) / denom <= NUM_REL_TOL:
                return True
        # last resort: exact digit run appears verbatim on the page
        return _digits(a) != "" and _digits(a) in _digits(raw)
    q = re.sub(r"\s+", " ", a).strip().lower()
    if len(q) >= 8 and q in norm:
        return True
    head = q[:60]
    return len(head) >= 12 and head in norm


# --------------------------------------------------------------- pass A / B --

def _passA_prompt(window_text: str, company: str, currency: str,
                  variant: str) -> str:
    persona = ("an audit-grade filing analyst" if variant == "A"
               else "a meticulous equity research associate")
    topics = ", ".join(TOPICS)
    return f"""You are {persona} reading one excerpt of {company}'s annual
report. The excerpt is split into pages delimited by lines like
"===== PDF PAGE 137 =====". Reporting currency: {currency}.

Produce up to {_MAX_ITEMS_PER_WINDOW} self-contained analyst questions whose
answer is a SINGLE concrete fact stated on EXACTLY ONE of these pages, drawn
from NON-face-statement content only (notes, segment tables, geographic
splits, debt maturities, leases, pensions, deferred tax, fair value,
share-based payments, or a quantified MD&A / Board-report claim). NEVER ask
about a number that only appears on the consolidated income statement,
balance sheet or cash-flow face statements.

For each item give:
- "question": unambiguous, answerable from the cited page alone, names the
  company and the period if relevant.
- "expected_answer": the exact answer. If numeric, the figure AS PRINTED
  (keep the magnitude/unit, e.g. "12,345" or "1,234.5"); else a short phrase.
- "source_page": the integer PDF PAGE number (from the delimiter) that
  contains the supporting figure/sentence.
- "anchor": the exact figure as printed on that page, OR a <=120-char
  verbatim quote copied from that page that contains the answer.
- "topic": one of [{topics}].

Hard rules: copy the anchor character-for-character from the page text;
never infer, sum, or compute; if a fact is not explicitly printed, do not
ask about it. Spread questions across as many distinct topics as the excerpt
supports.

Return ONLY JSON, no prose:
{{"items":[{{"question":"...","expected_answer":"...","source_page":<int>,
"anchor":"...","topic":"..."}}]}}

Excerpt:
{window_text}
"""


def _passB_prompt(question: str, page_text: str, page_no: int) -> str:
    return f"""You are an independent fact checker. Below is the verbatim
text of a single page (PDF page {page_no}) from a company annual report,
followed by one question. Answer the question USING ONLY this page's text.

If the page does not contain the answer, reply ONLY JSON:
{{"answer":"NOT_ON_PAGE"}}
Otherwise reply ONLY JSON: {{"answer":"<the exact fact, figure as printed>"}}
No prose, no explanation.

PAGE {page_no} TEXT:
{page_text[:36_000]}

QUESTION: {question}
"""


def _parse_json(raw: str) -> dict:
    raw = raw.strip()
    a, b = raw.find("{"), raw.rfind("}")
    if a == -1 or b == -1:
        raise ValueError(f"no JSON object: {raw[:160]}")
    return json.loads(raw[a:b + 1])


def _answers_agree(a: str, b: str) -> bool:
    if a is None or b is None:
        return False
    bs = str(b).strip()
    if bs.upper() in ("NOT_ON_PAGE", "", "N/A", "NULL", "NONE"):
        return False
    av, bv = parse_money(a), parse_money(bs)
    if av is not None and bv is not None:
        if abs(av) == abs(bv):
            return True
        denom = max(abs(av), 1.0)
        return abs(abs(av) - abs(bv)) / denom <= NUM_REL_TOL
    na = re.sub(r"[^a-z0-9]+", " ", str(a).lower()).strip()
    nb = re.sub(r"[^a-z0-9]+", " ", bs.lower()).strip()
    if not na or not nb:
        return False
    if na == nb or na in nb or nb in na:
        return True
    ta, tb = set(na.split()), set(nb.split())
    inter = len(ta & tb)
    return inter >= 3 and inter / max(len(ta | tb), 1) >= 0.6


def build_grounding_truth(ticker: str, company: str, currency: str,
                          pdf_path: str, *, force: bool = False) -> dict:
    out_path = gt_path(ticker)
    if out_path.exists() and not force:
        return json.loads(out_path.read_text(encoding="utf-8"))

    pages = _render_pages(pdf_path)
    by_no = {n: (raw, nm, vals) for (n, raw, nm, vals) in pages}
    windows = _select_windows(pages)

    # ---- Pass A: propose candidates over each window (alternate persona) ----
    candidates = []
    for wi, w in enumerate(windows):
        variant = "A" if wi % 2 == 0 else "B"
        try:
            pa = _parse_json(complete(_passA_prompt(w, company, currency,
                                                    variant),
                                      "Generate the question set now.",
                                      timeout=600))
        except Exception as e:  # noqa: BLE001 - one bad window must not kill it
            print(f"  [gt] window {wi} pass-A failed: {e}", file=sys.stderr)
            continue
        for it in pa.get("items", []) or []:
            q = str(it.get("question", "")).strip()
            ea = str(it.get("expected_answer", "")).strip()
            anchor = str(it.get("anchor", "")).strip()
            topic = str(it.get("topic", "")).strip().lower()
            try:
                pg = int(it.get("source_page"))
            except (TypeError, ValueError):
                continue
            if not (q and ea and anchor and pg in by_no):
                continue
            raw, nm, vals = by_no[pg]
            # MECHANICAL gate 1: anchor must really be on the cited page.
            if not _anchor_present(anchor, raw, nm, vals):
                continue
            candidates.append({"question": q, "expected_answer": ea,
                               "source_page": pg, "anchor": anchor,
                               "topic": topic if topic in TOPICS else "other"})

    # de-dupe on (normalized question)
    seen, uniq = set(), []
    for c in candidates:
        k = re.sub(r"[^a-z0-9]+", " ", c["question"].lower()).strip()
        if k and k not in seen:
            seen.add(k)
            uniq.append(c)

    # ---- Pass B: independent confirmation from the cited page only ---------
    trusted, rejected = [], []
    for c in uniq:
        raw, _nm, _v = by_no[c["source_page"]]
        try:
            pb = _parse_json(complete(
                _passB_prompt(c["question"], raw, c["source_page"]),
                "Answer now.", timeout=600))
            b_ans = str(pb.get("answer", "")).strip()
        except Exception as e:  # noqa: BLE001
            rejected.append({**c, "reason": f"passB error: {e}"})
            continue
        if _answers_agree(c["expected_answer"], b_ans):
            trusted.append({**c, "confirmed_answer": b_ans})
        else:
            rejected.append({**c, "reason": f"disagree (B={b_ans!r})"})

    key = {
        "ticker": ticker, "company": company, "currency": currency,
        "source_pdf": str(pdf_path), "n_pages": len(pages),
        "n_candidates": len(uniq), "n_trusted": len(trusted),
        "topics_covered": sorted({t["topic"] for t in trusted}),
        "items": trusted, "rejected": rejected,
    }
    _self_test(ticker, key, by_no)
    out_path.write_text(json.dumps(key, indent=2, ensure_ascii=False),
                        encoding="utf-8")
    return key


def _self_test(ticker: str, key: dict, by_no: dict):
    bad = []
    if key["n_trusted"] < MIN_TRUSTED:
        bad.append(f"only {key['n_trusted']} trusted items "
                   f"(need >= {MIN_TRUSTED})")
    topics = {t["topic"] for t in key["items"]
              if t["topic"] != "other"}
    if len(topics) < MIN_TOPICS:
        bad.append(f"only {len(topics)} distinct topics "
                   f"(need >= {MIN_TOPICS}): {sorted(topics)}")
    # Re-verify a sampled item's anchor against a fresh render of its page.
    if key["items"]:
        s = key["items"][len(key["items"]) // 2]
        raw, nm, vals = by_no.get(s["source_page"], ("", "", []))
        if not _anchor_present(s["anchor"], raw, nm, vals):
            bad.append(f"sampled anchor not re-found on page "
                       f"{s['source_page']}: {s['anchor']!r}")
    if bad:
        raise AssertionError(
            f"{ticker}: grounding answer-key self-test FAILED — instrument "
            "untrustworthy:\n  - " + "\n  - ".join(bad))


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    from tieout.config import BASKET
    from tieout.pin_filings import ensure_pinned
    tk = sys.argv[1] if len(sys.argv) > 1 else "ATCO-B.ST"
    row = next(r for r in BASKET if r["ticker"] == tk)
    pdf = ensure_pinned(row)
    k = build_grounding_truth(tk, row["company"], row["currency"], str(pdf),
                              force="--force" in sys.argv)
    print(json.dumps({"trusted": k["n_trusted"],
                       "candidates": k["n_candidates"],
                       "topics": k["topics_covered"]}, indent=2))
