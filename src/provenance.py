"""CellProvenance — source-location metadata for every number written to Excel.

For each numeric value extracted from a filing, we record the PDF page index
and bbox where the raw value text appears. This lets downstream consumers
build a file#page link to that page (audit trail); bbox is retained for a
future viewer that highlights the exact number.

Public API:
    CellProvenance       — dataclass for one cell's source location
    normalize_variants   — generate searchable text variants of a numeric value
    locate_value_in_pdf  — search a fitz PDF for a numeric value across pages
    provenance_dict      — JSON-serializable nested dict shape stored in cache
"""
from __future__ import annotations

from dataclasses import dataclass, asdict
from typing import Any, Iterable, Optional


@dataclass
class CellProvenance:
    """Source-location metadata for one extracted numeric value.

    pdf_path        absolute or repo-relative path to source PDF
    page_index      0-based page index in the PDF
    bbox            (x0, y0, x1, y1) in PDF coordinate space; None if low_confidence
    raw_text        the exact string located on the page (e.g. "168,343")
    label           sheet/row label for the cell (e.g. "Revenue")
    key             field key (e.g. "revenue")
    period          optional period tag (e.g. "2023A")
    low_confidence  True when bbox could not be located; page_index may still be set
    """
    pdf_path: str
    page_index: int
    bbox: Optional[tuple[float, float, float, float]]
    raw_text: str
    label: str
    key: str
    period: Optional[str] = None
    low_confidence: bool = False

    def to_json(self) -> dict[str, Any]:
        d = asdict(self)
        if d["bbox"] is not None:
            d["bbox"] = list(d["bbox"])
        return d

    @classmethod
    def from_json(cls, d: dict[str, Any]) -> "CellProvenance":
        bbox = d.get("bbox")
        if bbox is not None:
            bbox = tuple(bbox)
        return cls(
            pdf_path=d["pdf_path"],
            page_index=int(d["page_index"]),
            bbox=bbox,
            raw_text=d["raw_text"],
            label=d["label"],
            key=d["key"],
            period=d.get("period"),
            low_confidence=bool(d.get("low_confidence", False)),
        )


# ---------------------------------------------------------------------------
# Value-text normalization
# ---------------------------------------------------------------------------

# Narrow no-break space U+202F (Nordic IFRS PDFs) + regular NBSP U+00A0
_THOUSAND_SEPS = [",", " ", " ", " ", ".", ""]


def normalize_variants(value: float | int) -> list[str]:
    """Generate searchable string variants of a numeric value.

    Covers: comma-grouped (168,343), space-grouped (168 343), NBSP-grouped,
    parenthesized-negative ((168,343)), signed (-168,343), and plain (168343).

    Returns variants ordered most-likely-first.
    """
    if value is None:
        return []
    try:
        f = float(value)
    except (TypeError, ValueError):
        return []

    is_neg = f < 0
    n = abs(f)
    # Integer vs decimal
    if abs(n - round(n)) < 1e-9:
        digits = str(int(round(n)))
        decimals = ""
    else:
        s = f"{n:.4f}".rstrip("0").rstrip(".")
        head, _, tail = s.partition(".")
        digits = head
        decimals = tail

    variants: list[str] = []
    for sep in _THOUSAND_SEPS:
        if not digits:
            continue
        # Group digits in 3s from the right
        groups = []
        rem = digits
        while len(rem) > 3:
            groups.append(rem[-3:])
            rem = rem[:-3]
        groups.append(rem)
        groups.reverse()
        grouped = sep.join(groups) if sep else digits
        if decimals:
            grouped = f"{grouped}.{decimals}"
        # Plain
        variants.append(grouped)
        # Negative variants
        if is_neg:
            variants.append(f"-{grouped}")
            variants.append(f"({grouped})")

    # Deduplicate preserving order
    seen = set()
    out = []
    for v in variants:
        if v not in seen:
            seen.add(v)
            out.append(v)
    return out


# ---------------------------------------------------------------------------
# PDF search
# ---------------------------------------------------------------------------

def locate_value_in_pdf(
    doc: Any,
    value: float | int,
    *,
    page_hint: Optional[int] = None,
    max_pages: Optional[int] = None,
) -> tuple[Optional[int], Optional[tuple[float, float, float, float]], Optional[str]]:
    """Search a fitz Document for the first WHOLE-NUMBER occurrence of a value.

    Returns (page_index, bbox, raw_text) on hit, or (None, None, None) on miss.

    Uses word-level reconstruction (NOT page.search_for, which matches
    substrings: e.g. "7 067" matches inside "67 067"). Numbers are rebuilt
    from page words by concatenating a leading 1-3 digit group with any
    following exact 3-digit thousand groups on the same line, then compared
    by EXACT digit-string equality to the target.

    page_hint is searched first (if provided) for speed.
    max_pages caps how many pages to scan.
    """
    target_digits = _target_digits(value)
    if target_digits is None:
        return (None, None, None)

    n_pages = doc.page_count
    if max_pages is not None:
        n_pages = min(n_pages, max_pages)

    order: list[int] = []
    if page_hint is not None and 0 <= page_hint < n_pages:
        order.append(page_hint)
    for i in range(n_pages):
        if i not in order:
            order.append(i)

    for pg in order:
        page = doc[pg]
        for digits, raw, bbox in _iter_page_numbers(page):
            if digits == target_digits:
                return (pg, bbox, raw)

    return (None, None, None)



