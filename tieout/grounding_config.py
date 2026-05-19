"""Immutable config for the grounding-rate instrument.

Sibling of the tie-out instrument. Measures whether finmodel's ad-hoc
filing-research path can answer free-form analyst questions about a company
with a VERIFIABLE citation (filing page + exact figure/verbatim quote),
covering content BEYOND the 3 face statements: notes, segment tables, debt /
lease / pension / tax / fair-value disclosures, and MD&A narrative.

Built ONCE during autoresearch setup, then frozen. The optimization loop must
never edit anything under tieout/ (this file, the runner, the answer key).
"""
from tieout.config import BASKET, GT_DIR, RESULTS_DIR  # noqa: F401  re-export

# Immutable Q&A answer key + per-run results live in their own subdirs so they
# never collide with the tie-out instrument's files.
GROUNDING_GT_DIR = GT_DIR / "grounding"
GROUNDING_RESULTS_DIR = RESULTS_DIR / "grounding"
for _d in (GROUNDING_GT_DIR, GROUNDING_RESULTS_DIR):
    _d.mkdir(parents=True, exist_ok=True)

# Companies measured by the grounding instrument. Start with ATCO only: its
# PDF is pinned on disk (deterministic, no network), and it self-tests the
# instrument. Expand by appending tickers that already tie out at 100% AND
# whose PDF pins reliably (SAP.DE / MC.PA are excluded — known sourcing gaps
# that would poison the metric, per the goal spec).
GROUNDING_BASKET = ["ATCO-B.ST"]

# Non-face content classes the question key must probe. Pass A is told to
# spread questions across these so the metric reflects broad comprehension,
# not just one easy disclosure.
TOPICS = [
    "segment_revenue",        # revenue/profit by operating segment
    "geographic_split",       # revenue by region/country
    "debt_maturities",        # borrowings due by year / maturity profile
    "lease_obligations",      # operating vs finance lease amounts
    "pension_obligations",    # defined-benefit obligation / plan assets
    "tax_note",               # effective tax rate, deferred tax components
    "fair_value",             # financial-instrument fair-value disclosures
    "capex_or_da_schedule",   # depreciation/amortisation or capex breakdown
    "share_based_payments",   # SBC expense / option plans
    "mdna_narrative",         # a quantified claim from the Board/MD&A review
]

# Self-test floor: the immutable key must yield at least this many dual-pass
# trusted items spread over at least MIN_TOPICS distinct topics, or the
# instrument is declared untrustworthy for that company (loudly skipped, never
# silently certifying a thin/garbage key).
MIN_TRUSTED = 6
MIN_TOPICS = 3

# Numeric answers count as correct only if equal at the reporting unit. A small
# relative tolerance absorbs rounding between "1,234.5" and "1234".
NUM_REL_TOL = 0.005

# Off-by-one page tolerance: pdfplumber 0-based vs filing printed folio, and
# disclosures that straddle a page break.
PAGE_TOL = 1
