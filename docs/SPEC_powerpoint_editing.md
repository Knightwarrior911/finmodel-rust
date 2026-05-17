# SPEC: PowerPoint Editing

> Companion to `SPEC_powerpoint_engineering.md`, `SPEC_powerpoint_formatting.md`,
> `SPEC_powerpoint_layout_decisions.md`, and `SPEC_PitchPres_A4_Landscape.md`.
> This spec covers **modifying an existing `.pptx`** (text swaps, image
> replacements, recoloring, layout tweaks, slide management, chart updates)
> rather than building from archetypes.

Implemented by:
- `src/research/pptx_inspector.py` — read/inspect/clone
- `src/research/pptx_editor.py` — in-place edits, safe slide ops
- `src/research/pptx_render.py` — preview render (LibreOffice / PowerPoint COM)
- `src/research/pptx_writer.py` — full rebuild path (18 archetypes)

Orchestrator surface (`src/orchestrator.py`):
- `inspect_pptx` — JSON descriptor of every shape on every slide
- `inspect_deck_with_preview` — JSON + slide PNGs visible to the LLM (use for fuzzy visual references)
- `edit_deck_text` — bulk/targeted text replacement preserving formatting
- `replace_deck_image` — swap an image at a known shape
- `manage_deck_slides` — duplicate / delete / reorder via OOXML-aware ops
- `recolor_deck_theme` — rebrand by editing theme color slots in place
- `render_deck_preview` — PNG/PDF render for visual verification
- `build_deck` — full rebuild path (auto-renders on save by default)

Granular shape primitives (Phase 1):
- `move_shape`, `resize_shape` — position / size in inches
- `set_shape_fill`, `set_shape_line` — fill/stroke style
- `set_text_style` — bold/italic/color/size on a run or whole shape
- `add_textbox`, `add_line`, `add_shape_box` — add new content
- `delete_shape` — remove a shape
- `align_shapes`, `distribute_shapes` — arrange a group
- `copy_shape_style` — clone fill/line/font from another shape

Native-table operations (cell-level):
- `swap_table_columns` — swap two columns of a native PPTX table
- `move_table_column` — move a column from index A to index B
- `swap_table_rows` — swap two rows

Semantic macros (Phase 4) — preferred for common MD intents:
- `emphasize` — bold + scale font + brand color ("make X stand out")
- `de_emphasize` — gray + smaller ("tone down X")
- `highlight_row` — fill cells brand color, white bold text
- `add_footnote` — bottom-left footnote with rule line above
- `add_section_label` — corner badge ("DRAFT", "V2", "CONFIDENTIAL")
- `make_callout` — capsule + arrow pointing at a target shape
- `match_brand_style` — apply theme palette from a reference deck

Edit log + replay (Phase 5):
- `get_edit_history` — read recent edit log entries (for "do same on slide N")

---

## 1. Triggers

Apply when modifying an existing `.pptx`:

- "Edit this slide / deck"
- "Update [some text/value]"
- "Replace [logo / headshot / chart]"
- "Recolor / rebrand this deck to [palette]"
- "Add / delete / duplicate / reorder slides"
- "Change this from A4 to widescreen / 16:9"
- "Fix the [issue] on slide N"
- "Combine these two decks"
- "Apply [brand] to this generic deck"
- Any thread where a `.pptx` was previously delivered and follow-up changes are requested

Do **not** trigger when:
- New slide built from scratch with no template → `build_deck` (one of the 18 `pptx_writer.py` archetypes)
- Read/audit/QC only → `inspect_pptx`
- File is `.ppt` (binary legacy) → convert to `.pptx` first

---

## 2. Edit categories

Six categories, each with a different risk profile and technique.

| Category | Examples | Risk | Preferred technique |
|----------|----------|------|---------------------|
| **A. Text content swaps** | Update analyst name, change a metric label, rewrite a bullet | Low | `pptx_editor.replace_text_in_slide()` or `clone_template(text_replacements=…)` |
| **B. Image swaps** | Replace logo, swap headshot, update chart screenshot | Low | `pptx_editor.replace_picture()` |
| **C. Color / theme rebrand** | Apply a new palette, change accent color | Medium | Modify `ppt/theme/theme1.xml` accent slots, or rebuild via `pptx_writer.py` with new `BrandProfile` |
| **D. Layout / shape adjustments** | Move a card, resize a chart, add a divider, restyle a run | Medium | Phase 1 primitives — `move_shape`, `resize_shape`, `set_shape_fill`, `set_shape_line`, `set_text_style`, `add_textbox`, `add_line`, `add_shape_box`, `delete_shape`, `align_shapes`, `distribute_shapes`, `copy_shape_style` |
| **E. Slide structure** | Delete slide 3, duplicate, reorder, combine decks | High | `pptx_editor.duplicate_slide / delete_slide / reorder_slides` (OOXML-aware) |
| **F. Chart / data edits** | Refresh series, change chart type, add a series | High | `pptx_writer.py` chart APIs in a single open/edit/save pass; **never** reopen with `Presentation()` afterwards |

