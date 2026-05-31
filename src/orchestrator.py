"""
Virtual Financial Analyst — Top-Level LLM Orchestrator.

Receives natural-language queries, plans steps using Claude, dispatches
to sub-systems as tools, synthesizes results. Every current capability —
research, financial modeling, DCF, comps, EV bridge — is a callable tool.
New features added to the project automatically become available by
registering a tool here.

Entry points:
  run_sync(query, ticker, company)   — blocking, for CLI use
  run(query, ticker, company)        — async, for embedding
"""

import asyncio
import json
import logging
import os
from typing import Optional

import anthropic

# Load .env from project root if present (convenient for local dev)
try:
    from dotenv import load_dotenv
    load_dotenv(os.path.join(os.path.dirname(__file__), "..", ".env"), override=False)
except ImportError:
    pass

logger = logging.getLogger(__name__)

_MODEL = "claude-opus-4-7"

_SYSTEM = """You are a senior investment banking analyst with access to financial data tools.

When given a query, reason about the best approach, use the available tools to gather data, and deliver a precise, source-backed answer.

Tool selection rules:
- US companies → try `search_sec_edgar` first (fast, 1-2s, authoritative XBRL data)
- Non-US companies or missing EDGAR data → use `run_browser_pipeline` (slow, 30-120s, use sparingly)
- News, M&A terms, regulatory approvals → `search_web` then `fetch_page` for the top hit
- Full 3-statement model → `run_financial_model`
- DCF or price target → `run_dcf`
- EV calculation → `run_ev_bridge`
- Peer multiples table → `run_public_comps`
- Run independent tools in the same turn — the harness executes them in parallel.
- Only invoke `run_browser_pipeline` when faster paths fail or for non-US annual report extraction.

PowerPoint edit workflow (when modifying an existing deck):

Step 1 — INSPECT WITH VISION (always first when user gives a fuzzy/visual cue):
  Use `inspect_deck_with_preview` for any request that references shapes
  by visual description ("the chart on the right", "the dark blue card",
  "the Falcon column", "tighten this slide", "looks cramped"). The tool
  returns slide PNGs you can SEE plus the shape table mapping every visible
  element to a concrete shape_id. Use plain `inspect_pptx` only when you
  already know the shape and just need the JSON.

Step 2 — PLAN: pick the smallest set of primitives that satisfy the user's
intent. Prefer surgical edits (move/resize/restyle) over rebuild.

Step 3 — EDIT. Prefer named macros over chains of primitives when a macro fits:

  Macros (preferred for common MD intents):
    `emphasize`         — bold+scale+brand color (MD: "make X stand out")
    `de_emphasize`      — gray+smaller (MD: "tone down X")
    `highlight_row`     — fill cells brand color, white bold text
    `add_footnote`      — bottom-left footnote with rule line above
    `add_section_label` — corner badge ("DRAFT", "V2", "CONFIDENTIAL")
    `make_callout`      — capsule + arrow pointing at a shape
    `match_brand_style` — apply theme palette from a reference deck

  Primitives (when no macro fits):
    Position    → `move_shape`, `resize_shape`, `align_shapes`, `distribute_shapes`
    Style       → `set_shape_fill`, `set_shape_line`, `set_text_style`, `copy_shape_style`
    Add         → `add_textbox`, `add_line`, `add_shape_box`
    Remove      → `delete_shape`
    Bulk text   → `edit_deck_text`
    Image swap  → `replace_deck_image`
    Theme       → `recolor_deck_theme`
    Slides      → `manage_deck_slides` (duplicate/delete/reorder)
    Full rebuild→ `build_deck` (when restructuring a slide's archetype)

Step 4 — RENDER & REFLECT (mandatory after every edit batch):
  Call `render_deck_preview` AND THEN re-inspect via `inspect_deck_with_preview`
  on the affected slides. Look at the PNG. Did the change match the user's
  intent? If overlap, overflow, white-on-white, or wrong shape edited —
  edit again. Loop up to 3 iterations before reporting.

Chart-bearing slides: do all chart data edits via `build_deck` in one pass;
do not round-trip the file through python-pptx after.

Coordinate system: all primitives use INCHES (floats). Standard 16:9 deck is
13.33" wide × 7.5" tall. 4:3 is 10" × 7.5". A4 landscape is 11.69" × 8.27".

Cross-slide replay: every edit is logged to `<deck>.edit_log.jsonl` next to
the deck. When the user says "do the same on slide N", "apply that to slide
M too", or refers back to "the last change", call `get_edit_history` first
to read the actual operations that were performed, then re-issue them
targeting the new slide. Macro names ('emphasize', 'highlight_row', etc.)
appear in the log at the right semantic level — replay them as macros, not
as their internal primitive expansions.
"""

