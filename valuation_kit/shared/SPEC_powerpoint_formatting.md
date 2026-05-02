# SPEC_powerpoint_formatting - PowerPoint Formatting Standards

This file defines **visual conventions** for PowerPoint output. These are generic IB-style standards.  
**If a firm-specific brand standard exists, it overrides this file.**

---

## 1. Slide Dimensions

- **16:9 widescreen** (13.33" × 7.5") - **default** for new decks (universal standard)
- **4:3** only if mandated by client (legacy template, conference projection, etc.)
- Resolution: 1920×1080 px equivalent, exported at **300 DPI** for print

**Standard margins (16:9 slide):**
- Left / Right: 0.5"
- Top: 0.5"
- Bottom: 0.5"
- Content area: 12.33" × 6.5"

---

## 2. Master Layouts

A typical IB deck uses these layouts:

| Layout              | Use |
|---------------------|-----|
| **Cover**           | Title slide - deck title, sub-title, date, firm logo |
| **Section divider** | Section break - section title, accent image / color |
| **Content (1-up)**  | Standard slide - headline + one primary exhibit + footnote |
| **Content (2-up)**  | Two parallel exhibits side-by-side |
| **Table-heavy**     | Comp tables, financial summaries - minimum chrome |
| **Diagram-heavy**   | Process flows, org charts, transaction structure |
| **Closing / Next Steps** | Conclusion slide |

Apply consistently -- don't invent new layouts mid-deck.

---

## 3. Typography

### 3.1 Default fonts

| Element                  | Font                  | Size     | Weight      |
|--------------------------|-----------------------|----------|-------------|
| Headline / action title  | Arial or Calibri      | 24-28pt  | Bold        |
| Body text                | Arial or Calibri      | 11-14pt  | Regular     |
| Sub-headline / callout   | Same as headline      | 14-18pt  | Bold / Semi-bold |
| Table header             | Same as body          | 10-11pt  | Bold        |
| Table body               | Same as body          | 9-11pt   | Regular     |
| Footnote / source        | Same as body          | 7-8pt    | Italic      |
| Page number / footer     | Same as body          | 8pt      | Regular     |

### 3.2 Font color hierarchy

