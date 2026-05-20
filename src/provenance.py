"""CellProvenance — source-location metadata for every number written to Excel.

For each numeric value extracted from a filing, we record the PDF page index
and bbox where the raw value text appears. This lets downstream consumers
render a snapshot of that page with the number highlighted (audit trail).

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
    """Search a fitz Document for the first occurrence of a numeric value.

    Tries each variant from normalize_variants() in order. Returns
    (page_index, bbox, raw_text) on hit, or (None, None, None) on miss.

    page_hint is searched first (if provided) for speed.
    max_pages caps how many pages to scan.
    """
    variants = normalize_variants(value)
    if not variants:
        return (None, None, None)

    n_pages = doc.page_count
    if max_pages is not None:
        n_pages = min(n_pages, max_pages)

    # Build search order: hint first, then sequential, skipping duplicates
    order: list[int] = []
    if page_hint is not None and 0 <= page_hint < n_pages:
        order.append(page_hint)
    for i in range(n_pages):
        if i not in order:
            order.append(i)

    for pg in order:
        page = doc[pg]
        for variant in variants:
            try:
                hits = page.search_for(variant)
            except Exception:
                hits = []
            if hits:
                r = hits[0]
                return (pg, (float(r.x0), float(r.y0), float(r.x1), float(r.y1)), variant)

    return (None, None, None)


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