_TOOLS = [
    {
        "name": "search_sec_edgar",
        "description": (
            "Fetch financial data directly from SEC EDGAR XBRL API. Fast (1-2s). "
            "Use first for any US public company. Returns revenue, EBIT, net income, "
            "total assets, debt, cash, shares, and filing metadata."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "US stock ticker (e.g. AAPL, MSFT)"},
                "form_type": {
                    "type": "string",
                    "enum": ["10-K", "10-Q", "8-K"],
                    "description": "Filing type. Omit for latest annual data.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "search_web",
        "description": (
            "Search DuckDuckGo and return the top result URLs + snippets. "
            "No browser — pure HTTP (instant). Use for news, M&A deals, "
            "non-US company info, regulatory approvals, ownership data."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use operators: site:, filetype:, after: when useful.",
                },
            },
            "required": ["query"],
            "additionalProperties": False,
        },
    },
    {
        "name": "fetch_page",
        "description": (
            "Fetch a specific URL via direct HTTP and return the page text. "
            "Fast, no browser. Use after search_web to read a specific article, "
            "press release, IR page, or regulatory filing."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Full URL to fetch"},
            },
            "required": ["url"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_browser_pipeline",
        "description": (
            "Full browser pipeline: finds the company's annual report PDF, downloads it, "
            "extracts 40+ financial fields (P&L, balance sheet, IFRS 16 leases, EV bridge items, "
            "pension, D&A). SLOW (30-120s). Use only for non-US companies or when EDGAR has no data."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "company": {"type": "string", "description": "Company name"},
                "year": {"type": "string", "description": "Fiscal year (e.g. '2024')"},
                "ticker": {"type": "string", "description": "Ticker symbol if known"},
                "country": {"type": "string", "description": "Country of listing if known"},
            },
            "required": ["company", "year"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_financial_model",
        "description": (
            "Build a full 3-statement financial model (IS, BS, CF) and export to Excel. "
            "Takes 30-90s. Use when the user explicitly wants a model or Excel file."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
                "periods_historical": {
                    "type": "integer",
                    "description": "Historical periods. Default 3.",
                    "default": 3,
                },
                "periods_projected": {
                    "type": "integer",
                    "description": "Projected periods. Default 5.",
                    "default": 5,
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional path for the output .xlsx file.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_dcf",
        "description": (
            "Run a DCF valuation (WACC, terminal value, implied EV and equity value) "
            "for a US-listed company. Returns key valuation metrics."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_ev_bridge",
        "description": (
            "Compute Enterprise Value bridge: Market Cap → EV via net debt, minority interest, "
            "pension obligations, lease liabilities. Returns all EV bridge components."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Stock ticker"},
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "run_public_comps",
        "description": (
            "Build a trading comps table for a company and its sector peers. "
            "Returns EV/EBITDA, P/E, EV/Revenue multiples for target + peers."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "ticker": {"type": "string", "description": "Target company ticker"},
                "peers": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional peer tickers. If omitted, uses curated peer list.",
                },
            },
            "required": ["ticker"],
            "additionalProperties": False,
        },
    },
    {
        "name": "build_deck",
        "description": (
            "Build an IB-style PowerPoint deck from structured slide specs. "
            "Each slide spec is a dict with 'type' (cover|section|comparison|"
            "scorecard|quote_wall|timeline|process|strategy|bar_chart|"
            "football_field|line_chart|waterfall|stacked_bar|pie|pros_cons|"
            "quad_page|org_chart|tombstone_page|team_page|table_of_contents) "
            "and type-specific keys. Returns saved deck path. "
            "Optional brand_pdf to clone visual style from a sample firm deck. "
            "Pass 'markdown' (multi-doc YAML stream) instead of 'slides' to "
            "build from a flat text spec."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "filename": {
                    "type": "string",
                    "description": "Output filename without extension"
                },
                "firm": {"type": "string", "description": "Firm name in footer"},
                "project": {"type": "string", "description": "Project name"},
                "confidentiality": {
                    "type": "string",
                    "description": "CONFIDENTIAL / DRAFT / empty to suppress"
                },
                "brand_pdf": {
                    "type": "string",
                    "description": "Optional path/URL to a sample firm deck PDF "
                                   "(BrandProfile.from_pdf), or 'pitchpres' for "
                                   "the Citi A4 PitchPres preset."
                },
                "headline_bold": {
                    "type": "boolean",
                    "description": "Bold action titles (default true)"
                },
                "auto_render": {
                    "type": "boolean",
                    "description": (
                        "Auto-render PDF + per-slide PNGs after save for "
                        "visual verification (default true). Falls back "
                        "silently if no render backend is available."
                    ),
                },
                "slides": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": (
                        "List of slide specs. Each MUST have 'type' key. "
                        "Mutually exclusive with 'markdown'. Examples: "
                        "{type:'cover', title:'...', subtitle:'...', date:'...'}; "
                        "{type:'section', section_num:'I', title:'Setup'}; "
                        "{type:'comparison', action_title:'...', entities:[...], "
                        "metrics:[...], values:[[...]], target_label:'...', source:'...'}; "
                        "{type:'line_chart', action_title:'...', x_labels:[...], "
                        "series:[{label,values}], target_series:'...', y_format:'...', source:'...'}; "
                        "{type:'waterfall', action_title:'...', "
                        "segments:[{label,value,kind}], source:'...'}; "
                        "{type:'stacked_bar', action_title:'...', categories:[...], "
                        "series:[{label,values,color?}], target_category:'...', source:'...'}; "
                        "{type:'pie', action_title:'...', "
                        "slices:[{label,value,color?}], target_label:'...', source:'...'}"
                    ),
                },
                "markdown": {
                    "type": "string",
                    "description": (
                        "Multi-doc YAML stream (separated by '---'). Each doc is "
                        "one slide spec with 'type' and type-specific keys. "
                        "Mutually exclusive with 'slides'."
                    ),
                },
            },
            "required": ["filename"],
            "additionalProperties": False,
        },
    },
    {
        "name": "inspect_pptx",
        "description": (
            "Reverse-engineer a PowerPoint template: extracts slide dimensions, "
            "theme colors, fonts, shape positions, fills, text formatting, and "
            "layout type per slide. Use when the user supplies a template to match."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the .pptx file to inspect",
                },
            },
            "required": ["path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "edit_deck_text",
        "description": (
            "Edit text in an existing PowerPoint deck while preserving font, "
            "size, color, and bold/italic. Pass `replacements` as a dict of "
            "{old: new} strings; applies across every slide. Use for analyst "
            "name changes, metric updates, date refreshes, bulk wording fixes. "
            "Run `inspect_pptx` first if unsure which strings appear."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to .pptx to edit"},
                "replacements": {
                    "type": "object",
                    "description": "Map of {old_string: new_string}",
                    "additionalProperties": {"type": "string"},
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional output path (overwrites input by default)",
                },
            },
            "required": ["path", "replacements"],
            "additionalProperties": False,
        },
    },
    {
        "name": "replace_deck_image",
        "description": (
            "Swap a picture shape in an existing deck with a new image, "
            "preserving the original position and size. Identify the target by "
            "either `shape_name` or `shape_id` (one is required). Use `inspect_pptx` "
            "first to locate the right shape."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer", "description": "0-based slide index"},
                "new_image_path": {"type": "string"},
                "shape_name": {"type": "string"},
                "shape_id": {"type": "integer"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "new_image_path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "manage_deck_slides",
        "description": (
            "Duplicate, delete, or reorder slides via OOXML-aware ops "
            "(no orphan parts, no duplicate-name warnings). "
            "operation: 'duplicate' (uses slide_index, optional position), "
            "'delete' (uses slide_index), or 'reorder' (uses new_order list "
            "of 0-based indices, must be a permutation)."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "operation": {
                    "type": "string",
                    "enum": ["duplicate", "delete", "reorder"],
                },
                "slide_index": {"type": "integer"},
                "position": {
                    "type": "integer",
                    "description": "For duplicate: insert position (0-based). Default: end.",
                },
                "new_order": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "For reorder: permutation of 0..n-1.",
                },
                "output_path": {"type": "string"},
            },
            "required": ["path", "operation"],
            "additionalProperties": False,
        },
    },
    {
        "name": "recolor_deck_theme",
        "description": (
            "Rebrand a deck by editing its theme color slots in place. Pass "
            "`palette` as a dict of {slot: hex_color} where slot is one of "
            "dk1, lt1, dk2, lt2, accent1..accent6, hlink, folHlink. Shapes "
            "that reference theme colors update automatically. For shapes "
            "with hard-coded RGB, also pass `replace_hardcoded` as a "
            "{old_hex: new_hex} map for a global srgbClr swap."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "palette": {
                    "type": "object",
                    "description": "Map of {slot: hex_color}",
                    "additionalProperties": {"type": "string"},
                },
                "replace_hardcoded": {
                    "type": "object",
                    "description": "Optional map of {old_hex: new_hex}",
                    "additionalProperties": {"type": "string"},
                },
                "output_path": {"type": "string"},
            },
            "required": ["path", "palette"],
            "additionalProperties": False,
        },
    },
    {
        "name": "inspect_deck_with_preview",
        "description": (
            "Inspect a deck AND render PNG previews of slides, returned to "
            "you as visible images. Use this whenever the user makes a fuzzy "
            "visual reference ('the chart on the right', 'the dark blue card', "
            "'tighten this slide'). Always inspect-with-preview before any "
            "shape primitive call (move_shape, resize_shape, etc.) so you can "
            "map the user's language to a concrete shape_id. Optional "
            "slide_indices to limit which slides to render."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_indices": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "0-based slide indices. Omit for all slides.",
                },
                "dpi": {"type": "integer", "default": 120},
            },
            "required": ["path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "move_shape",
        "description": (
            "Move a shape on a slide. Pass `left`/`top` for absolute (inches) "
            "or `dx`/`dy` for relative deltas (inches). Identify the shape by "
            "shape_id (preferred) or shape_name."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "left": {"type": "number"},
                "top": {"type": "number"},
                "dx": {"type": "number"},
                "dy": {"type": "number"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "resize_shape",
        "description": "Resize a shape. Width/height in inches.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "width": {"type": "number"},
                "height": {"type": "number"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "set_shape_fill",
        "description": (
            "Set solid fill color (hex). Pass no_fill=true to clear fill."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "color": {"type": "string"},
                "no_fill": {"type": "boolean"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "set_shape_line",
        "description": (
            "Set shape line color (hex), width (pt), dash "
            "('solid'|'dash'|'dot'|'dashdot'|'longdash'). Pass no_line=true "
            "to remove line."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "color": {"type": "string"},
                "width": {"type": "number"},
                "dash": {"type": "string"},
                "no_line": {"type": "boolean"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "set_text_style",
        "description": (
            "Restyle text in a shape. Targets paragraph_index + run_index "
            "(0-based) when given, else applies to all runs. Set any of "
            "bold/italic/underline/color/size (pt)/font_name/text."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "paragraph_index": {"type": "integer"},
                "run_index": {"type": "integer"},
                "bold": {"type": "boolean"},
                "italic": {"type": "boolean"},
                "underline": {"type": "boolean"},
                "color": {"type": "string"},
                "size": {"type": "number"},
                "font_name": {"type": "string"},
                "text": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "add_textbox",
        "description": "Add a textbox at (left, top) sized (width, height) inches.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "left": {"type": "number"},
                "top": {"type": "number"},
                "width": {"type": "number"},
                "height": {"type": "number"},
                "text": {"type": "string"},
                "bold": {"type": "boolean"},
                "italic": {"type": "boolean"},
                "color": {"type": "string"},
                "size": {"type": "number"},
                "font_name": {"type": "string"},
                "name": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": [
                "path", "slide_index", "left", "top", "width", "height",
            ],
            "additionalProperties": False,
        },
    },
    {
        "name": "add_line",
        "description": (
            "Add a straight line connector from (x1,y1) to (x2,y2) inches."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "x1": {"type": "number"},
                "y1": {"type": "number"},
                "x2": {"type": "number"},
                "y2": {"type": "number"},
                "color": {"type": "string"},
                "width": {"type": "number"},
                "dash": {"type": "string"},
                "name": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "x1", "y1", "x2", "y2"],
            "additionalProperties": False,
        },
    },
    {
        "name": "add_shape_box",
        "description": (
            "Add an autoshape. kind: 'rect'|'rrect'|'oval'|'circle'|'capsule'"
            "|'arrow'. Optional fill/line hex, optional inline text."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "kind": {"type": "string"},
                "left": {"type": "number"},
                "top": {"type": "number"},
                "width": {"type": "number"},
                "height": {"type": "number"},
                "fill": {"type": "string"},
                "line": {"type": "string"},
                "line_width": {"type": "number"},
                "corner": {"type": "number"},
                "text": {"type": "string"},
                "text_color": {"type": "string"},
                "text_size": {"type": "number"},
                "bold": {"type": "boolean"},
                "name": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": [
                "path", "slide_index", "kind",
                "left", "top", "width", "height",
            ],
            "additionalProperties": False,
        },
    },
    {
        "name": "delete_shape",
        "description": "Remove a shape from a slide.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "align_shapes",
        "description": (
            "Align a group of shapes. how: "
            "'left'|'right'|'center' (horizontal) or "
            "'top'|'bottom'|'middle' (vertical)."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                },
                "how": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "shape_ids", "how"],
            "additionalProperties": False,
        },
    },
    {
        "name": "distribute_shapes",
        "description": "Even-distribute shapes along x or y axis (need >=3 shapes).",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                },
                "axis": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "shape_ids", "axis"],
            "additionalProperties": False,
        },
    },
    {
        "name": "copy_shape_style",
        "description": "Copy fill, line, and first-run font from source to target shape.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "source_shape_id": {"type": "integer"},
                "target_shape_id": {"type": "integer"},
                "output_path": {"type": "string"},
            },
            "required": [
                "path", "slide_index", "source_shape_id", "target_shape_id",
            ],
            "additionalProperties": False,
        },
    },
    {
        "name": "swap_table_columns",
        "description": (
            "Swap two columns of a NATIVE PPTX table by 0-based index. Use "
            "for comparison matrices when MD says 'swap Falcon and Peer A "
            "columns'. Identify the table by shape_id (preferred) or "
            "shape_name. Note: column 0 is usually the metric label; "
            "entity columns start at index 1."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "col_a": {"type": "integer"},
                "col_b": {"type": "integer"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "col_a", "col_b"],
            "additionalProperties": False,
        },
    },
    {
        "name": "move_table_column",
        "description": (
            "Move a table column from col_from to col_to (0-based indices "
            "in the original layout). Other columns shift to fill. Use when "
            "MD says 'move the Falcon column to the right end'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "col_from": {"type": "integer"},
                "col_to": {"type": "integer"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "col_from", "col_to"],
            "additionalProperties": False,
        },
    },
    {
        "name": "swap_table_rows",
        "description": (
            "Swap two rows of a NATIVE PPTX table by 0-based index. Row 0 "
            "is usually the header."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "row_a": {"type": "integer"},
                "row_b": {"type": "integer"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "row_a", "row_b"],
            "additionalProperties": False,
        },
    },
    {
        "name": "emphasize",
        "description": (
            "Make a shape stand out: bold + scale font up + apply brand color. "
            "Use when MD says 'make Falcon pop', 'highlight this row', "
            "'punch up the headline'. Identify shape by shape_id (preferred) "
            "or shape_name."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "brand_color": {"type": "string", "default": "#255BE3"},
                "scale": {"type": "number", "default": 1.25},
                "bold": {"type": "boolean", "default": True},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "de_emphasize",
        "description": (
            "Tone down a shape: gray out + scale font smaller. Use when MD "
            "says 'de-emphasize this', 'make less prominent'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_id": {"type": "integer"},
                "shape_name": {"type": "string"},
                "mute_color": {"type": "string", "default": "#999999"},
                "scale": {"type": "number", "default": 0.85},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index"],
            "additionalProperties": False,
        },
    },
    {
        "name": "highlight_row",
        "description": (
            "Fill a list of shapes (e.g. cells in a row/column) with brand "
            "color and set text white+bold. Use when MD says 'highlight the "
            "Falcon column' on a comparison matrix."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "shape_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                },
                "fill_color": {"type": "string", "default": "#255BE3"},
                "text_color": {"type": "string", "default": "#FFFFFF"},
                "bold": {"type": "boolean", "default": True},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "shape_ids"],
            "additionalProperties": False,
        },
    },
    {
        "name": "add_footnote",
        "description": (
            "Add a footnote at bottom-left with a thin horizontal rule above. "
            "Position auto-computed. Use when MD says 'add footnote: pre-IFRS "
            "16 basis' or 'cite the source at the bottom'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "text": {"type": "string"},
                "color": {"type": "string", "default": "#666666"},
                "size": {"type": "number", "default": 9},
                "rule_color": {"type": "string", "default": "#CCCCCC"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "text"],
            "additionalProperties": False,
        },
    },
    {
        "name": "add_section_label",
        "description": (
            "Add a small badge label (e.g. 'DRAFT', 'CONFIDENTIAL', 'V2'). "
            "position: 'top-left' / 'top-right' / 'bottom-left' / 'bottom-right'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "text": {"type": "string"},
                "position": {
                    "type": "string",
                    "enum": ["top-left", "top-right", "bottom-left", "bottom-right"],
                    "default": "top-left",
                },
                "fill": {"type": "string", "default": "#255BE3"},
                "text_color": {"type": "string", "default": "#FFFFFF"},
                "size": {"type": "number", "default": 10},
                "output_path": {"type": "string"},
            },
            "required": ["path", "slide_index", "text"],
            "additionalProperties": False,
        },
    },
    {
        "name": "make_callout",
        "description": (
            "Add a capsule callout near a target shape with an arrow line "
            "pointing at it. Use when MD says 'flag the inflection point', "
            "'point out the Q4 spike'. side: 'right' / 'left' / 'top' / 'bottom'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "slide_index": {"type": "integer"},
                "target_shape_id": {"type": "integer"},
                "text": {"type": "string"},
                "side": {
                    "type": "string",
                    "enum": ["right", "left", "top", "bottom"],
                    "default": "right",
                },
                "brand_color": {"type": "string", "default": "#255BE3"},
                "width": {"type": "number", "default": 2.5},
                "height": {"type": "number", "default": 0.5},
                "output_path": {"type": "string"},
            },
            "required": [
                "path", "slide_index", "target_shape_id", "text",
            ],
            "additionalProperties": False,
        },
    },
    {
        "name": "match_brand_style",
        "description": (
            "Apply the theme palette from a reference deck to the target "
            "deck. Reads accent slots from the ref's clrScheme and recolors "
            "the target. Use when MD says 'match our pitch deck style' "
            "or 'rebrand this to look like X'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "ref_deck_path": {"type": "string"},
                "output_path": {"type": "string"},
            },
            "required": ["path", "ref_deck_path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "get_edit_history",
        "description": (
            "Read the last_n entries from a deck's edit log. Use when user "
            "says 'do the same on slide N', 'apply that to slide M too', or "
            "'undo the last change' — call this first to see what was "
            "actually done. Optional op_filter limits to specific operation "
            "names (e.g. ['emphasize', 'highlight_row'])."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "last_n": {"type": "integer", "default": 20},
                "op_filter": {
                    "type": "array",
                    "items": {"type": "string"},
                },
            },
            "required": ["path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "render_deck_preview",
        "description": (
            "Render a .pptx to PDF + per-slide PNGs for visual verification. "
            "Tries LibreOffice (soffice) then PowerPoint COM. Always render "
            "after editing — python-pptx geometry differs from rendered output."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "out_dir": {"type": "string"},
                "dpi": {"type": "integer", "default": 140},
                "backend": {
                    "type": "string",
                    "enum": ["auto", "soffice", "com"],
                    "default": "auto",
                },
            },
            "required": ["path"],
            "additionalProperties": False,
        },
    },
    {
        "name": "find_icon",
        "description": (
            "Search Microsoft Fluent UI System Icons by keyword and render to PNG. "
            "Use when the user asks to add icons to slides or when picking icons "
            "to accompany metrics, process steps, or strategy pillars. "
            "Returns the path to the rendered PNG and the top 5 candidate icon names. "
            "Requires FLUENT_ICONS_DIR env var or cloned repo at default path. "
            "Examples: 'revenue growth', 'deal handshake', 'risk warning', 'analytics chart'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Icon search term, e.g. 'revenue growth' or 'handshake deal'",
                },
                "color": {
                    "type": "string",
                    "description": "Hex color for the icon, e.g. '#255BE3'. Defaults to brand blue.",
                },
                "size": {
                    "type": "integer",
                    "description": "PNG size in pixels (64-512). Default 128.",
                },
            },
            "required": ["query"],
            "additionalProperties": False,
        },
    },
]


