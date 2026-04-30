"""
Deal synthesis — shared between agent.py and news.py to avoid circular imports.
Extracts structured deal facts from raw multi-source article text.
"""

import re


_PRIORITY_SOURCES = {"wire_services", "company_announcement", "reuters", "bloomberg"}

_RATIONALE_KW = {
    "platform", "scale", "growth", "strategy", "complementary",
    "synerg", "position", "leader", "expand", "global", "network",
    "freight", "logistics", "supply chain", "value creation",
}


def synthesize_deal(raw: dict) -> dict:
    """
    Extract structured deal facts from multi-source article text.
    Covers: date, acquirer, target, deal value, stake, multiples,
    target financials, advisors, strategic rationale, close timeline.
    """
    parts = []
    for k, v in raw.items():
        if v and "ERROR" not in str(v) and len(str(v)) > 50:
            text = str(v)
            parts.append(text)
            if k in _PRIORITY_SOURCES:
                parts.append(text)  # double-weight primary sources
    combined = " ".join(parts)

    if not combined.strip():
        return {"status": "no_content_retrieved"}

    def _proper_name(s: str) -> str:
        """Strip trailing lowercase words — fixes IGNORECASE over-matching in entity names."""
        words = s.split()
        result = [w for i, w in enumerate(words) if i == 0 or w[0].isupper()]
        # Also stop at common prepositions/articles even if capitalised mid-sentence
        stop_at = {"To", "For", "In", "A", "An", "The", "And", "Of", "With", "From"}
        final = []
        for w in result:
            if w in stop_at and final:
                break
            final.append(w)
        return " ".join(final).strip(" ,.")

    def _find(patterns: list, text: str = None):
        src = text or combined
        for p in patterns:
            m = re.search(p, src, re.IGNORECASE | re.DOTALL)
            if m:
                return m.group(1).strip()
        return None

    def _find_all(pattern: str):
        return re.findall(pattern, combined, re.IGNORECASE)

    # Build deal-context text (sentences mentioning deal keywords) for date extraction.
    # This prevents matching page-metadata dates (e.g. article publication date).
    _deal_ctx_kw = r'(?:acqui|merger|deal|transaction|purchase|stake|announced|signed|agreed|partnership|invest)'
    _deal_sentences = [
        s for s in re.split(r'(?<=[.!?])\s+', combined)
        if re.search(_deal_ctx_kw, s, re.I)
    ]
    _deal_ctx = " ".join(_deal_sentences) if _deal_sentences else combined

    date = _find([
        r'(?:announced?|signed?|closed?|completed?|agreed)\s+(?:on\s+)?'
        r'(\w+\s+\d{1,2},?\s*202[4-6])',
        r'((?:January|February|March|April|May|June|July|August|September|October|November|December)'
        r'\s+\d{1,2},?\s*202[4-6])',
        r'(202[4-6]-\d{2}-\d{2})',
        r'(\d{1,2}\s+(?:January|February|March|April|May|June|July|August|September|October|November|December)\s+202[4-6])',
    ], text=_deal_ctx)

    # Acquirer name: 1-5 capitalized words on the SAME line ([ \t] avoids newline matching)
    _acq = r'([A-Z][A-Za-z&]+(?:[ \t]+[A-Z][A-Za-z&]+){0,4})'
    acquirer = _find([
        # Passive: "acquired/bought/purchased by X"
        r'(?:acquired?|purchased?|bought)\s+by\s+' + _acq,
        # "sold/transferred … to X"
        r'(?:sold\s+(?:a\s+)?(?:majority|controlling|minority|\d+%)?\s*'
        r'(?:stake|interest|ownership)?\s*to)\s+' + _acq,
        # "X has agreed to acquire / will acquire / completed acquisition of"
        _acq + r'\s+(?:has\s+)?(?:agreed\s+to\s+acquire|will\s+acquire'
        r'|completed\s+(?:the\s+|its\s+)?acquisition\s+of'
        r'|announced\s+(?:the\s+|its\s+)?acquisition\s+of'
        r'|signed\s+(?:a\s+)?definitive\s+agreement)\s+[A-Z]',
        # "X acquires/acquired/purchases Y"
        _acq + r'\s+(?:acquires?|acquired?|purchases?|to\s+acquire)\s+[A-Z]',
        _acq + r'\s+(?:Equity\s+Group|Capital|Partners|Ventures?)'
        r'(?:\s+acquires?|\s+announced|\s+has)',
        _acq + r'\s+(?:takes?|took)\s+(?:majority|controlling|minority)?\s*'
        r'(?:stake|ownership)',
    ], text=_deal_ctx)

    _NAV_VERBS = {
        "Read", "View", "See", "Click", "Download", "Visit", "Learn", "Get",
        "Sign", "Subscribe", "Watch", "Catch", "Join", "Follow", "More",
        "Related", "Featured", "Latest", "Recent", "Popular", "Search",
        "Contact", "About", "Press", "Industry", "Supply", "Share", "Print",
    }
    if acquirer:
        acquirer = _proper_name(acquirer)
        if not acquirer or acquirer.split()[0] in _NAV_VERBS:
            acquirer = None

    target = _find([
        r'(?:acquires?|acquired?|purchases?)\s+([A-Z][A-Za-z\s&,]+?)'
        r'(?:\s+for|\s+in\s+a|\s+\(|\.|,)',
    ])
    if target:
        target = _proper_name(target)
        if not target or len(target) < 4 or target.split()[0] in _NAV_VERBS:
            target = None

    deal_value = _find([
        r'valued?\s+at\s+(?:approximately\s+)?\$\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)',
        r'(?:enterprise value|EV)\s+of\s+(?:approximately\s+)?\$?\s*'
        r'([\d,.]+\s*(?:billion|million|bn|mn)\b)',
        r'\$([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+'
        r'(?:deal|transaction|acquisition|purchase)',
        r'(?:purchase price|consideration)\s+of\s+\$?\s*'
        r'([\d,.]+\s*(?:billion|million|bn|mn)\b)',
        r'\$([\d,.]+[Bb]\b)',
        r'\$([\d,.]+[Mm]\b)\s+(?:deal|acquisition)',
    ]) or "undisclosed"

    stake = _find([
        r'(\d{1,3}(?:\.\d+)?%)\s+(?:ownership|interest|equity stake|stake)',
        r'(?:acquire[sd]?|purchas\w+)\s+(?:a\s+)?(\d{1,3}(?:\.\d+)?%)\s+stake',
        r'(majority|controlling|minority|100%)\s+(?:ownership\s+)?(?:stake|interest)',
        r'(majority|controlling|minority)\s+(?:equity\s+)?(?:position|ownership)',
    ])

    revenue = _find([
        r'(?:annual\s+)?revenue[s]?\s+of\s+(?:approximately\s+|\~)?\$?\s*'
        r'([\d,.]+\s*(?:billion|million|bn|mn)\b)',
        r'\$\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+in\s+(?:annual\s+)?revenue',
        r'(?:generates?|reported?)\s+(?:annual\s+)?revenue[s]?\s+of\s+\$?\s*'
        r'([\d,.]+\s*(?:billion|million|bn|mn)\b)',
    ])

    ebitda = _find([
        r'EBITDA\s+of\s+(?:approximately\s+)?\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)',
        r'\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+(?:of\s+)?EBITDA',
        r'adjusted\s+EBITDA\s+of\s+\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)',
    ])

    multiple = _find([
        r'(\d+\.?\d*[xX])\s+(?:LTM\s+)?EBITDA',
        r'(\d+\.?\d*[xX])\s+(?:trailing|forward)?\s*revenue',
        r'(?:EBITDA|revenue)\s+multiple\s+of\s+(\d+\.?\d*[xX]?)',
        r'valued?\s+at\s+(\d+\.?\d*[xX])\s+(?:times|x)',
    ])

    close = _find([
        r'(?:expected?\s+to\s+close|anticipated?\s+to\s+close|close\s+in)\s+'
        r'((?:Q[1-4]\s+)?(?:the\s+)?(?:first|second|third|fourth)\s+(?:quarter\s+of\s+)?'
        r'20\d{2}|(?:early|mid|late)\s+20\d{2}|\w+\s+20\d{2})',
        r'(?:subject\s+to|pending)\s+([^.]{10,80}(?:regulatory|approval|clearance)[^.]{0,40})',
    ])

    advisor_hits = _find_all(
        r'\b(Goldman Sachs|Morgan Stanley|J\.?P\.?\s*Morgan|Barclays|Lazard|Rothschild|'
        r'Evercore|Centerview|Jefferies|Citi(?:group|bank)?|Deutsche Bank|UBS|'
        r'Houlihan Lokey|PJT Partners|Perella Weinberg|Kirkland(?:\s*&\s*Ellis)?|'
        r'Sullivan(?:\s*&\s*Cromwell)?|Weil(?:\s*,?\s*Gotshal)?|'
        r'Latham(?:\s*&\s*Watkins)?|Simpson(?:\s*Thacher)?|Skadden|'
        r'Davis\s*Polk|Freshfields|Cleary)\b'
    )

    _boilerplate = re.compile(
        r'\b(LAUNCH|OFFER|SUBSCRIBE|NEWSLETTER|PODCAST|WEBCAST|SIGN\s*UP'
        r'|ADVERTISEMENT|SPONSORED|COOKIE|PRIVACY|COPYRIGHT|ALL\s*RIGHTS'
        r'|leading\s+provider|leading\s+source|leading\s+publication'
        r'|is\s+the\s+premier|is\s+a\s+leading|is\s+the\s+leading'
        r'|catch\s+up\s+on|trade\s+shows?|special\s+events?|events\s+taking)\b',
        re.IGNORECASE,
    )
    # Use deal-context sentences for rationale — filters out nav/boilerplate naturally
    rationale_sents = []
    for sent in re.split(r'(?<=[.!?])\s+', _deal_ctx):
        sent = sent.strip()
        sent = re.sub(r'^\([^)]{1,30}\)\s*', '', sent)  # strip leading (Photo: ...) captions
        if (50 < len(sent) < 300
                and sum(1 for kw in _RATIONALE_KW if kw in sent.lower()) >= 2
                and not _boilerplate.search(sent)
                and not re.search(r'[A-Z]{3,}\s+[A-Z]{3,}', sent)):  # no ALL-CAPS run
            rationale_sents.append(sent)

    sources_used = [
        k for k, v in raw.items()
        if v and "ERROR" not in str(v) and len(str(v)) > 100
    ]

    out: dict = {}
    if date:             out["announced"]          = date
    if acquirer:         out["acquirer"]           = acquirer
    if target:           out["target"]             = target
    out["deal_value"]    = deal_value
    if stake:            out["stake"]              = stake
    if revenue:          out["target_revenue"]     = revenue
    if ebitda:           out["target_ebitda"]      = ebitda
    if multiple:         out["multiple"]           = multiple
    if close:            out["expected_close"]     = close
    if advisor_hits:     out["advisors"]           = "; ".join(dict.fromkeys(advisor_hits[:4]))
    if rationale_sents:  out["strategic_rationale"] = " ".join(rationale_sents[:2])
    if sources_used:     out["sources"]            = ", ".join(sources_used)

    return out


def is_sufficient(summary: dict) -> bool:
    """Phase cascade stop condition — do we have enough to stop searching?"""
    has_date     = bool(summary.get("announced"))
    has_value    = summary.get("deal_value", "undisclosed") != "undisclosed"
    has_acquirer = bool(summary.get("acquirer"))
    has_rationale = bool(summary.get("strategic_rationale"))
    n_sources    = len(summary.get("sources", "").split(",")) if summary.get("sources") else 0
    return (
        (has_date and (has_value or (has_acquirer and has_rationale)))
        or n_sources >= 3
    )
