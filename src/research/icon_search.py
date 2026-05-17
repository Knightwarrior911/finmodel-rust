"""
Fluent UI System Icons — enhanced search + render for PowerPoint slides.

Requires the Microsoft Fluent UI System Icons repo cloned locally:
    git clone https://github.com/microsoft/fluentui-system-icons

Configure via env var or constructor:
    FLUENT_ICONS_DIR=/path/to/fluentui-system-icons/assets

Usage:
    from src.research.icon_search import FluentIconSearcher

    searcher = FluentIconSearcher("/path/to/fluentui-system-icons/assets")
    path = searcher.find_and_render("revenue growth", color="#255BE3", size=128)
    # Returns Path to PNG, or None if no match found

    # Debug: see what matched
    for key, score in searcher.list_matches("deal close", top_k=5):
        print(f"  {key:40s}  {score:.1f}")
"""

import difflib
import io
import json
import os
import re
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Synonym / concept map
# Maps user-facing concept words → icon name fragments that should score higher
# ---------------------------------------------------------------------------

SYNONYM_MAP: dict[str, list[str]] = {
    # Finance / valuation
    "money":       ["currency", "coin", "payment", "wallet", "dollar", "bank"],
    "finance":     ["currency", "bank", "coin", "money", "payment", "briefcase"],
    "revenue":     ["currency", "chart", "money", "arrow_up", "trending"],
    "cost":        ["money", "currency", "subtract", "arrow_down"],
    "savings":     ["piggy_bank", "money", "coin", "arrow_down"],
    "investment":  ["briefcase", "bank", "money", "chart"],
    "profit":      ["arrow_up", "trending", "money", "checkmark"],
    "loss":        ["arrow_down", "trending_down", "minus"],
    "valuation":   ["chart", "money", "number", "calculator"],
    "budget":      ["money", "calculator", "document", "currency"],
    "dividend":    ["money", "coin", "currency", "arrow_right"],
    "debt":        ["bank", "building", "briefcase", "document"],
    "equity":      ["people", "building", "money", "briefcase"],
    "deal":        ["handshake", "contract", "document", "sign"],
    "acquisition": ["handshake", "building", "briefcase", "add"],
    "merger":      ["handshake", "building", "people", "arrow"],
    "transaction": ["handshake", "arrow_right", "document"],

    # Growth / trend
    "growth":      ["trending", "arrow_circle_up", "chart_trending", "arrow_up"],
    "decline":     ["trending_down", "arrow_down", "arrow_circle_down"],
    "increase":    ["arrow_up", "trending", "add", "arrow_circle_up"],
    "decrease":    ["arrow_down", "trending_down", "subtract"],
    "improve":     ["arrow_up", "checkmark", "trending", "sparkle"],
    "target":      ["flag", "arrow_right_circle", "checkmark_circle", "bullseye"],
    "goal":        ["flag", "star", "checkmark_circle", "target"],

    # Business / org
    "company":     ["building", "office", "organization", "briefcase"],
    "enterprise":  ["building", "organization", "briefcase", "globe"],
    "industry":    ["building_factory", "organization", "briefcase"],
    "market":      ["chart", "globe", "trending", "people"],
    "competitor":  ["people", "arrow_bidirectional", "flag"],
    "strategy":    ["chess", "flag", "lightbulb", "arrow_right"],
    "leadership":  ["star", "people", "award", "crown"],
    "team":        ["people", "group", "person", "organization"],
    "people":      ["person", "people", "group", "organization"],
    "meeting":     ["people", "calendar", "video_conference", "chat"],
    "conference":  ["people", "calendar", "video"],
    "customer":    ["person", "people", "star", "heart"],

    # Technology
    "analytics":   ["chart_multiple", "data_bar", "graph", "trending"],
    "data":        ["database", "chart", "table", "cylinder"],
    "cloud":       ["cloud", "arrow_upload", "sync"],
    "ai":          ["brain", "sparkle", "bot", "wand"],
    "automation":  ["bot", "arrow_repeat", "gear", "wand"],
    "code":        ["code", "window_dev_tools", "cursor", "developer"],
    "network":     ["wifi", "globe", "connect", "diagram"],
    "database":    ["database", "cylinder", "server", "table"],
    "security":    ["lock", "shield", "key", "protect"],

    # Documents / workflow
    "document":    ["document", "file", "page", "text"],
    "report":      ["document", "chart", "text", "clipboard"],
    "contract":    ["document", "pen", "sign", "checkmark"],
    "presentation":["presenter", "screen", "chart", "document"],
    "email":       ["mail", "envelope", "message"],
    "checklist":   ["clipboard_task", "checkmark", "list"],
    "approval":    ["checkmark_circle", "thumbs_up", "approve"],

    # Communication
    "chat":        ["chat", "comment", "bubble", "message"],
    "message":     ["message", "mail", "chat"],
    "notification":["bell", "alert", "badge"],
    "broadcast":   ["megaphone", "speaker", "mail"],

    # Navigation / action
    "search":      ["search", "magnify", "zoom_in"],
    "find":        ["search", "magnify", "location"],
    "filter":      ["filter", "funnel", "sort"],
    "settings":    ["settings", "options", "wrench", "gear"],
    "configure":   ["settings", "wrench", "options"],
    "add":         ["add", "plus", "new"],
    "delete":      ["delete", "dismiss", "trash"],
    "edit":        ["edit", "pen", "pencil", "compose"],
    "share":       ["share", "send", "arrow_up_circle"],
    "download":    ["arrow_download", "save", "arrow_down"],
    "upload":      ["arrow_upload", "send", "cloud"],
    "refresh":     ["arrow_counterclockwise", "sync", "rotate"],
    "sync":        ["arrow_sync", "refresh", "rotate"],
    "link":        ["link", "connect", "chain", "plug"],

    # Status / quality
    "check":       ["checkmark", "checkmark_circle", "approve"],
    "warning":     ["warning", "alert", "triangle_exclamation"],
    "error":       ["dismiss_circle", "warning", "error"],
    "success":     ["checkmark_circle", "checkmark", "approve"],
    "info":        ["info", "information"],
    "help":        ["question_circle", "info", "chat"],
    "risk":        ["warning", "shield", "alert", "triangle"],
    "quality":     ["checkmark_circle", "star", "badge"],
    "award":       ["trophy", "medal", "star", "award"],
    "star":        ["star", "favorite", "sparkle", "award"],

    # Time / schedule
    "time":        ["clock", "timer", "hourglass"],
    "schedule":    ["calendar", "clock", "list"],
    "deadline":    ["calendar", "clock", "flag"],
    "calendar":    ["calendar", "event", "clock"],
    "history":     ["history", "clock", "arrow_counterclockwise"],

    # Location / global
    "global":      ["globe", "earth", "location", "map"],
    "location":    ["location", "map_pin", "globe", "place"],
    "map":         ["map", "globe", "location"],

    # Misc business
    "innovation":  ["lightbulb", "sparkle", "wand", "idea"],
    "idea":        ["lightbulb", "sparkle", "brain"],
    "lightning":   ["flash", "lightning_bolt", "power", "sparkle"],
    "key":         ["key", "lock", "access", "security"],
    "tag":         ["tag", "label", "price_tag"],
    "bookmark":    ["bookmark", "save", "flag"],
    "eye":         ["eye", "view", "preview"],
    "home":        ["home", "house", "building"],
    "phone":       ["phone", "call", "mobile"],
    "image":       ["image", "picture", "photo", "camera"],
    "video":       ["video", "camera", "play"],
    "print":       ["print", "printer", "document"],
    "expand":      ["expand", "full_screen", "arrow_expand"],
    "collapse":    ["minimize", "contract", "arrow_collapse"],
}


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

