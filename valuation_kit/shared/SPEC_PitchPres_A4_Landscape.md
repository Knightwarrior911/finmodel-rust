# SPEC_PitchPres_A4_Landscape – Citi Pitch Presentation Template

This file is the reverse-engineered specification of the **Citi PitchPres A4 Landscape** template. It documents the template's structural, typographic, and visual conventions in enough detail to produce decks indistinguishable from the original.

This spec is firm-specific (Citi) and **overrides** the generic `SPEC_powerpoint_formatting.md` where conventions differ. The substance of `SPEC_powerpoint_engineering.md` (action titles, citations, verification) still applies.

---

## 1. Template Identity

| Property          | Value |
|-------------------|-------|
| Template name     | PitchPres A4 Landscape |
| Slide dimensions  | 10.83" × 7.50" (780pt × 540pt) – A4-calibrated landscape |
| Master count      | 1 |
| Layouts           | 30 |
| Default deck length | 26 slides (full template walkthrough; actual decks vary) |

The "A4" in the name reflects calibration for A4 paper printing – narrower than US Letter widescreen 13.33" × 7.5" (16:9 standard). Decks built from this template are intended to print cleanly on A4 (210mm × 297mm).

---

## 2. Typography

### 2.1 Fonts
- Headings (title, section dividers): **Citi Sans Display**
- Body text: **Citi Sans Text**
- Footnotes, source lines, table notes: **Citi Sans Condensed** (minimum 8pt)

Fall back to Arial / Arial Narrow if Citi Sans family unavailable on rendering machine.

### 2.2 Type hierarchy

