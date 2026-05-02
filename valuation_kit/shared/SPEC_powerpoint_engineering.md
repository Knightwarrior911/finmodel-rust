# SPEC_powerpoint_engineering – PowerPoint Engineering Standards

This file defines the **slide-level rules** that govern every deck in the kit. These rules are universal — they apply to pitch books, CIMs, IC memos, lender presentations, management presentations, and company profiles. Deliverable-specific structure lives in each model’s `SPEC_methodology.md`.

---

## 1. Five Binding Rules

These are **blocking**. The verification cycle (Section 8) checks them.

1. **Action titles, not descriptive titles.** Every slide headline conveys the takeaway, not the topic.  
   *"EBITDA expanding 200bps annually"* beats *"EBITDA Margin Trajectory."*
2. **One idea per slide.** Don’t cram. If a slide has two distinct points, split it.
3. **Cite every data point, quote, and chart.** Footnote with source + date in 7pt at slide bottom.
4. **Visual hierarchy enforced.** Headline > body > supporting > footnotes. The reader’s eye should travel top-to-bottom in priority order.
5. **Verify every slide visually before delivery.** The mechanics of verification are in Section 8.

---

## 2. Action Titles (Headlines)

The headline is the most important text on the slide. It carries the takeaway in one line.

### 2.1 What makes a good action title
- **Stands alone**: A reader who only sees the headline gets the message
- **Specific**: Numbers, dates, names — not vague generalities
- **Active voice**: *"Tesla grew automotive revenue 25%"* beats *"Automotive revenue growth"*
- **Single idea**: Don’t pack two takeaways into one line
- **6–14 words**: Long enough to be specific, short enough to fit and read fast

### 2.2 Examples

| Descriptive (avoid)              | Action (preferred)                                      |
|----------------------------------|---------------------------------------------------------|
| "EBITDA Margin Trends"           | "EBITDA margins expanded 230bps over 2022-2024"        |
| "Comparable Companies"           | "Target trades at a 30% discount to peer median EV/EBITDA" |
| "Valuation Summary"              | "DCF and comps converge on $180–$220 per share"        |
| "Risk Factors"                   | "Three primary risks: regulatory, supply chain, execution" |

### 2.3 Headline placement
- Top of slide, **left-aligned** (default) or centered (sometimes)
- Bold, brand color, larger than body text (typically 24–28pt)
- One line, no wrapping unless absolutely necessary

---

## 3. Slide Body – Visual Hierarchy

**Three tiers**:
1. **Primary content** – the main exhibit (chart, table, diagram, large callout)
2. **Supporting content** – annotations, callouts, sub-bullets explaining the primary
3. **Footnote / source line** – at slide bottom, smallest font

Avoid mid-tier clutter: don’t fill empty space just to balance the slide.

### 3.1 Primary content type by slide purpose

| Purpose                        | Primary content type                              |
|--------------------------------|---------------------------------------------------|
| Quantify a trend               | Chart (line / bar / area)                         |
| Compare entities               | Table or grouped bar chart                        |
| Show a process                 | Diagram (boxes + arrows)                          |
| Show a structure               | Org chart or pyramid                              |
| Show geography                 | Map                                               |
| Show a single number           | Big stat callout (e.g., "$2.4B" in giant text)   |
| Show timing                    | Timeline / Gantt                                  |
| Show valuation range           | Football field                                    |

### 3.2 What goes in body
- One primary exhibit per slide
- Maximum 2–3 supporting callouts or annotations
- No more than 5 bullets (and only if a chart/table doesn’t fit better)
- White space is a feature, not a bug — don’t fill it

---

## 4. Citations and Sources

Every slide that contains a data point, quote, or third-party claim **must** carry a source line.

### 4.1 Source line format
At the bottom of the slide, in **7–8pt italics**:
Source: [Source 1]; [Source 2]; [Source 3]
Note: [optional clarifying notes about methodology, definitions, time period]


### 4.2 What needs to be cited
- Every numerical value (revenue, multiples, growth rates, market sizes)
- Every direct quote from management or third-party
- Every chart or table (cite the data source)
- Every market / competitive characterization ("Apple is the leader in…")