# ---------------------------------------------------------------------------
# Tool implementations
# ---------------------------------------------------------------------------

def _tool_search_sec_edgar(ticker: str, form_type: str = "10-K") -> str:
    try:
        from src.research.sec_edgar import SECEdgarClient
        client = SECEdgarClient()
        company, financials = client.get_company_financials(ticker)
        result = {
            "company": company.name,
            "ticker": ticker,
            "currency": getattr(financials, "currency", "USD"),
            "revenue": getattr(financials, "revenue", None),
            "operating_income": getattr(financials, "operating_income", None),
            "net_income": getattr(financials, "net_income", None),
            "total_assets": getattr(financials, "total_assets", None),
            "total_debt": getattr(financials, "total_debt", None),
            "cash": getattr(financials, "cash", None),
            "shares_outstanding": getattr(financials, "shares_outstanding", None),
        }
        return json.dumps({k: v for k, v in result.items() if v is not None}, indent=2)
    except Exception as e:
        return f"SEC EDGAR error for {ticker}: {e}"


def _tool_search_web(query: str) -> str:
    try:
        from src.browser.navigation import BrowserNav
        from src.browser.session import BrowserSession
        nav = BrowserNav(BrowserSession())
        urls = nav.search_urls(query)
        if not urls:
            return "No results found."
        lines = [f"{i+1}. {u}" for i, u in enumerate(urls[:8])]
        return "\n".join(lines)
    except Exception as e:
        return f"Web search error: {e}"