def _target_digits(value: float | int) -> Optional[str]:
    """Digit-string of |value|'s integer part (e.g. -168343.0 -> '168343')."""
    if value is None:
        return None
    try:
        f = float(value)
    except (TypeError, ValueError):
        return None
    n = abs(f)
    if abs(n - round(n)) < 1e-9:
        return str(int(round(n)))
    # Decimal: keep integer part only for matching (decimals rare in statements)
    return str(int(n)) if int(n) != 0 else None


_GROUP_GAP_RATIO = 0.9
_GROUP_GAP_MIN = 2.5


def _strip_group_chars(s: str) -> str:
    """Remove in-token thousand separators: comma + any Unicode space char.

    Excludes '.' so decimals survive. Regular spaces never appear inside a word
    token (they split words), but NBSP/narrow-NBSP/thin-space can.
    """
    return "".join(ch for ch in s if ch != "," and not ch.isspace())


def _clean_token(word: str) -> tuple[str, bool]:
    """Strip grouping chars + sign/paren. Returns (core_digits, is_negative)."""
    w = word.strip()
    neg = w.startswith("(") or w.startswith("-")
    w = w.lstrip("(-").rstrip(").,;")
    w = _strip_group_chars(w)
    return (w, neg)


def _iter_page_numbers(page: Any):
    """Yield (digit_string, raw_text, bbox) for every whole number on the page.

    Reconstructs thousand-grouped numbers split across words by regular spaces
    (e.g. "172" + "664" -> "172664"), and also handles numbers whose grouping
    lives inside one word via NBSP/comma (e.g. "172 664" -> "172664").
    """
    import fitz

    words = page.get_text("words")  # (x0,y0,x1,y1,word,block,line,wordno)
    from collections import defaultdict
    lines: dict[tuple[int, int], list] = defaultdict(list)
    for w in words:
        x0, y0, x1, y1, word, blk, ln, wn = w
        lines[(blk, ln)].append((wn, x0, y0, x1, y1, word))

    for _key, ws in lines.items():
        ws.sort(key=lambda t: t[0])
        n = len(ws)
        i = 0
        while i < n:
            wn, x0, y0, x1, y1, word = ws[i]
            core, neg = _clean_token(word)
            if not core.isdigit():
                i += 1
                continue

            # core is pure digits. Two cases:
            #  (a) len > 3 OR len in 1..3 with no following 3-digit group →
            #      standalone complete number (grouping was in-token or absent)
            #  (b) len in 1..3 followed by exact 3-digit groups → space-grouped
            digits = core
            ux0, uy0, ux1, uy1 = x0, y0, x1, y1
            raw_parts = [word.strip()]
            char_w = (x1 - x0) / max(1, len(core))
            max_gap = max(_GROUP_GAP_MIN, _GROUP_GAP_RATIO * char_w)
            j = i + 1
            extended = False
            if len(core) <= 3:
                while j < n:
                    nwn, nx0, ny0, nx1, ny1, nword = ws[j]
                    ncore, _neg2 = _clean_token(nword)
                    gap = nx0 - ux1
                    # thousand group: EXACTLY 3 digits AND horizontally close
                    # (gap guard prevents merging adjacent table columns)
                    if ncore.isdigit() and len(ncore) == 3 and 0 <= gap <= max_gap:
                        digits += ncore
                        ux1, uy1 = nx1, max(uy1, ny1)
                        raw_parts.append(nword.strip())
                        j += 1
                        extended = True
                    else:
                        break

            bbox = (float(ux0), float(uy0), float(ux1), float(uy1))
            yield (digits, " ".join(raw_parts), bbox)
            i = j if extended else i + 1


# ---------------------------------------------------------------------------
# Cache persistence shape
# ---------------------------------------------------------------------------

def provenance_dict(provenances: Iterable[CellProvenance]) -> dict[str, Any]:
    """Build the nested dict shape persisted under cache `__provenance__` key.

    Shape: {"<key>": {"<period_or_index>": <CellProvenance.to_json()>, ...}, ...}
    """
    out: dict[str, Any] = {}
    for p in provenances:
        bucket = out.setdefault(p.key, {})
        period = p.period or "0"
        bucket[period] = p.to_json()
    return out


def load_provenance(d: dict[str, Any]) -> list[CellProvenance]:
    """Inverse of provenance_dict()."""
    out: list[CellProvenance] = []
    for _key, bucket in (d or {}).items():
        if not isinstance(bucket, dict):
            continue
        for _period, payload in bucket.items():
            if isinstance(payload, dict):
                out.append(CellProvenance.from_json(payload))
    return out
