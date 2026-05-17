"""Deterministic text + number normalization for European filings.

Pure functions, no LLM. Handles the two recurring extraction killers:
  * Nordic/European space-as-thousands ("172 664" -> 172664)
  * Non-ASCII minus glyphs used by Atlas Copco / others ("−" or PUA -> '-')
"""
import re

# Every dash/minus codepoint that has shown up in EU annual report PDFs.
_MINUS = "−‐‑‒–—―➖﹣－­"
_MINUS_RE = re.compile("[" + re.escape(_MINUS) + "]")
# space-like separators between digit groups
_THOUSANDS_RE = re.compile(r"(?<=\d)[\s   ](?=\d{3}(?:\D|$))")


def normalize_minus(s: str) -> str:
    """Only fix the non-ASCII minus glyphs. Leaves digit spacing intact so an
    LLM can still distinguish a leading note-ref digit from the figures."""
    return _MINUS_RE.sub("-", s)


def normalize_text(s: str) -> str:
    """Minus-normalize AND join space-separated thousand groups. Use only for
    parsing an ISOLATED clean numeric token — NOT on whole statement lines
    (a leading "Note 3" glues into the figure otherwise)."""
    s = _MINUS_RE.sub("-", s)
    prev = None
    while prev != s:  # "1 234 567" needs multiple passes
        prev = s
        s = _THOUSANDS_RE.sub("", s)
    return s


_NUM_RE = re.compile(r"-?\(?\d[\d.,]*\)?")


def parse_money(token: str):
    """Parse a single as-reported figure to a float (millions, signed).

    Accepts: '172664', '-97547', '(97 547)', '1,234.5', '1.234,5' (rare).
    Returns float or None.
    """
    if token is None:
        return None
    t = normalize_text(str(token)).strip()
    if t in ("", "-", "--", "n/a", "na", "—"):
        return None
    neg = False
    if t.startswith("(") and t.endswith(")"):
        neg, t = True, t[1:-1]
    if t.startswith("-"):
        neg, t = True, t[1:]
    t = re.sub(r"[^\d.,]", "", t)
    if not t or not any(c.isdigit() for c in t):
        return None
    # Decide decimal separator: if both '.' and ',' present, the LAST one is
    # the decimal sep. If only ',' and it looks like a decimal (<=2 trailing).
    if "." in t and "," in t:
        if t.rfind(".") > t.rfind(","):
            t = t.replace(",", "")
        else:
            t = t.replace(".", "").replace(",", ".")
    elif "," in t:
        frac = t.split(",")[-1]
        if len(frac) <= 2 and t.count(",") == 1:
            t = t.replace(",", ".")
        else:
            t = t.replace(",", "")
    try:
        v = float(t)
    except ValueError:
        return None
    return -v if neg else v


def find_years(header_region: str):
    """Return up to the two most recent distinct fiscal years (oldest-first).

    Looks for 4-digit years 2010-2030 in the income-statement header band.
    """
    yrs = []
    for m in re.findall(r"\b(20[1-3]\d)\b", header_region):
        y = int(m)
        if 2010 <= y <= 2030 and y not in yrs:
            yrs.append(y)
    if len(yrs) < 2:
        return None
    # The two most frequent / earliest-appearing pair on the face header are
    # the comparative columns. Take the two largest distinct (current + prior).
    top2 = sorted(set(yrs), reverse=True)[:2]
    return sorted(top2)  # oldest-first to match model array order