---

## 3. Standard workflow

Every edit follows the same five-step loop.

### 3.1 Inspect

Two modes:

**Plain JSON** when shape IDs are already known:
```bash
python -m src.research.pptx_inspector <path-to.pptx>
python -m src.research.pptx_inspector <path-to.pptx> --no-xml --max-xml=400
```

```python
from src.research.pptx_inspector import inspect_pptx
desc = inspect_pptx("deck.pptx", include_raw_xml=False)
```

**Vision-augmented** when the user gives fuzzy/visual references
("the chart on the right", "the Falcon column", "looks cramped"):

```python
from src.research.pptx_editor import inspect_with_preview
res = inspect_with_preview("deck.pptx", slide_indices=[2, 4], dpi=120)
# res = {"json": <descriptor>, "previews": [{"slide_index": 2, "png": "..."}, ...]}
```

Through the orchestrator, the `inspect_deck_with_preview` tool returns slide
PNGs as image content blocks alongside the shape table — the LLM sees both
and can map fuzzy language to concrete shape IDs before calling primitives.

Use the descriptor to:
- Find the right shape ID for in-place edits
- Confirm layout / placeholder structure
- Detect charts (any shape with `hasChart: true`)

**Charts on a slide change the workflow.** If `inspect_pptx` reports any
`hasChart: true`, NEVER round-trip the file through python-pptx after editing
text/images by other means — it corrupts the embedded chart workbook. For
chart-bearing decks: do all chart edits via `pptx_writer.py` chart APIs in
one open/edit/save pass; do all subsequent text/image edits via OOXML-only
helpers.

### 3.2 Plan

Pick the edit category (A–F). This determines:
- In-place vs. rebuild
- Which technique (python-pptx, OOXML zip surgery, full rebuild)
- Whether to copy the file first (always do this if there is any chance of a destructive op going wrong)

### 3.3 Edit

Make the changes. Keep edits minimal and targeted — do not touch shapes you were not asked to.

### 3.4 Render and view

After every save, render to PNG and visually inspect:

```python
from src.research.pptx_render import render_deck
pngs = render_deck("deck.pptx", out_dir="preview/", dpi=140)
```

`pptx_render` tries backends in this order:
1. `soffice` / LibreOffice (`--headless --convert-to pdf` → `pdftoppm` PNGs)
2. PowerPoint COM (`win32com.client`, Windows only, opens PPT in background)
3. Falls back to returning `None` with a clear error if neither is available

Never declare done without rendering and visually inspecting. python-pptx
geometry does not always match the rendered output — text overflow, overlap,
white-on-white invisible icons, and font fallback only show up on render.

### 3.5 Verify

Run the verification checklist (Section 8), and run `pptx_writer.verify()` on
the saved deck for the R1–R5 binding-rule audit:

```python
from src.research.pptx_writer import verify
qa = verify("deck.pptx")  # {"passed": int, "critical": [...], "minor": [...]}
```

---

## 4. Specific edit patterns

### 4.1 Text content swap (Category A)

**Programmatic, single-run, format-preserving:**

```python
from pptx import Presentation
prs = Presentation("file.pptx")
slide = prs.slides[0]
for shape in slide.shapes:
    if not shape.has_text_frame:
        continue
    for para in shape.text_frame.paragraphs:
        for run in para.runs:
            if run.text == "OLD TEXT":
                run.text = "NEW TEXT"
prs.save("file.pptx")
```

Modifying `run.text` preserves font, size, color, bold/italic. Critical for
brand-template shapes whose font name is not resolvable by python-pptx
(e.g. proprietary fonts referenced via theme).

**Bulk replacement across all slides** (uses `clone_template`):

```python
from src.research.pptx_inspector import clone_template
clone_template(
    template_path="in.pptx",
    output_path="out.pptx",
    text_replacements={"Q1": "Q2", "FY23": "FY24"},
)
```