| Element             | Color |
|---------------------|-------|
| Primary text        | Dark text color (Citi Ink #0F1632 or firm equivalent) |
| Headlines           | Dark text or brand primary |
| Brand emphasis      | Brand primary (Citi Blue #255BE3 or firm equivalent) |
| Negative numbers    | Standard dark text in parentheses (red sparingly) |
| Footnote / source   | Mid-gray |

**Avoid**: Red text in body (reserve for material negatives only); pure black; decorative colors outside the brand palette.

### 3.3 Font weight conventions
- **Bold**: Headlines, table headers, key subtotals
- **Regular**: Body text, table data, supporting bullets
- **Italic**: Margins, growth rates, footnotes, sources, captions
- **Underline**: Avoid in PowerPoint (suggests hyperlinks); use color for emphasis instead

---

## 4. Color System (defaults; override with firm brand)

### 4.1 Primary palette

| Name                | Hex       | Usage |
|---------------------|-----------|-------|
| Brand Primary (Blue) | `#255BE3` | Headlines, table headers, primary chart series, key emphasis |
| Brand Dark (Ink)    | `#0F1632` | Body text, secondary chart series |
| Brand Accent (Red)  | `#FF3C28` | Negative highlights, sparingly |
| White               | `#FFFFFF` | Slide background, header bar text |

### 4.2 Chart series palette (use in order)

When a chart has multiple data series, use brand colors in priority order:

| Position | Color            | Hex       |
|----------|------------------|-----------|
| 1 (most important) | Brand Primary   | `#255BE3` |
| 2        | Brand Dark       | `#0F1632` |
| 3        | Light Brand      | `#73C2FC` |
| 4        | Mid Gray         | `#A4ACAF` |
| 5        | Forest           | `#388A42` |
| 6        | Light Forest     | `#80CE84` |
| 7        | Tan / Orange     | `#FAB728` |
| 8        | Orange Light     | `#FFA15A` |
| 9        | Purple           | `#8E319C` |
| 10       | Plum / Magenta   | `#D71671` |

> The series representing the firm or target should always be **Brand Primary**.

### 4.3 Utility colors

| Name           | Hex       | Usage |
|----------------|-----------|-------|
| Light Gray     | `#E6EBED` | Alt row shading, sensitivity table headers |
| Mid Gray       | `#D3DADD` | Hist/proj dividers, sensitivity base case |
| Border Gray    | `#A4ACAF` | Borders, outlines, gridlines |
| Sand / Beige   | `#EAE0D3` | Subsection shading, emphasis cells |

---

## 5. Slide Layout (default content slide)

[Headline - action title, 24-28pt bold, brand primary]          ← Top
[Primary exhibit: chart / table / diagram]
[Optional supporting callouts: 2-3 short annotations]
Source: [data sources, 7-8pt italic]                           ← Bottom
[Firm name | Project name | Page X]

- Top (~0.6"): headline area
- Body (~5.5"): primary content + callouts
- Bottom (~0.6"): footer with source line and page number

---

## 6. Cover Slide

[Firm Logo or Mark]
[Deck Title - 36-44pt bold]
[Subtitle / project name - 18-24pt]
[Date - 14pt]
[Confidential markings if applicable]

- Brand-colored background **OR** neutral with brand accent
- Center-aligned title block

---

## 7. Section Dividers

Used between major sections (e.g., "Situation Overview" → "Valuation Analysis").

[Section Number]    [Section Title]
(e.g., "II. Valuation Analysis")

- Brand color background **OR** neutral
- Section number + title centered
- Optional accent imagery / brand mark

---

## 8. Tables (in PowerPoint)

### 8.1 Banker comp table

**Header row**: brand fill, white text, bold, 10pt  
**Summary stats row** (Median / Mean): lightly shaded  
**Target row**: bold + brand fill

**Source**: below table

### 8.2 Financial summary table

Year headers across top.  
Bold for main line items, **italic** for margins/growth.

### 8.3 Table style standards
- **Header row**: brand color fill, white text, bold
- **Body cells**: white background, dark text
- **Alternating rows**: optional light gray (only if improves readability)
- **Subtotals**: bold, thin top border
- **Italics**: margins, growth rates, multiples
- **Numbers**: right-aligned
- **Labels**: left-aligned
- **No vertical gridlines**
- **Subtle horizontal lines** between sections only
- **Consistent decimal places** within a column

---

## 9. Headers and Footers

### 9.1 Footer (every slide except cover)
[Firm name] | [Project name or "Confidential"] | [Page X of Y]
- 7-8pt regular, mid-gray
- Full width, just above bottom margin

### 9.2 Page numbers
- Bottom right **OR** centered (firm convention)
- Format: "X" or "X of Y" - pick one and be consistent

### 9.3 Confidentiality marking
- "**CONFIDENTIAL**" or "**PRIVILEGED & CONFIDENTIAL**" in 7-8pt bold at top right or bottom right
- "**DRAFT**" if not yet final
- "**WORKING DOCUMENT**" for internal team materials

---

## 10. Charts in PowerPoint

### 10.1 Chart sizing
- Take **60-70%** of slide body area
- Leave room for callouts, legend, and source line below
- Don't fill edge-to-edge -- breathing room matters

### 10.2 Chart elements

| Element          | Style |
|------------------|-------|
| Chart title      | Brand color, bold (omit if same as slide headline) |
| Y-axis labels    | 8-9pt, mid-gray |
| Y-axis gridlines | Subtle (light gray, thin) or omitted |
| X-axis labels    | 8-9pt, dark |
| Legend           | 8-9pt, brand colors, top or right |
| Data labels      | 8-9pt; on bars when possible |
| Series colors    | Brand palette in priority order |
| Borders          | Removed |
| Background       | White; no fill |
| Footnote         | 7pt italic, below chart |

### 10.3 Chart annotations
For emphasis, add callouts with connector lines and short insight text in brand color.

---

## 11. Diagrams

### 11.1 Box-and-arrow diagrams
- Boxes: rounded corners (4-8pt radius), brand fill
- Connectors: solid lines, brand color
- Direction: consistent (top-to-bottom **or** left-to-right)
- Equal spacing between sibling elements

### 11.2 Timelines
- Horizontal line with milestones
- Brand-color milestones; mid-gray for past

### 11.3 Org charts
- Top-down hierarchy
- Equal width boxes per level
- Clear reporting lines (avoid crossing)

### 11.4 Maps
- Stylized, not photo-realistic
- Brand colors for highlighted regions
- Source attribution mandatory

---

## 12. Logos
- Light background → dark logo
- Dark background → light/white logo
- Consistent height across a slide
- Preserve aspect ratio
- Always cite source if not the brand's own logo

---

## 13. Headshots
- Square aspect ratio (1:1) preferred
- Consistent size across all faces on a slide
- Real photos only
- Source attribution required

---

## 14. Number Formatting in PowerPoint

Match Excel conventions:

| Type                  | Format |
|-----------------------|--------|
| Dollars in $M         | $#,##0 - no decimals (e.g., $391,035) |
| Plain numbers         | #,##0 |
| Percentages           | 0.0% - one decimal |
| Multiples             | 0.0x - one decimal |
| Per-share             | $0.00 - two decimals |
| Negatives (tables)    | Parentheses (e.g., ($50)) |
| Negatives (charts)    | Minus sign or parentheses (consistent within deck) |

---

## 15. White Space and Density

A slide should feel **calm**, not cluttered.

**Rules of thumb**:
- 30-50% of slide area should be white space
- One primary exhibit per slide
- No more than 5 bullets (if using bullets at all)
- Don't shrink fonts below 11pt -- split into two slides instead

---

## 16. Firm Brand Override

If a firm-specific PowerPoint brand standard exists, it overrides this file.  
Common overrides include colors, fonts, footer style, logo placement, etc.

> The **substance** of `SPEC_powerpoint_engineering.md` (action titles, citations, verification cycle, anti-patterns) is universal and never overridden.

---

*End of document. Use in conjunction with `SPEC_powerpoint_engineering.md`.*