| Element                        | Font                  | Size       | Weight              | Color |
|--------------------------------|-----------------------|------------|---------------------|-------|
| Slide title                    | Citi Sans Display     | ~22-28pt   | Regular / bold      | Citi Ink (#0F1632) |
| Page message (subheader)       | Citi Sans Text        | 14pt       | **Bold**            | Citi Ink |
| Body text                      | Citi Sans Text        | 10-12pt    | Regular             | Citi Ink |
| Subheading                     | Citi Sans Text        | 11pt       | Bold                | Citi Ink |
| Chart title                    | Citi Sans Text        | 12pt       | **Bold**            | **Citi Blue** (#255BE3) |
| Chart subtitle                 | Citi Sans Text        | 11pt       | Bold                | Citi Ink |
| Chart units                    | Citi Sans Text        | 8pt        | Regular             | Citi Ink (no paragraph space before) |
| Chart axis labels              | Citi Sans Text        | Condensed min 8pt | Regular       | Citi Ink |
| Table title                    | Citi Sans Text        | 12pt       | **Bold**            | **Citi Blue** |
| Table column header            | Citi Sans Text        | 10pt       | Bold                | (per fill) |
| Table body                     | Citi Sans Text        | 9-10pt     | Regular             | Citi Ink |
| Footnote / source line         | Citi Sans Condensed   | 8pt        | Regular             | Citi Ink |
| Footer / page number           | Citi Sans Text        | 7-8pt      | Regular             | Citi Ink |
| Section divider title          | Citi Sans Display     | ~36pt      | Regular             | Citi Ink |
| Tombstone tile body            | Citi Sans Text        | 8pt        | Regular             | (per fill) |
| Tombstone deal size            | Citi Sans Text        | 8pt        | **Bold**            | (per fill) |
| Color swatch label             | Citi Sans Text        | 7-8pt      | Regular             | (per fill) |

### 2.3 Vertical alignment
- Page message: top vertical alignment, may extend to two lines
- Body content: top alignment within content placeholder
- Tombstone / team tiles: top alignment, padded

---

## 3. Color Palette

### 3.1 Primary (theme) colors

| Name                  | Hex         | Theme slot       | Usage |
|-----------------------|-------------|------------------|-------|
| Citi Ink (Body text)  | `#0F1632`   | dk1 / accent2 / tx1 | Body text, primary headings, dark fill on tiles |
| White (Background)    | `#FFFFFF`   | lt1 / bg1        | Slide background, light text on dark fills |
| Citi Red              | `#FF3C28`   | dk2 / tx2        | Negative emphasis only – sparingly |
| Gray 03               | `#D3DADD`   | lt2 / bg2        | Dividers, subtle shading, hist/proj separation |
| Citi Blue             | `#255BE3`   | accent1          | Primary brand, headlines, table headers, top of org charts |
| Blue Light            | `#73C2FC`   | accent3          | Secondary blue, supporting elements |
| Gray 01               | `#A4ACAF`   | accent4          | Borders, connectors (0.5pt), supporting tiles |
| Forest Bright         | `#388A42`   | accent5          | Pros (positive) emphasis |
| Blue Green Light      | `#80CE84`   | accent6          | Secondary green, supporting green |
| Hyperlink             | `#245BE2`   | hlink / folhlink | Hyperlinks (visited and unvisited) |

### 3.2 Utility palette (shading / fills)

| Name       | Hex         | Usage |
|------------|-------------|-------|
| Gray 04    | `#E6EBED`   | Subsection shading, neutral fills, modular layout fill option |
| Gray 03    | `#D3DADD`   | Hist/proj divider, subtle shading |
| Gray 02    | `#BCC5C9`   | Mid-gray |
| Gray 01    | `#A4ACAF`   | Border / outline color (0.5pt) |
| Sand 04    | `#EAE0D3`   | Subsection shading (warmer alternative to Gray 04), modular layout fill option |
| Sand 03    | `#DFD0BE`   | Lighter sand |
| Sand 02    | `#D5C1A8`   | Mid sand |
| Sand 01    | `#BB9A71`   | Dark sand |

### 3.3 Extended palette
For chart series beyond the 6 default brand colors, draw from this extended palette:

- Tan Bright `#FAB728`
- Tan `#916024`
- Yellow Light `#FEDD58`
- Yellow `#FFFC00`
- Yellow Dark `#E5A824`
- Orange Light `#FFA15A`
- Orange `#FF5C0B`
- Orange Dark `#B23802`
- Red Light `#FF7671`
- Red Dark `#751308`
- Green Light `#B6DC62`
- Green `#970D00` (Note: appears truncated in source)
- Green Dark `#335525`
- Blue Green `#00AF68`
- Blue Green Dark `#194044`
- Plum Light `#F98AC9`
- Plum `#D71671`
- Plum Dark `#871A4E`
- Purple Light `#C599FF`
- Purple `#8E319C`
- Purple Dark `#56225A`

### 3.4 Color use rules
- **Citi Blue** is the primary brand color. Use for headlines, table headers, top-of-hierarchy boxes, primary chart series.
- **Citi Ink** is the body text color and the secondary-emphasis fill (e.g., second-tier org chart boxes).
- **Citi Red** is used only for negative highlights; never for body text or decorative fills.
- **Gray 01** (`#A4ACAF`) is the standard outline color at 0.5pt for modular layouts, table borders, and connector lines.
- **Gray 04** (`#E6EBED`) and **Sand 04** (`#EAE0D3`) are the two shading fills for emphasizing modules / sections.
- The **bold colored squares** in the swatch slide (Forest Bright, Blue Light, etc.) are theme accents 3-6. Use these for chart series 3-6 in priority order, then go to the extended palette for series 7+.

---

## 4. Slide Layout Grid

All measurements in points (1pt = 1/72 inch). Slide dimensions: 780pt × 540pt.

### 4.1 Standard content slide
- **[Title]** — x=21.5, y=17.7, w=737, h=56.7 (← Title zone ~22-28pt CSD)
- **[Page message]** — x=21.5, y=80, w=737, h=40.5 (← Subheader, 14pt CST bold)
- **[Content area]** — x=12.8, y=131.1, w=754.0, h=365.6 (or split: w=368.5 each)
- Body extends to y=496.7
- **[Footer]** — x=21.5, y=515.6, w=639.6, h=14.2

### 4.2 Two-column split
- **Left**: x=12.8, y=131.1, w=368.5, h=365.6
- **Right**: x=398.5, y=131.1, w=368.5, h=365.6
- Gap between columns: ~17pt

### 4.3 Quad split (2 × 2 grid)
- Top-left: x=12.8, y=131.1, w=368.5, h=175.7
- Top-right: x=398.5, y=131.1, w=368.5, h=175.7
- Bottom-left: x=12.8, y=321.0, w=368.5, h=175.7
- Bottom-right: x=398.5, y=321.0, w=368.5, h=175.7
- Gap between rows: ~14pt

### 4.4 Mixed split (text panel + two stacked exhibits)
- **Left** (full height text panel): x=12.8, y=131.1, w=368.5, h=365.6
- **Right top**: x=398.5, y=131.1, w=368.5, h=175.7
- **Right bottom**: x=398.5, y=321.0, w=368.5, h=175.7

Used on chart-formatting slides where the left panel carries instructions and the right panel carries example exhibits.

### 4.5 Section divider slide
- **[Title]** — x=21.5, y=215.7, w=737, h=56.7 (Centered vertically, Citi Sans Display, large)
- **[Subtitle]** — x=88.3, y=297.1, w=670.2, h=28.3 (Optional)

Section dividers number sections as '1.', '2.', etc., with sub-sections labeled 'A.', 'B.', and sub-sub-sections labeled 'i.', 'ii.'. Tab-separated from the title (e.g., '1.\tSection name').

### 4.6 Cover (title) slide
- [Citi Business Name | Subgroup Name] — x=33, y=30, 12pt CST
- [Presentation title (printer friendly)]
- [Presentation subtitle (optional)]
- [Mandatory swap footnote] — x=33, y=402, w=711, h=61 (10pt Citi Sans Text, bold)
- [Date | Strictly private and confidential] — x=569, y=495, 10pt

The cover’s mandatory swap footnote is required when the deck contains references to swaps (per Citi compliance). The boilerplate is shipped pre-populated in the template – the user must either complete it with the appropriate swap product partner or remove it if no swap references exist.

---

## 5. Standard Slide Types

### 5.1 Cover slide
- Title + subtitle + business name + date
- Includes mandatory swap footnote (compliance)
- Confidentiality marking at bottom right

### 5.2 Table of Contents
- Numbered hierarchy (1, 2, 3 + A, B + i, ii) with page references
- Tab-separated formatting: "1.\tSection name\t3"

### 5.3 Section divider
- Layout type "title"
- Single centered title (large CSD)
- Optional subtitle
- Numbered consistently with TOC

### 5.4 Standard text + bullets
- Title + page message + full-width content area
- Bullet phrases concise; max 3 lines per bullet
- 3 levels of bullet hierarchy supported

### 5.5 Pros / Cons / Neutral 3-column
- Three columns for evaluative content
- **Pros column**: Forest Bright (`#388A42`) text/bullets
- **Cons column**: Citi Red (`#FF3C28`) text/bullets
- **Neutral column**: Gray 04 (`#E6EBED`) fill, default text color

### 5.6 Quad page
- 2 × 2 grid of text panels
- Each panel: heading + 3 bullets
- 0.5pt Gray 01 outline around each panel

### 5.7 Modular layout slides (slide 3)
- Demonstrates the modular system
- Three module styles:
  - **Plain**: white fill, no outline
  - **Outlined**: 0.5pt Gray 01 outline
  - **Filled**: Gray 04 or Sand 04 fill
- Choose based on whether modules need separation

### 5.8 Organisation chart
- Hierarchical box-and-line structure
- **Top tier**: Citi Blue fill, white text – typically the head / primary entity
- **Second tier**: Citi Ink fill, white text – direct reports
- **Third tier**: Blue Light fill, white text – sub-reports / divisions
- **Stat boxes**: Gray 01 fill, white text – financial metrics, ratings
- Connector lines: 0.5pt Gray 01
- Box dimensions: typically 135.7w × 63.7h

### 5.9 Charts
- Chart types supported: column, bar, area, pie / donut
- Maximum 10 series per chart
- Chart Title: 12pt bold Citi Blue
- Chart Units: 8pt Citi Ink, no paragraph space before
- Subtitle: 11pt bold Citi Ink
- 2D charts only – no 3D, no gradients, no graphic effects
- Citi Sans Text or Citi Sans Condensed for axis labels (min 8pt)
- No tick marks; value axis has no line, only values
- Citi logo (small) optional in chart corner per template guidance
- Tints: monochrome (single color, varying lightness) or multi-color tints – created via PitchPres macro

### 5.10 Tables
- Table title: 12pt bold Citi Blue
- Column headings: center-aligned, bold
- Numbers right-aligned; sharing same value share same precision (decimals)
- Footnotes: 8pt Citi Sans Condensed
- Sand 04 fill option for emphasis cells (matrix highlighting)
- Tables may be PowerPoint native, Word, Excel pictures, or tab-delimited

### 5.11 Tombstone page
- Grid of deal tiles
- Tile dimensions: 112w × 113h
- Tile contents (top-to-bottom): `[Deal Status]`, `[Client Logo]`, `[Deal Description]`, `**[Deal Size]**` (bold), `[Announcement Date]`
- Default highlight: Citi Blue fill with white text (use sparingly to draw attention to a featured deal)
- Standard tile: white fill with 0.5pt Gray 01 outline
- Variations: Gray 04 fill, Sand 04 fill – for sub-grouping
- Layout typically 7 columns × 4 rows = 28 tombstone tiles per slide
- Tombstone footer (bottom): notes

### 5.12 Team page
- Team name banner: Citi Ink fill, white text, 11pt bold
- Person tiles: 168w × 69h, white fill, 0.5pt outline, **bold name in Citi Blue**
- Person tile contents: `**[First Last]**` (bold), `[Title]`, `[Segment]`, `[Email address]`, `[Telephone number]`
- Photo placeholders alongside person tiles (white background, optional gray outline)
- Multi-team layout: e.g., 3 columns of teams, multiple people per team

---

## 6. Modular Layout System

The template emphasizes a **modular layout system** as its core organizing principle.

### 6.1 Concept
Instead of monolithic slides, content is composed of "modules" – rectangular zones with consistent positioning. Modules can be:
- **Outlined**: 0.5pt Gray 01 (`#A4ACAF`) border, white fill – visual separation without heavy weight
- **Filled (neutral)**: Gray 04 (`#E6EBED`) fill – for instructional / supporting content
- **Filled (warm)**: Sand 04 (`#EAE0D3`) fill – for emphasis or grouped content
- **Plain**: no outline, no fill – when separation is unnecessary

### 6.2 Module dimensions (standard breakpoints)
- Full width: w=754
- Half width: w=368.5 (gap of 17pt between two halves at x=12.8 and x=398.5)
- Quarter: w=368.5, h=175.7 (with 14pt vertical gap between quarters)

### 6.3 Choosing a module style

| Need                              | Style |
|-----------------------------------|-------|
| Strong visual separation          | Outlined (0.5pt Gray 01) |
| Highlight a panel                 | Gray 04 fill |
| Emphasize a key insight or grouped section | Sand 04 fill |
| No separation needed              | Plain (no outline, no fill) |

---

## 7. Bullet Styles

The template supports up to 3 levels of bullets within Pros and Cons categories. Bullet hierarchy:

- Level 1 (12pt, brand color or Citi Ink)
- Level 2 (11pt, indented)
- Level 3 (10pt, further indented)

For Pros: bullets in Forest Bright (`#388A42`).  
For Cons: bullets in Citi Red (`#FF3C28`).  
For Neutral: bullets in Citi Ink (`#0F1632`) on Gray 04 fill.

Bullet phrases should be concise and generally limited to **three lines** in length per bullet.

---

## 8. Footer Conventions

Every content slide carries a footer at:
- Position: x=21.5, y=515.6
- Width: 639.6, height: 14.2

Typical footer content (per Citi banker convention):
- Confidentiality marking ("Strictly private and confidential")
- Sometimes a deal / project name (often top-right or in footer)
- Page number (often top-right or in footer)

The footer placeholder is empty in the template – it’s populated per deck.

---

## 9. Numbering and Section Hierarchy

Section numbering follows a strict hierarchy:

**Top-level sections:**
1. Modular layout
2. Common slides
3. Resources

**Sub-sections:**
A. Text and layouts
B. Charts and layouts

**Sub-sub-sections:**
i. Charts
ii. Tables

Each level gets its own section divider slide. The Table of Contents preserves the hierarchy with tab-separated indentation.

---

## 10. Must-Follow Rules

In addition to shared/SPEC_powerpoint_engineering.md:
- **Use Citi Sans family** (Display for headings, Text for body, Condensed for footnotes). Do not substitute Calibri or Arial unless rendering machine lacks the font.
- **Slide dimensions are A4 landscape** (10.83" × 7.50") – do not change to widescreen 16:9 unless explicitly authorized.
- **Standard slide layout grid** (Section 4.1) – title, page message, content area, footer in fixed positions.
- **Modular layout system** (Section 6) – content lives in modules with consistent fills / outlines.
- **Color palette restricted** to the theme / utility / extended palettes (Section 3); no off-brand colors.
- **Pros / Cons color discipline**: Forest Bright for pros, Citi Red for cons; do not invert.
- **Tombstone tile structure** (Section 5.11): 5-line content with **bold** deal size; consistent tile size.
- **Org chart hierarchy** (Section 5.8): Citi Blue → Citi Ink → Blue Light → Gray 01 from top to bottom.
- **Chart conventions** (Section 5.9): 2D only, no 3D, no gradients, max 10 series, 12pt bold Citi Blue title.
- **Table conventions** (Section 5.10): center-aligned column headers, right-aligned numbers, 8pt Citi Sans Condensed footnotes.
- **Confidentiality marking** in the footer of every content slide.
- **Mandatory swap footnote** on cover slide if deck references swaps; otherwise removed.
- **Section numbering hierarchy** (Section 10): 1/2/3 → A/B → i/ii.
- **0.5pt Gray 01 outlines** for modular borders and table borders (where applied).

---

## 11. Final Acceptance

| Check | Required |
|-------|----------|
| Slide dimensions 10.83" × 7.50" (A4 landscape) | yes |
| Citi Sans Display (headings) and Citi Sans Text (body) used throughout | yes |
| Citi Sans Condensed for all footnotes (8pt) | yes |
| Slide title at standard position (x=21.5, y=17.7) on every content slide | yes |
| Page message zone populated (x=21.5, y=80) on every content slide | yes |
| Footer at standard position (x=21.5, y=515.6) on every content slide | yes |
| Confidentiality marking in footer | yes |
| Color palette restricted to Citi theme / utility / extended palettes | yes |
| Pros / Cons color discipline followed | yes |
| Mandatory swap footnote present on cover OR explicitly removed | yes |
| Charts: 2D, max 10 series, 12pt bold Citi Blue title | yes |
| Tables: center-aligned column headers, right-aligned numbers, Citi Sans Condensed footnotes | yes |
| Tombstone tiles consistent size (112×113pt) and structure | yes |
| Team tiles consistent size (168×69pt), bold name in Citi Blue | yes |
| Org chart hierarchy color-coded (Blue / Ink / Light Blue / Gray) | yes |
| Section dividers use Citi Sans Display, large, centered | yes |
| Section numbering follows 1/2/3 → A/B → i/ii hierarchy | yes |
| 0.5pt Gray 01 outlines for borders and connectors | yes |
| Verified visually before delivery (per shared/SPEC_powerpoint_engineering.md) | yes |

---

## 13. Common Errors
- **Wrong slide dimensions** – using widescreen 13.33×7.5 instead of A4 10.83×7.5
- **Substituted fonts** – Calibri or Arial used instead of Citi Sans family without fallback rationale
- **Page message zone empty** – leaving the 14pt subheader blank on a content slide
- **Off-brand colors** – using non-palette colors for chart series or fills
- **Pros / Cons inverted** – green for cons or red for pros
- **Tombstone tiles inconsistent size** – eyeballed instead of 112×113pt
- **Org chart all one color** – losing the hierarchy signal
- **3D charts** – ever (always 2D)
- **More than 10 series** in one chart
- **Footnote font wrong** – using Citi Sans Text 10pt instead of Citi Sans Condensed 8pt
- **Mandatory swap footnote left as placeholder** – neither completed nor removed
- **Section numbering inconsistent** – mixing styles across the deck
- **Modular borders varying weight** – should be 0.5pt Gray 01 universally
- **Footer missing or wrong position** – should be at x=21.5, y=515.6 every content slide

---

**End of document.** This is the complete, unabridged transcription. All details from the provided images are included. Let me know if you need any further clarification.