`clone_template` also accepts `by_shape_id={shape_id: "new text"}` and
`by_placeholder_idx={idx: "new text"}` for surgical edits.

### 4.2 Image swap (Category B)

```python
from src.research.pptx_editor import replace_picture
replace_picture(
    deck_path="file.pptx",
    slide_index=0,
    shape_name="Logo",          # or shape_id=
    new_image_path="/path/to/new_logo.png",
)
```

Internally: locates the existing picture shape, captures `(left, top, width,
height)`, removes the old element, inserts the new image at the same box, and
cleans up the orphan relationship.

### 4.3 Color / theme rebrand (Category C)

For brand recoloring, **do not** call `RGBColor()` directly on template shapes
that use theme references. Changing the underlying theme is what propagates
through. Use `recolor_theme`:

```python
from src.research.pptx_editor import recolor_theme

recolor_theme(
    "deck.pptx",
    palette={
        "accent1": "#255BE3",   # primary brand
        "accent2": "#0F1632",   # ink
        "accent3": "#73C2FC",   # light blue
        "dk1":     "#0F1632",   # body text
        "lt1":     "#FFFFFF",
    },
    # Shapes that hard-code RGB instead of referencing the theme
    # need a global srgbClr swap:
    also_replace_hardcoded={"4472C4": "255BE3", "ED7D31": "0F1632"},
)
```

Valid slot names: `dk1`, `lt1`, `dk2`, `lt2`, `accent1`–`accent6`, `hlink`, `folHlink`.

For partial recoloring (specific shapes only), iterate, identify, and set
fill solid + RGB explicitly:

```python
from pptx.dml.color import RGBColor
from src.research.pptx_editor import iter_named_shapes
for shape in iter_named_shapes(slide, prefix="accent_"):
    shape.fill.solid()
    shape.fill.fore_color.rgb = RGBColor(0x25, 0x5B, 0xE3)
```

For a full rebrand from scratch, use `pptx_writer.py` with a new `BrandProfile`
(see `make_pitchpres_profile()` for the pattern).

### 4.4 Slide management (Category E) — OOXML-aware only

```python
from src.research.pptx_editor import duplicate_slide, delete_slide, reorder_slides

duplicate_slide("deck.pptx", slide_index=0, output_path="deck.pptx")
delete_slide("deck.pptx", slide_index=3, output_path="deck.pptx")
reorder_slides("deck.pptx", new_order=[0, 2, 1, 3], output_path="deck.pptx")
```

**Never** use python-pptx's `sldIdLst.remove()` directly. It leaves orphan
parts (`slides.xml`, `_rels`, etc.) inside the OOXML package, producing
duplicate-name warnings on save and occasionally corrupting the file. The
`pptx_editor` helpers manage relationships, content types, and creation IDs
correctly.

### 4.4b Native PPTX tables (comparison_matrix archetype)

`pptx_writer.py`'s `comparison_matrix` archetype renders as a **native PPTX
table** (`<a:tbl>` element), not as individual cell shapes. This means:

- Phase 1 primitives (`move_shape`, `set_shape_fill`, `set_text_style`,
  etc.) **cannot reach inside table cells** — they target the table shape
  as a whole.
- Use the cell-level helpers instead:
  - `swap_table_columns(deck, slide, shape_name="Matrix", col_a, col_b)`
  - `move_table_column(deck, slide, shape_name="Matrix", col_from, col_to)`
  - `swap_table_rows(deck, slide, shape_name="Matrix", row_a, row_b)`
- These manipulate `<a:tblGrid>` widths and every `<a:tr>`'s `<a:tc>` cells
  via lxml — values and per-cell formatting (fills, fonts) stay bonded
  to their column/row.
- Column 0 in `comparison_matrix` is the metric label; entity columns
  start at index 1.

Example MD comment → action:

> "On slide 3 swap Falcon and Peer A columns"
1. `inspect_deck_with_preview(path, slide_indices=[2])`
2. Locate `Matrix` shape (type TABLE)
3. Read header row: Falcon at col 1, Peer A at col 2
4. `swap_table_columns(path, 2, shape_name="Matrix", col_a=1, col_b=2)`
5. `render_deck_preview` → confirm

### 4.5 Chart-bearing slides (Category F)

If `inspect_pptx` reports any `hasChart: true`:

1. All text / image / shape changes must go via OOXML-only helpers — do not
   re-open and re-save with `Presentation()`.
2. Chart data edits use `pptx_writer.py` chart APIs in a single
   open/edit/save pass; do not re-open the saved file with `Presentation()`
   afterwards.
3. Never edit chart XML directly with `zipfile` + `lxml` — strips metadata
   and breaks OOXML schema validation.

### 4.6 In-place vs. rebuild — when to choose

| Situation | Approach |
|-----------|----------|
| Updating a few text values | In-place |
| Swapping a logo or headshot | In-place |
| Changing one shape's color or position | In-place |
| Adjusting bullet wording | In-place |
| Reorganizing the entire slide composition | Rebuild |
| Adding multiple new shapes that change layout flow | Rebuild |
| User says "redesign this slide" or "change the layout" | Rebuild |
| Two or more rounds of feedback already accumulated | Consider rebuild — fewer side effects than progressive edits |

When rebuilding:
1. Clone the master template via `clone_template()` to preserve theme and
   master-driven layout elements (page numbers, footers, logos).
2. Drop sample slides, add fresh slides via `pptx_writer.py` archetypes.
3. Apply `BrandProfile` for consistent tokens.

---

## 5. Hard rules and gotchas

These are file-corruption or rendering footguns. Bake them in.

1. **Em dashes (—) are banned in body text.** Replace with `:`, `,`, `;`, or `–` per context.
2. **Page messages end with a full stop.** Always.
3. **Never duplicate section names.** If a left visual panel labels each section, the right content tiles must not repeat the label.
4. **Variable-height bands beat fixed-height** when bullet counts vary. Sizing each tile proportional to content prevents overflow.
5. **Always render after every save.** python-pptx geometry differs from rendered output. Subtle issues — wrapping headlines into underline tabs, white-on-white invisible icons, off-by-2pt overlaps — only surface in the rendered PNG.
6. **Verify icons are visible against their background.** A white icon on a white circle is invisible. Always check the contrast pairing before placing.
7. **Use `pptx_editor` slide-management helpers, not python-pptx native.** python-pptx slide deletion produces duplicate-name warnings and orphan parts.
8. **Don't round-trip chart-bearing files through python-pptx.** Open once, edit, save, stop. For post-save fixes use OOXML-only helpers.
9. **Brand-template fonts may not resolve via `font.name`** in python-pptx. They live in the master template's theme references. The only way to keep them is to edit existing template shapes in place (which preserves the reference) or clone shapes from the template via `copy.deepcopy(shape._element)`.
10. **Corner radius on rounded rectangles**: use `adjustments[0]` in the 0.04–0.08 range for a tight modern look. Default 0.16 is too chunky. Match whatever the brand template uses (read it back via `inspect_pptx`).
11. **Honor `pptx_writer.py` R1–R5 on every edit:** action title required, one archetype per slide, auto-cite data slides, visual hierarchy, run `verify()` before declaring done.
12. **Preserve aspect ratio.** `inspect_pptx` reports `dimensions`. Do not change between `16:9`, `4:3`, and `A4_LANDSCAPE` mid-edit unless explicitly asked — it reflows every shape.

---

## 6. Conversational iteration patterns

| User says | What to do |
|-----------|-----------|
| "Replace X with Y" | Single-run text replacement; preserves font/size/color |
| "Remove the bottom banner" | Delete the synthesis banner shape; extend body content downward to fill freed space |
| "Make the page heading shorter" | Edit title placeholder text only; do not touch body |
| "Get rid of the page message" | Either clear the message placeholder text, or rebuild the slide with a no-message layout if the body should extend up |
| "Combine these two slides into one deck" | Rebuild approach: fresh template, add slides, populate each |
| "Change colors to [palette]" | Modify theme accent slots OR rebuild with a new `BrandProfile` |
| "The icons are missing / invisible" | Re-fetch icons in the right color via `find_icon`, or change the background for contrast |
| "Move things up to fill the space" | Restructure vertical geometry — recompute `body_top`, `body_h`, `banner_top` with the freed space |
| "Make 3 versions of this slide" | Add three slides of same content with progressively different layouts |
| "These tiles are overflowing" | Shorten content, increase tile height, or shift to bullet-count-weighted variable heights |
| "These two boxes look distorted" | Replace chunky shapes (e.g., `RIGHT_ARROW`) with cleaner alternatives (capsule, circle + icon) |
| "The tag stands out too much" | Merge the tag into body prose; drop the separate styled paragraph |