### 4.3 Source styles

| Source type               | Citation example                                              |
|---------------------------|---------------------------------------------------------------|
| SEC filing                | "Company 10-K FY2024"                                         |
| Earnings call             | "Company Q4 FY24 earnings call (January 30, 2025)"           |
| Investor presentation     | "Company investor day, March 2025"                            |
| Third-party research      | "Gartner, Magic Quadrant 2024"                                |
| Aggregated data           | "Bloomberg / FactSet, retrieved [date]"                       |
| Internal analysis         | "[Firm] analysis based on [underlying sources]"               |
| Multiple sources          | List with semicolons; longest source last                     |

### 4.4 Citing a chart
The source goes below the chart (or in the slide footnote line). Identify what each series came from if multiple sources fed the chart.

**Example**:
> Source: Apple 10-K (FY2024 Revenue); Apple investor presentation (FY2025E guidance); FactSet consensus (FY2026E–FY2028E); [Firm] analysis (terminal year).

### 4.5 In-document citation tags
When building decks programmatically, use the same `cite:{citationId}` pattern as Excel cell comments. The citation system resolves these to source URLs at render time.

---

## 5. Charts

### 5.1 Chart type by purpose

| Question                                      | Chart type                                      |
|-----------------------------------------------|-------------------------------------------------|
| How has X changed over time?                  | Line chart                                      |
| What’s the size of each component?            | Stacked bar or column                           |
| How do entities compare on metric X?          | Horizontal bar chart                            |
| What’s the breakdown of a total?              | 100% stacked bar (preferred) or pie (sparingly) |
| Two metrics over time?                        | Two-axis line chart (sparingly)                 |
| Distribution?                                 | Box plot or histogram                           |
| Geographic distribution?                      | Map                                             |
| Multiple metrics × multiple entities?         | Heatmap or table with conditional formatting    |
| Valuation range from multiple methods?        | Football field (horizontal bar)                 |
| Bridge from one number to another?            | Waterfall                                       |
| Cumulative buildup over time?                 | Area chart                                      |

### 5.2 Chart standards
- **2D only**, never 3D
- No gradients, shadows, glow effects — clean and flat
- **Brand colors only**, ordered by priority
- **Labels on the bars / lines**, not buried in a legend whenever possible
- Axis with rounded numbers (10, 20, 30 — not 11, 22, 33)
- Y-axis starts at zero for bar charts
- No background gridlines unless they serve a purpose
- Every chart has a **source line** below it
- Chart title describes what is being shown (often same as slide headline)

### 5.3 Football field
A horizontal bar chart showing the implied per-share price range from each valuation method.

### 5.4 Waterfall
Used for value creation bridges, EBITDA bridges, P&L bridges. Use connector lines to show how each component builds.

---

## 6. Tables

### 6.1 Table types

| Use case                        | Table style                                              |
|---------------------------------|----------------------------------------------------------|
| Comp table (peer vs. peer)      | Banker comp table — multi-row header, summary stats row at bottom |
| Financial summary               | Year columns, line item rows; bold subtotals; italic margins/growth |
| Transaction comparable          | One row per deal; consistent column structure            |
| Sources and Uses                | Two-column layout with $ + % + multiple per row          |
| Returns summary                 | Scenarios in columns, metrics (IRR / MOIC) in rows       |

### 6.2 Table formatting
- **Header row**: brand color fill, white text, bold
- **Subtotal rows**: bold, with thin border above
- **Italic** for derived metrics (margins, growth rates, multiples)
- Right-align numeric columns
- Left-align text / label columns
- No vertical gridlines — clean
- Subtle horizontal lines between sections
- Summary stats row (median, mean, range) at bottom of comp tables, lightly shaded
- Source line below the table

### 6.3 Number formatting in tables
Match Excel conventions:
- Dollars in $M: no decimals (e.g., 391,035), parentheses for negatives
- Multiples: one decimal with `x` suffix (e.g., 12.5x)
- Percentages: one decimal with `%` (e.g., 23.5%)
- Per-share: 2 decimals
- "n/a" for not applicable; "NM" for not meaningful