def _tool_fetch_page(url: str) -> str:
    try:
        import requests
        from bs4 import BeautifulSoup
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        }
        resp = requests.get(url, headers=headers, timeout=15)
        text = BeautifulSoup(resp.text, "lxml").get_text(separator=" ", strip=True)
        return text[:8000]
    except Exception as e:
        return f"Fetch error for {url[:80]}: {e}"


async def _tool_run_browser_pipeline(
    company: str, year: str, ticker: str = "", country: str = ""
) -> str:
    try:
        from src.research.browser_pipeline import BrowserPipeline
        pipeline = BrowserPipeline()
        try:
            doc, fin = await pipeline.run_full_pipeline(
                company=company, year=year, country=country, ticker=ticker
            )
            result = {k: v for k, v in fin.__dict__.items()
                      if v is not None and not k.startswith("_")
                      and k not in ("source_sections", "extraction_confidence",
                                    "raw_snippets", "field_sources")}
            result["pdf_url"] = doc.pdf_url
            result["source"] = doc.source
            return json.dumps(result, indent=2, default=str)
        finally:
            await pipeline.close()
    except Exception as e:
        return f"Browser pipeline error for {company}: {e}"


def _tool_run_financial_model(
    ticker: str,
    periods_historical: int = 3,
    periods_projected: int = 5,
    output_path: Optional[str] = None,
) -> str:
    try:
        import subprocess, sys
        out = output_path or f"{ticker.replace('.', '_')}_model.xlsx"
        cmd = [
            sys.executable, "-m", "src.cli",
            "--ticker", ticker,
            "--periods-historical", str(periods_historical),
            "--periods-projected", str(periods_projected),
            "--output", out,
            "--direct",
        ]
        result = subprocess.run(
            cmd,
            capture_output=True, text=True,
            cwd="C:/Users/vinit/Documents/financial_model",
            timeout=180,
        )
        if result.returncode == 0:
            return f"Financial model built: {out}\n{result.stdout[-2000:]}"
        return f"Model build failed:\n{result.stderr[-2000:]}"
    except Exception as e:
        return f"Financial model error: {e}"