---

## 7. How to ask for edits effectively

These phrasings produce the cleanest results:

- **Reference visually:** "the dark blue card on the left", "the second tile in the bottom row", "the title at the top"
- **Be specific about the change:** "change 'Q2' to 'Q3' in the page message" rather than "update the date"
- **Give before/after if structural:** "Replace this bullet: '…' with: '…'"
- **Group related edits:** "On slide 2: change X, fix Y, and also Z" — one inspect-edit-render round is much faster than three.
- **Specify scope when ambiguous:** "Apply this change only to slide 1" — otherwise the change might be applied broadly.
- **Flag rebuilds explicitly:** "Redesign this slide with [direction]" or "Make a different version with [layout style]".

When the shape is unclear, describe the visual context (color, position, surrounding elements) — the agent can then `inspect_pptx` and pick the right shape ID.

---

## 8. Verification checklist (run before declaring done)

- [ ] All requested changes are applied
- [ ] No accidental side-effects on other shapes/slides
- [ ] PDF/PNG preview rendered via `pptx_render.render_deck()` and visually inspected
- [ ] Page numbers, footers, master-driven logos still in place
- [ ] No "duplicate name" warnings on save (means orphan parts — wrong rebuild needed)
- [ ] No invisible elements (white-on-white, blue-on-blue) due to wrong contrast pairing
- [ ] Em dashes replaced if applying that rule
- [ ] No content overflow off the slide edge or into adjacent shapes
- [ ] Charts (if any) still render correctly and have not been corrupted
- [ ] Title and page message are still on-brand (correct color, font, size)
- [ ] `pptx_writer.verify(path)` returns 0 critical issues (R1–R5)
- [ ] Aspect ratio unchanged from input (unless explicitly requested)

---

## 8.1 Cross-slide replay ("do the same on slide 4")

Every edit operation appends to `<deck>.edit_log.jsonl` next to the deck.
Macros log at the macro level (`emphasize`, `highlight_row`, ...) — inner
primitive calls are suppressed via a thread-local depth counter so the log
stays at the right semantic abstraction.

When the user says "do the same on slide N":

1. Call `get_edit_history(path, last_n=N)` to read what was just done.
2. Identify the most recent semantic operation that matches the user's
   reference ("the highlight", "the callout you just added").
3. Re-issue the same operation with `slide_index` set to the new slide.
4. Update shape_id/target_shape_id by re-inspecting the new slide first.

Optional `op_filter` lets the LLM scope history to specific ops:
`get_edit_history(path, op_filter=["emphasize", "highlight_row"])`.

To reset: `clear_edit_history(path)`.

## 9. Regression bed

The `experiments/` directory is the live regression bed. Each `build_*.py`
script renders a known-good deck. After any change to `pptx_editor.py` or
`pptx_render.py`, re-run the relevant experiments and `diff_decks(old, new)`
to confirm fingerprints match.

```python
from src.research.pptx_inspector import diff_decks
print(diff_decks("baseline.pptx", "after_edit.pptx"))
```

---

## 10. Helper module surface

`src/research/pptx_editor.py` exposes the following helpers (see module docstring for full signatures):