---

## 7. Logos, Headshots, Diagrams, and Maps

### 7.1 Logos
- Use when referencing company entities
- Light background → dark logo; dark background → light logo
- Consistent height across logos on a slide
- Always cite the source if not the company’s own logo

### 7.2 Headshots
- Use for management bios and deal team slides
- Square aspect ratio preferred
- Consistent size across all faces on a slide
- Real photos, not placeholders or stylized illustrations
- Always source attribution

### 7.3 Diagrams (process / structure / org charts)
- Boxes with rounded corners (4–8pt radius)
- Connector lines that don’t cross when possible
- Brand colors only
- Consistent box sizes within the same diagram
- Flow direction: top-to-bottom **or** left-to-right (pick one and stick with it)

### 7.4 Maps
- Stylized, not photo-realistic
- Brand colors for highlighted regions
- Consistent legend
- Source attribution for any data overlay

---

## 8. Verification Cycle

After every deck is built, **verify before delivery**.

### 8.1 Three layers of verification

**1. Structural QA (mechanical)**
- Every slide loads without errors
- Layout consistent across slides
- Page numbers, footers populated
- No placeholder text remaining
- Logos and images render

**2. Visual QA (aesthetic)**
- Headlines fit on one line
- No text overflowing boxes
- Charts render correctly
- Tables are aligned
- No overlapping elements
- Brand consistency across slides

**3. Content QA (substance)**
- Every claim has a source line
- Every chart has a source line
- Every quote has attribution
- Numbers tie out to underlying model / data
- No leftover instructions visible
- Spell check passes
- Math check (totals add up, percentages sum to 100, etc.)

### 8.2 Maximum 3 verification rounds
If the same critical issue persists after 2 fix attempts, reclassify as minor and stop.

### 8.3 Critical vs. minor issues

| Critical (must fix)              | Minor (acceptable to ship)                  |
|----------------------------------|---------------------------------------------|
| Text overflowing slide           | Minor whitespace inconsistency              |
| Missing source line              | Slight color shade variation                |
| Wrong number / data              | Font rendering quirk                        |
| Broken layout                    | Header position 2px off                     |
| Empty placeholder visible        | Subtle alignment imperfection               |
| Spelling error in headline       | Typo in body fixed already                  |
| Logo missing                     | Subtle logo size inconsistency              |
| Math doesn’t add up              | Decimal place inconsistency                 |

---

## 9. Slide Density

Don’t cram.

| Slide type                  | Maximum density                                      |
|-----------------------------|------------------------------------------------------|
| Cover slide                 | Title + subtitle + date + brand                      |
| Section divider             | Section title + image / accent                       |
| Content slide               | 1 primary exhibit + 1–3 callouts + footnote         |
| Comp / data table           | Table + summary stats + source                       |
| Conclusion / next steps     | 3–5 bullets max                                      |

If a slide feels crowded, **split it** — don’t shrink fonts.

### 9.1 The “squint test”
Squint at the slide so details blur. The headline and primary exhibit should still be readable. If the slide loses its message at a squint, redesign.

---

## 10. Brand Consistency

Within a single deck:
- Same fonts (typically one for headlines, one for body)
- Same color palette across all charts and tables
- Same chart style
- Same table style
- Same source line format
- Same logo placement
- Same page number / footer placement

Cross-deck consistency (within a firm or engagement) requires a brand template.

---

## 11. Common Anti-Patterns

- **Descriptive titles** ("Revenue") instead of action titles ("Revenue grew 25%")
- **Wall of bullets** — 8 bullets crammed onto one slide
- **Untitled charts**
- **Sourceless data**
- **Inconsistent fonts**
- **3D pie charts**
- **Multiple primary exhibits** on one slide
- **Dense text on data slides**
- **Generic bullet lists** with no hierarchy
- **Misleading axes** (Y-axis not starting at zero)
- **Logo soup**
- **Placeholder text shipped**
- **Inconsistent decimal places**

---

*End of document. Pair with `SPEC_powerpoint_formatting.md` for detailed visual style rules.*