def _tool_run_dcf(ticker: str) -> str:
    try:
        from src.research.sec_edgar import SECEdgarClient
        from src.dcf import compute_dcf
        from src.wacc import compute_wacc
        client = SECEdgarClient()
        company, financials = client.get_company_financials(ticker)
        wacc_result = compute_wacc(ticker, financials)
        dcf_result = compute_dcf(financials, wacc_result)
        return json.dumps({
            "company": company.name,
            "wacc": getattr(wacc_result, "wacc", None),
            "terminal_value": getattr(dcf_result, "terminal_value", None),
            "enterprise_value": getattr(dcf_result, "enterprise_value", None),
            "equity_value": getattr(dcf_result, "equity_value", None),
            "implied_price": getattr(dcf_result, "implied_price", None),
        }, indent=2, default=str)
    except Exception as e:
        return f"DCF error for {ticker}: {e}"


def _tool_run_ev_bridge(ticker: str) -> str:
    try:
        from src.research.agent import ev_bridge_sync
        return ev_bridge_sync(ticker)
    except Exception as e:
        return f"EV bridge error for {ticker}: {e}"


def _tool_run_public_comps(ticker: str, peers: Optional[list] = None) -> str:
    try:
        from src.public_comps import build_public_comps
        result = build_public_comps(ticker, peer_tickers=peers)
        return str(result)
    except Exception as e:
        return f"Comps error for {ticker}: {e}"