```python
# Text
replace_text_in_slide(slide, old: str, new: str) -> int
replace_text_in_deck(deck_path, replacements: dict, output_path=None) -> str
set_placeholder_text(slide, idx: int, text: str) -> None

# Images
replace_picture(deck_path, slide_index, *, shape_name=None, shape_id=None,
                new_image_path: str, output_path=None) -> str

# Slides (OOXML-aware)
duplicate_slide(deck_path, slide_index: int, *, position=None,
                output_path=None) -> str
delete_slide(deck_path, slide_index: int, output_path=None) -> str
reorder_slides(deck_path, new_order: list[int], output_path=None) -> str

# Theme recolor
recolor_theme(deck_path, palette: dict[str, str], *,
              also_replace_hardcoded: dict[str, str] | None = None,
              output_path=None) -> str

# Phase 1 — granular shape primitives (all coords in inches)
move_shape(deck_path, slide_index, *, shape_id|shape_name,
           left=, top=, dx=, dy=, output_path=) -> str
resize_shape(deck_path, slide_index, *, shape_id|shape_name,
             width=, height=, output_path=) -> str
set_shape_fill(deck_path, slide_index, *, shape_id|shape_name,
               color=, no_fill=False, output_path=) -> str
set_shape_line(deck_path, slide_index, *, shape_id|shape_name,
               color=, width=, dash=, no_line=False, output_path=) -> str
set_text_style(deck_path, slide_index, *, shape_id|shape_name,
               paragraph_index=None, run_index=None,
               bold=, italic=, underline=, color=, size=, font_name=,
               text=, output_path=) -> str
add_textbox(deck_path, slide_index, *, left, top, width, height,
            text="", bold=False, italic=False, color=, size=, font_name=,
            name=, output_path=) -> str
add_line(deck_path, slide_index, *, x1, y1, x2, y2,
         color="#000000", width=1.0, dash="solid", name=,
         output_path=) -> str
add_shape_box(deck_path, slide_index, *, kind, left, top, width, height,
              fill=, line=, line_width=, corner=, text="",
              text_color=, text_size=, bold=False, name=,
              output_path=) -> str
delete_shape(deck_path, slide_index, *, shape_id|shape_name,
             output_path=) -> str
align_shapes(deck_path, slide_index, shape_ids: list[int],
             how: str, output_path=) -> str
distribute_shapes(deck_path, slide_index, shape_ids: list[int],
                  axis: str, output_path=) -> str
copy_style(deck_path, slide_index, *,
           source_shape_id, target_shape_id, output_path=) -> str

# Phase 2 — vision-augmented inspect
inspect_with_preview(deck_path, *, slide_indices=None,
                     out_dir=None, dpi=120) -> dict

# Native-table column/row ops (operate on <a:tbl> XML directly)
swap_table_columns(deck_path, slide_index, *, shape_id|shape_name,
                   col_a: int, col_b: int, output_path=None) -> str
move_table_column(deck_path, slide_index, *, shape_id|shape_name,
                  col_from: int, col_to: int, output_path=None) -> str
swap_table_rows(deck_path, slide_index, *, shape_id|shape_name,
                row_a: int, row_b: int, output_path=None) -> str

# Phase 4 — semantic macros
emphasize(deck_path, slide_index, *, shape_id|shape_name,
          brand_color="#255BE3", scale=1.25, bold=True,
          output_path=None) -> str
de_emphasize(deck_path, slide_index, *, shape_id|shape_name,
             mute_color="#999999", scale=0.85,
             output_path=None) -> str
highlight_row(deck_path, slide_index, shape_ids: list[int], *,
              fill_color="#255BE3", text_color="#FFFFFF", bold=True,
              output_path=None) -> str
add_footnote(deck_path, slide_index, text, *,
             color="#666666", size=9, rule_color="#CCCCCC",
             output_path=None) -> str
add_section_label(deck_path, slide_index, text, *,
                  position="top-left", fill="#255BE3",
                  text_color="#FFFFFF", size=10,
                  output_path=None) -> str
make_callout(deck_path, slide_index, target_shape_id, text, *,
             side="right", brand_color="#255BE3",
             width=2.5, height=0.5,
             output_path=None) -> str
match_brand_style(deck_path, ref_deck_path, *,
                  output_path=None) -> str

# Phase 5 — edit log
get_edit_history(deck_path, *, last_n=20,
                 op_filter: list[str] | None = None) -> list[dict]
clear_edit_history(deck_path) -> None

# Shape iteration
iter_named_shapes(slide, *, prefix=None, suffix=None, contains=None)
find_shape_by_id(slide, shape_id: int)
```

`src/research/pptx_render.py`:

```python
render_deck(deck_path, *, out_dir=None, dpi=140,
            backend: Literal["auto","soffice","com"]="auto") -> list[Path]
```

---

## 11. Skill frontmatter (for personal skill registration)

```yaml
name: PPT Editing
description: >
  Modify an existing PowerPoint file in the financial_model project.
  Triggers on requests like "update this deck", "fix slide N", "replace the
  logo", "delete this slide", "combine these decks", or any thread where a
  .pptx was previously delivered and the user follows up with changes.
  Defers to pptx_writer.py archetypes for new-slide construction; encodes
  the inspect → plan → edit → render → verify workflow and the safety rules
  needed to avoid file corruption (chart round-trips, orphan slide parts,
  theme-font loss, geometry-vs-render drift).
tools:
  - inspect_pptx
  - edit_deck_text
  - replace_deck_image
  - manage_deck_slides
  - recolor_deck_theme
  - render_deck_preview
  - build_deck
  - find_icon
```