@dataclass
class IconMatch:
    """One icon in the index."""
    folder_name: str           # e.g. "Arrow Circle Down"
    icon_key: str              # e.g. "arrow_circle_down"
    svgs: dict = field(default_factory=dict)  # {(size:int, style:str): Path}
    score: float = 0.0

    def best_svg(self, preferred_size: int = 24, style: str = "filled") -> Optional[Path]:
        """Return best SVG path: preferred size + style, then cascaded fallback."""
        for sz in [preferred_size, 24, 20, 28, 16, 32, 48]:
            if (sz, style) in self.svgs:
                return self.svgs[(sz, style)]
        # Any style at preferred size
        for st in ["filled", "regular"]:
            if (preferred_size, st) in self.svgs:
                return self.svgs[(preferred_size, st)]
        # Anything at all
        if self.svgs:
            return next(iter(self.svgs.values()))
        return None


# ---------------------------------------------------------------------------
# Main searcher class
# ---------------------------------------------------------------------------

class FluentIconSearcher:
    """
    Enhanced Fluent UI icon search + PNG render.

    Search strategy (multi-pass, scored):
      1. Exact token match against icon name tokens             → 10 pts each
      2. Synonym-expanded token match                          → 6 pts each
      3. Substring containment (token in icon word or vice versa) → 3 pts each
      4. Fuzzy match via SequenceMatcher (ratio > 0.75)        → ratio × 2 pts
      5. Bonus: query first token starts folder name           → +4 pts

    Rendering stack: svglib + reportlab → PNG.
    Render outputs cached in cache_dir keyed by icon_key + color + size.
    """

    DEFAULT_PATHS = [
        Path.home() / "fluentui-system-icons" / "assets",
        Path.home() / "fluent-system-icons" / "assets",
        Path.home() / "Downloads" / "fluentui-system-icons" / "assets",
        Path("fluentui-system-icons") / "assets",
    ]

    def __init__(
        self,
        assets_dir: Optional[str | Path] = None,
        cache_dir: Optional[str | Path] = None,
    ):
        """
        Args:
            assets_dir: path to the `assets/` folder of the cloned
                fluentui-system-icons repo. Falls back to FLUENT_ICONS_DIR
                env var, then common default paths.
            cache_dir: where to store index cache + rendered PNGs.
                Defaults to <assets_dir>/../.icon_cache.
        """
        resolved = self._resolve_assets_dir(assets_dir)
        self.assets_dir = resolved

        if cache_dir is None:
            if resolved is not None:
                cache_dir = resolved.parent / ".icon_cache"
            else:
                cache_dir = Path(tempfile.gettempdir()) / "fluent_icon_cache"
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)

        self._index: Optional[list[IconMatch]] = None
        self._index_path = self.cache_dir / "_icon_index.json"

    def _resolve_assets_dir(self, arg: Optional[str | Path]) -> Optional[Path]:
        if arg is not None:
            return Path(arg)
        env = os.environ.get("FLUENT_ICONS_DIR")
        if env:
            return Path(env)
        for p in self.DEFAULT_PATHS:
            if p.exists():
                return p
        return None

    def is_available(self) -> bool:
        return self.assets_dir is not None and self.assets_dir.exists()

    # -----------------------------------------------------------------------
    # Index management
    # -----------------------------------------------------------------------

    def _build_index(self) -> list[IconMatch]:
        if self.assets_dir is None or not self.assets_dir.exists():
            return []
        icons: list[IconMatch] = []
        for folder in sorted(self.assets_dir.iterdir()):
            if not folder.is_dir():
                continue
            # SVGs live in SVG/ subdir or directly in folder
            svg_dir = folder / "SVG"
            if not svg_dir.is_dir():
                svg_dir = folder

            svgs: dict[tuple[int, str], Path] = {}
            icon_key = ""
            for svg in svg_dir.glob("*.svg"):
                m = re.match(r"ic_fluent_(.+?)_(\d+)_(filled|regular)\.svg$", svg.name)
                if m:
                    key_candidate = m.group(1)
                    size = int(m.group(2))
                    style = m.group(3)
                    svgs[(size, style)] = svg
                    if not icon_key:
                        icon_key = key_candidate

            if svgs and icon_key:
                icons.append(IconMatch(
                    folder_name=folder.name,
                    icon_key=icon_key,
                    svgs=svgs,
                ))
        return icons

    def _save_index(self, icons: list[IconMatch]):
        data = [
            {
                "folder_name": ic.folder_name,
                "icon_key": ic.icon_key,
                "svgs": {f"{k[0]}_{k[1]}": str(v) for k, v in ic.svgs.items()},
            }
            for ic in icons
        ]
        self._index_path.write_text(json.dumps(data, indent=None))

    def _load_index(self) -> Optional[list[IconMatch]]:
        if not self._index_path.exists():
            return None
        try:
            data = json.loads(self._index_path.read_text())
            out = []
            for d in data:
                svgs = {}
                for k_str, v_str in d["svgs"].items():
                    sz, style = k_str.split("_", 1)
                    svgs[(int(sz), style)] = Path(v_str)
                out.append(IconMatch(
                    folder_name=d["folder_name"],
                    icon_key=d["icon_key"],
                    svgs=svgs,
                ))
            return out
        except Exception:
            return None

    @property
    def index(self) -> list[IconMatch]:
        if self._index is None:
            self._index = self._load_index()
            if self._index is None:
                self._index = self._build_index()
                if self._index:
                    self._save_index(self._index)
        return self._index

    def rebuild_index(self):
        """Force index rebuild (use after updating the icon repo)."""
        self._index = None
        if self._index_path.exists():
            self._index_path.unlink()

    # -----------------------------------------------------------------------
    # Search
    # -----------------------------------------------------------------------

    def _tokenize(self, text: str) -> list[str]:
        return [t for t in re.split(r"[\s_\-/]+", text.lower()) if len(t) >= 2]

    def _expand_tokens(self, tokens: list[str]) -> set[str]:
        expanded: set[str] = set(tokens)
        for t in tokens:
            if t in SYNONYM_MAP:
                for syn in SYNONYM_MAP[t]:
                    expanded.update(self._tokenize(syn))
        return expanded

    def _score_icon(self, icon: IconMatch, query_tokens: list[str]) -> float:
        icon_tokens = set(self._tokenize(icon.folder_name) + self._tokenize(icon.icon_key))
        icon_text_full = icon.folder_name.lower() + " " + icon.icon_key.lower()

        expanded = self._expand_tokens(query_tokens)
        original = set(query_tokens)
        score = 0.0

        for qt in expanded:
            is_original = qt in original
            weight_exact = 10.0 if is_original else 6.0
            weight_sub = 3.0 if is_original else 1.5

            if qt in icon_tokens:
                score += weight_exact
            elif any(qt in it or it in qt for it in icon_tokens):
                score += weight_sub
            else:
                best_ratio = max(
                    (difflib.SequenceMatcher(None, qt, it).ratio() for it in icon_tokens),
                    default=0.0,
                )
                if best_ratio > 0.75:
                    score += best_ratio * 2.0

        # Bonus: first query token starts the folder name
        if query_tokens:
            first = query_tokens[0]
            folder_lower = icon.folder_name.lower()
            if folder_lower.startswith(first):
                score += 4.0
            # Bonus: full query is substring of icon key
            joined = "_".join(query_tokens)
            if joined in icon.icon_key:
                score += 8.0

        return score

    def search(self, query: str, top_k: int = 5, style: str = "filled") -> list[IconMatch]:
        """
        Return top_k best-matching icons for the query string.
        Icons are copied with their .score set.
        """
        tokens = self._tokenize(query)
        if not tokens or not self.index:
            return []

        scored: list[tuple[IconMatch, float]] = []
        for ic in self.index:
            s = self._score_icon(ic, tokens)
            if s > 0:
                scored.append((ic, s))

        scored.sort(key=lambda x: -x[1])
        results = []
        for ic, s in scored[:top_k]:
            from copy import copy as _copy
            matched = _copy(ic)
            matched.score = s
            results.append(matched)
        return results

    def list_matches(self, query: str, top_k: int = 10) -> list[tuple[str, float]]:
        """Return (icon_key, score) for debugging / exploring the index."""
        return [(ic.icon_key, ic.score) for ic in self.search(query, top_k=top_k)]

    # -----------------------------------------------------------------------
    # Render
    # -----------------------------------------------------------------------

    def _recolor_svg(self, svg_text: str, color: str) -> str:
        """Replace all fill colors with target hex. Handles fill attr + style + currentColor."""
        svg_text = re.sub(r'fill="(?!none)[^"]*"', f'fill="{color}"', svg_text)
        svg_text = re.sub(r'(fill:\s*)(?!none)[^;}"]+', lambda m: m.group(1) + color, svg_text)
        svg_text = svg_text.replace("currentColor", color)
        return svg_text

    def _render_to_png(self, svg_text: str, size: int, output_path: Path,
                       color: str = "#000000") -> Path:
        """
        Render SVG to PNG with proper transparent background — no halos.

        Technique:
          1. Recolor SVG fill to black
          2. Render black-on-white via reportlab (clean antialiased grayscale)
          3. Use luminance as inverse alpha: alpha = 255 - r
          4. Recolor opaque pixels to target color in PIL

        This works because Fluent UI icons are monochrome — any rendered
        pixel sits on the line from black (full coverage) to white (no
        coverage), which maps cleanly to alpha.
        """
        try:
            from svglib.svglib import svg2rlg
            from reportlab.graphics import renderPM
            from PIL import Image

            black_svg = self._recolor_svg(svg_text, "#000000")

            with tempfile.NamedTemporaryFile(suffix=".svg", delete=False,
                                             mode="w", encoding="utf-8") as f:
                f.write(black_svg)
                tmp = Path(f.name)
            try:
                drawing = svg2rlg(str(tmp))
                if drawing is None:
                    raise ValueError("svg2rlg returned None")
                if drawing.width and drawing.height:
                    sx = size / drawing.width
                    sy = size / drawing.height
                    drawing.width = size
                    drawing.height = size
                    drawing.transform = (sx, 0, 0, sy, 0, 0)

                tmp_png = output_path.with_suffix(".tmp.png")
                renderPM.drawToFile(drawing, str(tmp_png), fmt="PNG",
                                    bg=0xFFFFFF)  # white bg

                hexc = color.lstrip("#")
                tr, tg, tb = int(hexc[0:2], 16), int(hexc[2:4], 16), int(hexc[4:6], 16)

                gray = Image.open(tmp_png).convert("L")
                w, h = gray.size
                rgba = Image.new("RGBA", (w, h), (tr, tg, tb, 0))
                # Alpha = 255 - gray  (black -> 255, white -> 0)
                from PIL import ImageOps
                alpha = ImageOps.invert(gray)
                rgba.putalpha(alpha)
                rgba.save(output_path, "PNG")
                tmp_png.unlink(missing_ok=True)
            finally:
                tmp.unlink(missing_ok=True)
        except Exception:
            try:
                from PIL import Image
                Image.new("RGBA", (size, size), (0, 0, 0, 0)).save(output_path, "PNG")
            except Exception:
                pass
        return output_path

    def render(
        self,
        icon: IconMatch,
        color: str = "#255BE3",
        size: int = 128,
        style: str = "filled",
        output_path: Optional[Path] = None,
    ) -> Optional[Path]:
        """
        Render icon to PNG. Returns output path, or None if no SVG found.
        Results are cached; repeated calls with same args return instantly.
        """
        svg_path = icon.best_svg(preferred_size=24, style=style)
        if svg_path is None or not svg_path.exists():
            return None

        if output_path is None:
            safe_key = re.sub(r"[^\w]", "_", icon.icon_key)
            safe_color = color.lstrip("#").upper()
            output_path = self.cache_dir / f"{safe_key}_{safe_color}_{size}.png"

        if output_path.exists():
            return output_path  # cache hit

        svg_text = svg_path.read_text(encoding="utf-8", errors="replace")
        return self._render_to_png(svg_text, size, output_path, color=color)

    def find_and_render(
        self,
        query: str,
        color: str = "#255BE3",
        size: int = 128,
        style: str = "filled",
        output_path: Optional[Path] = None,
    ) -> Optional[Path]:
        """
        One-shot: search + render. Returns PNG path or None.

        Example:
            path = searcher.find_and_render("deal close", color="#255BE3", size=128)
            if path:
                slide.shapes.add_picture(str(path), Inches(x), Inches(y), Inches(0.4), Inches(0.4))
        """
        if not self.is_available():
            return None
        matches = self.search(query, top_k=1, style=style)
        if not matches:
            return None
        return self.render(matches[0], color=color, size=size,
                           style=style, output_path=output_path)

    def find_and_render_batch(
        self,
        queries: list[str],
        color: str = "#255BE3",
        size: int = 128,
        style: str = "filled",
    ) -> dict[str, Optional[Path]]:
        """Render multiple icons; returns {query: png_path_or_None}."""
        return {q: self.find_and_render(q, color=color, size=size, style=style) for q in queries}

    # -----------------------------------------------------------------------
    # Convenience: module-level singleton for use within pptx_writer
    # -----------------------------------------------------------------------

    _singleton: Optional["FluentIconSearcher"] = None

    @classmethod
    def get_default(cls) -> "FluentIconSearcher":
        """Return a module-level singleton (auto-discovers assets dir)."""
        if cls._singleton is None:
            cls._singleton = cls()
        return cls._singleton

    @classmethod
    def configure(cls, assets_dir: str | Path, cache_dir: Optional[str | Path] = None):
        """Set the default singleton's paths. Call once at app startup."""
        cls._singleton = cls(assets_dir=assets_dir, cache_dir=cache_dir)
        return cls._singleton


# ---------------------------------------------------------------------------
# CLI entry point (for testing)
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        print("Usage: python -m src.research.icon_search <query> [color] [size]")
        print("       python -m src.research.icon_search --index-info")
        sys.exit(1)

    searcher = FluentIconSearcher.get_default()

    if sys.argv[1] == "--index-info":
        print(f"Assets dir : {searcher.assets_dir}")
        print(f"Available  : {searcher.is_available()}")
        print(f"Index size : {len(searcher.index)} icons")
        sys.exit(0)

    query = sys.argv[1]
    color = sys.argv[2] if len(sys.argv) > 2 else "#255BE3"
    size = int(sys.argv[3]) if len(sys.argv) > 3 else 128

    print(f"Searching: '{query}' ...")
    matches = searcher.list_matches(query, top_k=8)
    if not matches:
        print("No matches. Is FLUENT_ICONS_DIR set?")
    else:
        print("Top matches:")
        for key, score in matches:
            print(f"  {key:45s} {score:6.1f}")

        top = searcher.search(query, top_k=1)[0]
        png = searcher.render(top, color=color, size=size)
        print(f"\nRendered: {png}")