def _tool_build_deck(
    filename: str,
    slides: Optional[list] = None,
    markdown: str = "",
    firm: str = "",
    project: str = "Confidential",
    confidentiality: str = "CONFIDENTIAL",
    brand_pdf: str = "",
    headline_bold: bool = True,
    auto_render: bool = True,
) -> str:
    """
    Build a PowerPoint deck from structured slide specs or a multi-doc YAML
    markdown stream. Dispatches each slide spec to the matching PPTXDeckWriter
    method.
    """
    try:
        from src.research.pptx_writer import (
            PPTXDeckWriter, BrandProfile, make_pitchpres_profile,
            ScorecardTile, Quote, TimelineEvent, ProcessBox, ProcessArrow,
            FrameworkSection, OrgBox, TombstoneTile, TeamMember, TocEntry,
            parse_deck_markdown, verify,
        )
    except Exception as e:
        return f"Deck builder import error: {e}"

    if markdown and slides:
        return "build_deck error: pass either 'slides' or 'markdown', not both"
    if markdown:
        try:
            slides = parse_deck_markdown(markdown)
        except Exception as e:
            return f"Markdown parse error: {e}"
    if not slides:
        return "build_deck error: need 'slides' or 'markdown'"

    brand = None
    if brand_pdf:
        if brand_pdf.lower() == "pitchpres":
            brand = make_pitchpres_profile()
        else:
            try:
                brand = BrandProfile.from_pdf(brand_pdf)
            except Exception as e:
                return f"Failed to extract brand from {brand_pdf}: {e}"

    deck = PPTXDeckWriter(
        firm=firm, project=project, confidentiality=confidentiality,
        brand=brand, headline_bold=headline_bold,
    )

    type_handlers = {
        "cover": lambda s: deck.add_cover(
            s["title"], subtitle=s.get("subtitle", ""),
            deck_date=s.get("date") or s.get("deck_date"),
        ),
        "section": lambda s: deck.add_section_divider(
            s.get("section_num", "I"), s["title"],
        ),
        "comparison": lambda s: deck.add_comparison_matrix(
            action_title=s["action_title"],
            entities=s["entities"], metrics=s["metrics"], values=s["values"],
            target_label=s.get("target_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            summary_stats=s.get("summary_stats", True),
            skip_source=s.get("skip_source", False),
        ),
        "scorecard": lambda s: deck.add_scorecard(
            action_title=s["action_title"],
            tiles=[ScorecardTile(**t) for t in s["tiles"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "quote_wall": lambda s: deck.add_quote_wall(
            action_title=s["action_title"],
            quotes=[Quote(**q) for q in s["quotes"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "timeline": lambda s: deck.add_timeline(
            action_title=s["action_title"],
            events=[TimelineEvent(**e) for e in s["events"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "process": lambda s: deck.add_process_diagram(
            action_title=s["action_title"],
            boxes=[ProcessBox(**b) for b in s["boxes"]],
            arrows=[ProcessArrow(**a) for a in s.get("arrows", [])],
            direction=s.get("direction", "ltr"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "strategy": lambda s: deck.add_strategy_framework(
            action_title=s["action_title"],
            sections=[FrameworkSection(**sec) for sec in s["sections"]],
            vision=s.get("vision", ""),
            vision_label=s.get("vision_label", "OUR VISION"),
            framework_label=s.get("framework_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "bar_chart": lambda s: deck.add_bar_chart(
            action_title=s["action_title"],
            labels=s["labels"], values=s["values"],
            value_format=s.get("value_format", "{:,.1f}"),
            target_label=s.get("target_label", ""),
            x_label=s.get("x_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "football_field": lambda s: deck.add_football_field(
            action_title=s["action_title"], methods=s["methods"],
            target_value=s.get("target_value"),
            target_label=s.get("target_label", "Current"),
            value_format=s.get("value_format", "${:,.0f}"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "line_chart": lambda s: deck.add_line_chart(
            action_title=s["action_title"],
            x_labels=s["x_labels"], series=s["series"],
            target_series=s.get("target_series", ""),
            y_format=s.get("y_format", "{:,.0f}"),
            y_label=s.get("y_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "waterfall": lambda s: deck.add_waterfall(
            action_title=s["action_title"], segments=s["segments"],
            value_format=s.get("value_format", "{:+,.0f}"),
            y_label=s.get("y_label", ""),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "stacked_bar": lambda s: deck.add_stacked_bar(
            action_title=s["action_title"],
            categories=s["categories"], series=s["series"],
            target_category=s.get("target_category", ""),
            value_format=s.get("value_format", "{:,.0f}"),
            show_totals=s.get("show_totals", True),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "pie": lambda s: deck.add_pie(
            action_title=s["action_title"], slices=s["slices"],
            target_label=s.get("target_label", ""),
            show_pct=s.get("show_pct", True),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "pros_cons": lambda s: deck.add_pros_cons(
            action_title=s["action_title"],
            pros=s["pros"], cons=s["cons"], neutral=s.get("neutral"),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "quad_page": lambda s: deck.add_quad_page(
            action_title=s["action_title"], panels=s["panels"],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "org_chart": lambda s: deck.add_org_chart(
            action_title=s["action_title"],
            boxes=[OrgBox(**b) for b in s["boxes"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "tombstone_page": lambda s: deck.add_tombstone_page(
            action_title=s["action_title"],
            tiles=[TombstoneTile(**t) for t in s["tiles"]],
            cols=s.get("cols", 7),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "team_page": lambda s: deck.add_team_page(
            action_title=s["action_title"],
            team_name=s["team_name"],
            members=[TeamMember(**m) for m in s["members"]],
            cols=s.get("cols", 3),
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", False),
        ),
        "table_of_contents": lambda s: deck.add_table_of_contents(
            action_title=s["action_title"],
            entries=[TocEntry(**e) for e in s["entries"]],
            source=s.get("source", ""), notes=s.get("notes", ""),
            skip_source=s.get("skip_source", True),
        ),
    }

    errors = []
    for i, spec in enumerate(slides):
        st = spec.get("type")
        h = type_handlers.get(st)
        if h is None:
            errors.append(f"slide {i+1}: unknown type '{st}'")
            continue
        try:
            h(spec)
        except Exception as e:
            errors.append(f"slide {i+1} ({st}): {e}")

    path = deck.save(filename)
    qa = verify(path)
    summary = (
        f"Deck saved: {path}\n"
        f"Slides built: {qa['passed']}\n"
        f"QA - critical: {len(qa['critical'])}, minor: {len(qa['minor'])}"
    )
    if errors:
        summary += "\nBuild errors:\n  " + "\n  ".join(errors)
    if qa["critical"]:
        summary += "\nCritical issues:\n  " + "\n  ".join(qa["critical"][:5])
    if qa["minor"]:
        summary += "\nMinor issues (first 5):\n  " + "\n  ".join(qa["minor"][:5])

    if auto_render:
        try:
            from src.research.pptx_render import render_deck
            rendered = render_deck(path)
            summary += f"\nRendered preview ({len(rendered)} file(s)):"
            for p in rendered[:6]:
                summary += f"\n  {p}"
            if len(rendered) > 6:
                summary += f"\n  ... +{len(rendered) - 6} more"
        except Exception as e:
            summary += f"\nAuto-render skipped: {e}"

    return summary


# ---------------------------------------------------------------------------
# Tool dispatch
# ---------------------------------------------------------------------------

def _tool_inspect_pptx(path: str) -> str:
    from src.research.pptx_inspector import inspect_pptx_json
    return inspect_pptx_json(path)


def _tool_edit_deck_text(
    path: str,
    replacements: dict,
    output_path: Optional[str] = None,
) -> str:
    try:
        from src.research.pptx_editor import replace_text_in_deck
        out = replace_text_in_deck(path, replacements, output_path)
        return f"Edited deck saved: {out}\nApplied {len(replacements)} replacement key(s)."
    except Exception as e:
        return f"edit_deck_text error: {e}"


def _tool_replace_deck_image(
    path: str,
    slide_index: int,
    new_image_path: str,
    shape_name: Optional[str] = None,
    shape_id: Optional[int] = None,
    output_path: Optional[str] = None,
) -> str:
    try:
        from src.research.pptx_editor import replace_picture
        out = replace_picture(
            path, slide_index, new_image_path,
            shape_name=shape_name, shape_id=shape_id,
            output_path=output_path,
        )
        return f"Image replaced. Saved: {out}"
    except Exception as e:
        return f"replace_deck_image error: {e}"


def _tool_manage_deck_slides(
    path: str,
    operation: str,
    slide_index: Optional[int] = None,
    position: Optional[int] = None,
    new_order: Optional[list] = None,
    output_path: Optional[str] = None,
) -> str:
    try:
        from src.research.pptx_editor import (
            duplicate_slide, delete_slide, reorder_slides,
        )
        if operation == "duplicate":
            if slide_index is None:
                return "manage_deck_slides error: slide_index required for duplicate"
            out = duplicate_slide(
                path, slide_index, position=position, output_path=output_path,
            )
        elif operation == "delete":
            if slide_index is None:
                return "manage_deck_slides error: slide_index required for delete"
            out = delete_slide(path, slide_index, output_path)
        elif operation == "reorder":
            if not new_order:
                return "manage_deck_slides error: new_order required for reorder"
            out = reorder_slides(path, new_order, output_path)
        else:
            return f"manage_deck_slides error: unknown operation '{operation}'"
        return f"Slides {operation}d. Saved: {out}"
    except Exception as e:
        return f"manage_deck_slides error: {e}"


def _tool_inspect_deck_with_preview(
    path: str,
    slide_indices: Optional[list] = None,
    dpi: int = 120,
) -> dict:
    """
    Returns rich content (text + image blocks) for the LLM. The orchestrator
    detects this dict shape and converts to image content blocks.
    """
    import base64
    try:
        from src.research.pptx_editor import inspect_with_preview
    except Exception as e:
        return {"text": f"inspect_deck_with_preview error: {e}"}
    try:
        result = inspect_with_preview(
            path, slide_indices=slide_indices, dpi=dpi,
        )
    except Exception as e:
        return {"text": f"inspect_deck_with_preview error: {e}"}

    desc = result["json"]
    previews = result["previews"]

    summary_lines = [
        f"Deck: {path}",
        f"Slides: {desc['slideCount']} | Dimensions: {desc['dimensions']}",
        f"Previews attached: {len(previews)}",
        "",
        "Per-slide shape table (slide_index | shape_id | name | type | pos in inches):",
    ]
    for slide in desc["slides"]:
        idx = slide["index"]
        for el in slide["elements"]:
            pos = el.get("pos") or {}
            summary_lines.append(
                f"  {idx} | {el.get('id')} | {el.get('name','')!r} | "
                f"{el.get('type','')} | "
                f"L={pos.get('left','?')} T={pos.get('top','?')} "
                f"W={pos.get('width','?')} H={pos.get('height','?')}"
            )

    images = []
    for prev in previews:
        png_path = prev["png"]
        try:
            with open(png_path, "rb") as f:
                b64 = base64.standard_b64encode(f.read()).decode("ascii")
            images.append({
                "slide_index": prev["slide_index"],
                "data": b64,
                "media_type": "image/png",
            })
        except Exception as e:
            summary_lines.append(f"(failed to read {png_path}: {e})")

    return {
        "text": "\n".join(summary_lines),
        "images": images,
    }


def _tool_move_shape(**kwargs) -> str:
    try:
        from src.research.pptx_editor import move_shape
        out = move_shape(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape moved. Saved: {out}"
    except Exception as e:
        return f"move_shape error: {e}"


def _tool_resize_shape(**kwargs) -> str:
    try:
        from src.research.pptx_editor import resize_shape
        out = resize_shape(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape resized. Saved: {out}"
    except Exception as e:
        return f"resize_shape error: {e}"


def _tool_set_shape_fill(**kwargs) -> str:
    try:
        from src.research.pptx_editor import set_shape_fill
        out = set_shape_fill(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Fill set. Saved: {out}"
    except Exception as e:
        return f"set_shape_fill error: {e}"


def _tool_set_shape_line(**kwargs) -> str:
    try:
        from src.research.pptx_editor import set_shape_line
        out = set_shape_line(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Line set. Saved: {out}"
    except Exception as e:
        return f"set_shape_line error: {e}"


def _tool_set_text_style(**kwargs) -> str:
    try:
        from src.research.pptx_editor import set_text_style
        out = set_text_style(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Text styled. Saved: {out}"
    except Exception as e:
        return f"set_text_style error: {e}"


def _tool_add_textbox(**kwargs) -> str:
    try:
        from src.research.pptx_editor import add_textbox
        out = add_textbox(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Textbox added. Saved: {out}"
    except Exception as e:
        return f"add_textbox error: {e}"


def _tool_add_line(**kwargs) -> str:
    try:
        from src.research.pptx_editor import add_line
        out = add_line(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Line added. Saved: {out}"
    except Exception as e:
        return f"add_line error: {e}"


def _tool_add_shape_box(**kwargs) -> str:
    try:
        from src.research.pptx_editor import add_shape_box
        out = add_shape_box(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape added. Saved: {out}"
    except Exception as e:
        return f"add_shape_box error: {e}"


def _tool_delete_shape(**kwargs) -> str:
    try:
        from src.research.pptx_editor import delete_shape
        out = delete_shape(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape deleted. Saved: {out}"
    except Exception as e:
        return f"delete_shape error: {e}"


def _tool_align_shapes(**kwargs) -> str:
    try:
        from src.research.pptx_editor import align_shapes
        out = align_shapes(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("shape_ids"), kwargs.pop("how"),
            **kwargs,
        )
        return f"Shapes aligned. Saved: {out}"
    except Exception as e:
        return f"align_shapes error: {e}"


def _tool_distribute_shapes(**kwargs) -> str:
    try:
        from src.research.pptx_editor import distribute_shapes
        out = distribute_shapes(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("shape_ids"), kwargs.pop("axis"),
            **kwargs,
        )
        return f"Shapes distributed. Saved: {out}"
    except Exception as e:
        return f"distribute_shapes error: {e}"


def _tool_copy_shape_style(**kwargs) -> str:
    try:
        from src.research.pptx_editor import copy_style
        out = copy_style(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Style copied. Saved: {out}"
    except Exception as e:
        return f"copy_shape_style error: {e}"


def _tool_recolor_deck_theme(
    path: str,
    palette: dict,
    replace_hardcoded: Optional[dict] = None,
    output_path: Optional[str] = None,
) -> str:
    try:
        from src.research.pptx_editor import recolor_theme
        out = recolor_theme(
            path, palette,
            also_replace_hardcoded=replace_hardcoded,
            output_path=output_path,
        )
        return f"Theme recolored. Saved: {out}\nSlots updated: {list(palette.keys())}"
    except Exception as e:
        return f"recolor_deck_theme error: {e}"


def _tool_swap_table_columns(**kwargs) -> str:
    try:
        from src.research.pptx_editor import swap_table_columns
        out = swap_table_columns(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Columns swapped. Saved: {out}"
    except Exception as e:
        return f"swap_table_columns error: {e}"


def _tool_move_table_column(**kwargs) -> str:
    try:
        from src.research.pptx_editor import move_table_column
        out = move_table_column(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Column moved. Saved: {out}"
    except Exception as e:
        return f"move_table_column error: {e}"


def _tool_swap_table_rows(**kwargs) -> str:
    try:
        from src.research.pptx_editor import swap_table_rows
        out = swap_table_rows(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Rows swapped. Saved: {out}"
    except Exception as e:
        return f"swap_table_rows error: {e}"


def _tool_emphasize(**kwargs) -> str:
    try:
        from src.research.pptx_editor import emphasize
        out = emphasize(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape emphasized. Saved: {out}"
    except Exception as e:
        return f"emphasize error: {e}"


def _tool_de_emphasize(**kwargs) -> str:
    try:
        from src.research.pptx_editor import de_emphasize
        out = de_emphasize(
            kwargs.pop("path"), kwargs.pop("slide_index"), **kwargs,
        )
        return f"Shape de-emphasized. Saved: {out}"
    except Exception as e:
        return f"de_emphasize error: {e}"


def _tool_highlight_row(**kwargs) -> str:
    try:
        from src.research.pptx_editor import highlight_row
        out = highlight_row(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("shape_ids"), **kwargs,
        )
        return f"Row highlighted. Saved: {out}"
    except Exception as e:
        return f"highlight_row error: {e}"


def _tool_add_footnote(**kwargs) -> str:
    try:
        from src.research.pptx_editor import add_footnote
        out = add_footnote(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("text"), **kwargs,
        )
        return f"Footnote added. Saved: {out}"
    except Exception as e:
        return f"add_footnote error: {e}"


def _tool_add_section_label(**kwargs) -> str:
    try:
        from src.research.pptx_editor import add_section_label
        out = add_section_label(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("text"), **kwargs,
        )
        return f"Section label added. Saved: {out}"
    except Exception as e:
        return f"add_section_label error: {e}"


def _tool_make_callout(**kwargs) -> str:
    try:
        from src.research.pptx_editor import make_callout
        out = make_callout(
            kwargs.pop("path"), kwargs.pop("slide_index"),
            kwargs.pop("target_shape_id"), kwargs.pop("text"),
            **kwargs,
        )
        return f"Callout added. Saved: {out}"
    except Exception as e:
        return f"make_callout error: {e}"


def _tool_match_brand_style(**kwargs) -> str:
    try:
        from src.research.pptx_editor import match_brand_style
        out = match_brand_style(
            kwargs.pop("path"), kwargs.pop("ref_deck_path"), **kwargs,
        )
        return f"Brand style matched. Saved: {out}"
    except Exception as e:
        return f"match_brand_style error: {e}"


def _tool_get_edit_history(
    path: str,
    last_n: int = 20,
    op_filter: Optional[list] = None,
) -> str:
    try:
        from src.research.pptx_editor import get_edit_history
        entries = get_edit_history(
            path, last_n=last_n, op_filter=op_filter,
        )
        if not entries:
            return f"No edit history found for {path}"
        lines = [f"Edit history for {path} ({len(entries)} entries, oldest first):"]
        for e in entries:
            params = e.get("params", {})
            slide = params.get("slide_index", "-")
            keys = [
                k for k in params
                if k not in ("slide_index", "output_path")
            ]
            summary = ", ".join(
                f"{k}={params[k]}" for k in keys[:5]
            )
            lines.append(
                f"  [{e['ts']}] {e['op']:25s} slide={slide} | {summary}"
            )
        return "\n".join(lines)
    except Exception as e:
        return f"get_edit_history error: {e}"


def _tool_render_deck_preview(
    path: str,
    out_dir: Optional[str] = None,
    dpi: int = 140,
    backend: str = "auto",
) -> str:
    try:
        from src.research.pptx_render import render_deck
        paths = render_deck(path, out_dir=out_dir, dpi=dpi, backend=backend)
        lines = [f"Rendered {len(paths)} file(s):"]
        lines.extend(f"  {p}" for p in paths)
        return "\n".join(lines)
    except Exception as e:
        return f"render_deck_preview error: {e}"


def _tool_find_icon(query: str, color: str = "#255BE3", size: int = 128) -> str:
    from src.research.icon_search import FluentIconSearcher
    searcher = FluentIconSearcher.get_default()
    if not searcher.is_available():
        return (
            "Fluent UI icons not available. "
            "Clone https://github.com/microsoft/fluentui-system-icons and set "
            "FLUENT_ICONS_DIR=/path/to/repo/assets"
        )
    matches = searcher.list_matches(query, top_k=5)
    if not matches:
        return f"No icons found for '{query}'"
    top_match = searcher.search(query, top_k=1)[0]
    png = searcher.render(top_match, color=color, size=size)
    candidates = ", ".join(f"{k} ({s:.0f})" for k, s in matches)
    return (
        f"Best match: {top_match.icon_key} (score {top_match.score:.0f})\n"
        f"PNG: {png}\n"
        f"Top 5 candidates: {candidates}"
    )


async def _execute_tool(tool_name: str, tool_input: dict) -> str:
    """Dispatch a tool call to its implementation."""
    dispatch = {
        "search_sec_edgar": lambda i: _tool_search_sec_edgar(**i),
        "search_web": lambda i: _tool_search_web(**i),
        "fetch_page": lambda i: _tool_fetch_page(**i),
        "run_browser_pipeline": lambda i: _tool_run_browser_pipeline(**i),
        "run_financial_model": lambda i: _tool_run_financial_model(**i),
        "run_dcf": lambda i: _tool_run_dcf(**i),
        "run_ev_bridge": lambda i: _tool_run_ev_bridge(**i),
        "run_public_comps": lambda i: _tool_run_public_comps(**i),
        "build_deck": lambda i: _tool_build_deck(**i),
        "inspect_pptx": lambda i: _tool_inspect_pptx(**i),
        "edit_deck_text": lambda i: _tool_edit_deck_text(**i),
        "replace_deck_image": lambda i: _tool_replace_deck_image(**i),
        "manage_deck_slides": lambda i: _tool_manage_deck_slides(**i),
        "recolor_deck_theme": lambda i: _tool_recolor_deck_theme(**i),
        "inspect_deck_with_preview": lambda i: _tool_inspect_deck_with_preview(**i),
        "move_shape": lambda i: _tool_move_shape(**i),
        "resize_shape": lambda i: _tool_resize_shape(**i),
        "set_shape_fill": lambda i: _tool_set_shape_fill(**i),
        "set_shape_line": lambda i: _tool_set_shape_line(**i),
        "set_text_style": lambda i: _tool_set_text_style(**i),
        "add_textbox": lambda i: _tool_add_textbox(**i),
        "add_line": lambda i: _tool_add_line(**i),
        "add_shape_box": lambda i: _tool_add_shape_box(**i),
        "delete_shape": lambda i: _tool_delete_shape(**i),
        "align_shapes": lambda i: _tool_align_shapes(**i),
        "distribute_shapes": lambda i: _tool_distribute_shapes(**i),
        "copy_shape_style": lambda i: _tool_copy_shape_style(**i),
        "swap_table_columns": lambda i: _tool_swap_table_columns(**i),
        "move_table_column": lambda i: _tool_move_table_column(**i),
        "swap_table_rows": lambda i: _tool_swap_table_rows(**i),
        "emphasize": lambda i: _tool_emphasize(**i),
        "de_emphasize": lambda i: _tool_de_emphasize(**i),
        "highlight_row": lambda i: _tool_highlight_row(**i),
        "add_footnote": lambda i: _tool_add_footnote(**i),
        "add_section_label": lambda i: _tool_add_section_label(**i),
        "make_callout": lambda i: _tool_make_callout(**i),
        "match_brand_style": lambda i: _tool_match_brand_style(**i),
        "get_edit_history": lambda i: _tool_get_edit_history(**i),
        "render_deck_preview": lambda i: _tool_render_deck_preview(**i),
        "find_icon": lambda i: _tool_find_icon(**i),
    }
    try:
        fn = dispatch.get(tool_name)
        if fn is None:
            return f"Unknown tool: {tool_name}"
        result = fn(tool_input)
        if asyncio.iscoroutine(result):
            return await result
        return result
    except Exception as e:
        logger.error(f"Tool dispatch error [{tool_name}]: {e}")
        return f"Tool error: {e}"


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------

class VirtualAnalystOrchestrator:
    """LLM-brain orchestrator. Understands intent, plans steps, calls tools."""

    def __init__(self):
        self._client = anthropic.AsyncAnthropic()

    async def run(
        self,
        query: str,
        ticker: str = "",
        company: str = "",
        max_iterations: int = 10,
    ) -> str:
        """
        Process a natural-language query.
        Returns the final analyst response as a string.
        """
        # Build initial message with context hints
        context_parts = []
        if ticker:
            context_parts.append(f"Ticker: {ticker}")
        if company:
            context_parts.append(f"Company: {company}")
        if context_parts:
            user_content = "\n".join(context_parts) + "\n\n" + query
        else:
            user_content = query

        messages = [{"role": "user", "content": user_content}]
        iterations = 0

        while iterations < max_iterations:
            iterations += 1

            response = await self._client.messages.create(
                model=_MODEL,
                max_tokens=16000,
                thinking={"type": "adaptive"},
                system=_SYSTEM,
                tools=_TOOLS,
                messages=messages,
            )

            if response.stop_reason == "end_turn":
                answer = next(
                    (b.text for b in response.content if b.type == "text"),
                    "Analysis complete.",
                )
                return self._finalize(answer, ticker)

            if response.stop_reason != "tool_use":
                break

            # Append assistant turn (must include tool_use blocks)
            messages.append({"role": "assistant", "content": response.content})

            # Collect all tool calls
            tool_calls = [b for b in response.content if b.type == "tool_use"]
            if not tool_calls:
                break

            # Execute all tools in parallel
            tool_results = await asyncio.gather(*[
                _execute_tool(tc.name, tc.input) for tc in tool_calls
            ])

            logger.info(
                "Tools executed: %s",
                ", ".join(tc.name for tc in tool_calls),
            )

            # Append tool results as user turn. Tools may return either a
            # plain string OR a dict {"text": str, "images": [{data,media_type}]}
            # for vision-augmented results.
            tr_blocks = []
            for tc, result in zip(tool_calls, tool_results):
                if isinstance(result, dict) and "text" in result:
                    content = [{"type": "text", "text": result["text"]}]
                    for img in result.get("images") or []:
                        content.append({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": img.get("media_type", "image/png"),
                                "data": img["data"],
                            },
                        })
                    tr_blocks.append({
                        "type": "tool_result",
                        "tool_use_id": tc.id,
                        "content": content,
                    })
                else:
                    tr_blocks.append({
                        "type": "tool_result",
                        "tool_use_id": tc.id,
                        "content": str(result),
                    })
            messages.append({"role": "user", "content": tr_blocks})

        # Fallback: return last text block if loop exits without end_turn
        for block in reversed(response.content):
            if block.type == "text":
                return self._finalize(block.text, ticker)
        return self._finalize("Analysis complete.", ticker)

    def _finalize(self, answer: str, ticker: str) -> str:
        """Append a Sources & Assumptions provenance appendix when a ticker
        cache with a ledger exists. Never raises — returns answer unchanged on
        any failure or when there is nothing to cite."""
        if not ticker:
            return answer
        try:
            import json
            from pathlib import Path
            from src.sources_report import build_sources_report
            cdir = Path("extraction_cache")
            cpath = cdir / (ticker.replace(".", "_").replace("-", "_") + ".json")
            if not cpath.exists():
                cpath = cdir / (ticker.replace(".", "_") + ".json")
            if not cpath.exists():
                cpath = cdir / (ticker + ".json")
            if not cpath.exists():
                return answer
            cache = json.loads(cpath.read_text(encoding="utf-8"))
            if not (cache.get("__ledger__", {}) or {}).get("entries"):
                return answer
            return answer + "\n\n---\n" + build_sources_report(cache)
        except Exception:
            return answer


# ---------------------------------------------------------------------------
# Convenience sync wrapper (CLI / model.py integration)
# ---------------------------------------------------------------------------

def run_sync(
    query: str,
    ticker: str = "",
    company: str = "",
    max_iterations: int = 10,
) -> str:
    """Blocking wrapper for use from CLI or synchronous code."""
    orchestrator = VirtualAnalystOrchestrator()
    return asyncio.run(
        orchestrator.run(query, ticker=ticker, company=company,
                         max_iterations=max_iterations)
    )
