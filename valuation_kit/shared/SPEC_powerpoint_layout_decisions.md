# SPEC_powerpoint_layout_decisions - How to Design Slides for Ad-Hoc Content

This file defines the **decision-making process** for designing PowerPoint slides when no pre-defined template or standard outline applies. Use alongside **SPEC_powerpoint_engineering.md** and **SPEC_powerpoint_formatting.md**.

**The problem**: A user says *"make me a slide showing X"* where X doesn't map to a standard pitch book / CIM / IC memo template. You need to decide: what exhibit type? How to organize the content spatially? How many companies per slide? When to split into multiple slides?

---

## 1. The Exhibit Selection Decision Tree

Start here. Work top-down.

### Q1: What is the message?
Write it as **one sentence**. This becomes the **action title**.

### Q2: What is the data shape behind the message?
- **(a)** Comparison across entities → **Table or grouped bar**
- **(b)** Trend over time → **Line chart or bar chart**
- **(c)** Breakdown / composition of a total → **Stacked bar, treemap, or pie**
- **(d)** Process / flow / sequence → **Diagram (boxes + arrows)**
- **(e)** Structure / hierarchy → **Org chart or pyramid**
- **(f)** Geography / location → **Map**
- **(g)** Single emphasis (one big number) → **Stat callout**
- **(h)** Range from multiple methods → **Football field**
- **(i)** Bridge from A to B → **Waterfall**
- **(j)** Relationship between two metrics → **Scatter plot**
- **(k)** Text-heavy (multiple quotes / opinions) → **Quote wall or comparison matrix**
- **(l)** Decision matrix (options × criteria) → **Table with checkmarks or ratings**

### Q3: How many entities?
- **1-4** → Fit on one slide (most layouts)
- **5-10** → Single slide if table or bar chart; two slides if each needs a paragraph
- **10-20** → Compact table only (no paragraphs); or split into two slides
- **20+** → Summary slide (top 5) + detail in appendix

### Q4: How much supporting context per entity?
- **(a)** Just a number → Chart or stat
- **(b)** A number + a label → Chart with data labels
- **(c)** A number + a short sentence → Table with annotation column
- **(d)** A paragraph or quote per entity → One entity per slide, or quote wall
- **(e)** Mixed → Table with numbers + text column

---

## 2. The Five Ad-Hoc Slide Archetypes

Most ad-hoc requests map to one of these:

### 2.1 Comparison Matrix

**When**: Comparing N entities on M dimensions (side-by-side analysis).

**Layout**:

[Action title: "Target trades at a 30% discount to peers"]
Company A   Company B   Company C   Target
Revenue   $XX,XXX     $XX,XXX     $XX,XXX     $XX,XXX
EBITDA    $XX,XXX     $XX,XXX     $XX,XXX     $XX,XXX
Margin %  XX.X%       XX.X%       XX.X%       XX.X%
EV/EBITDA X.Xx        X.Xx        X.Xx        X.Xx
Growth %  X.X%        X.X%        X.X%        X.X%
Source: [sources]

**Rules**:
- Entities in columns, metrics in rows (or vice versa -- whichever has fewer items goes horizontally)
- Target / subject highlighted with brand color column shading
- Summary row (median, mean) if >5 entities
- Max ~8 entities × ~8 metrics on one slide

**Example uses**: peer benchmarking, bid comparison, strategic alternatives

### 2.2 Scorecard / Dashboard

**When**: Summarizing multiple dimensions for one entity, often with red/amber/green ratings.

**Layout** (grid of tiles):


**Rules**:
- Entities in columns, metrics in rows (or vice versa -- whichever has fewer items goes horizontally)
- Target / subject highlighted with brand color column shading
- Summary row (median, mean) if >5 entities
- Max ~8 entities × ~8 metrics on one slide

**Example uses**: peer benchmarking, bid comparison, strategic alternatives

### 2.2 Scorecard / Dashboard

**When**: Summarizing multiple dimensions for one entity, often with red/amber/green ratings.

**Layout** (grid of tiles):

[Action title: "Target scores well on 4 of 6 criteria"]
[Revenue $X.XB ●●●●○]  [Margin 34.4% ●●●○○]  [Market #1 Share ●●●●●]
[Growth 12.5% ●●●○○]  [Leverage 3.2x ●●○○○]  [ESG Medium ●●●○○]

**Rules**:
- 4-9 tiles (2×2, 3×2, 3×3 grid)
- Each tile: one metric, one value, optional rating (dots, stars, arrows, or shading)
- Don't over-title tiles -- metric label + value is usually enough

**Example uses**: investment screening criteria, DD progress tracker, management quality assessment

### 2.3 Quote Wall / Commentary Synthesis

**When**: Displaying verbatim management commentary from multiple companies on one theme.

**Layout** (2-column grid of cards):

[Action title: "7 of 10 peers cited tariff pressure in Q4"]
[Logo] Apple                  [Logo] Microsoft
"We expect tariffs to add    "Limited direct exposure,
~50bps of cost pressure      but supply chain partners
in FY26"                     may..."
- CFO, Q4 FY25 call          - CEO, Q4 FY25 call

**Rules**:
- 4-8 companies per slide (2-column grid)
- Each card: logo, company name, verbatim quote (abridged if needed), speaker + date
- Sort by relevance or posture
- Split into multiple slides if >10 companies

**Example uses**: management commentary on tariffs / AI / regulation / capex

### 2.4 Timeline / Event Track

**When**: Showing a sequence of events over time for one or multiple entities.

**Layout**:

[Action title: "$120B of capex announced across hyperscalers in the last 12 months"]
Jan ── Feb ── Mar ── Apr ── May ── Jun ── Jul ...
MSFT   GOOG    AMZN               META
$50B   $20B    $30B               $20B
"AI    "Cloud"  "AWS"              "AI"
infra"

**Rules**:
- Horizontal timeline with milestones
- Max 8-10 milestones per slide
- Color-code by company or event type

**Example uses**: M&A activity tracking, capex announcements, regulatory milestones

### 2.5 Process / Structure Diagram

**When**: Showing how something works (transaction structure, supply chain, operating model, org structure).

**Layout** (boxes + arrows):

[Action title: "Transaction structured as a two-step merger with reverse termination fee protection"]
Sponsor ──→ AcquireCo ──→ Target
(merger)       (public |
sub)            target)

**Rules**:
- Boxes for entities, arrows for flows
- Flow direction: left-to-right **or** top-to-bottom (consistent)
- Label every arrow
- Max 6-8 boxes on one slide

**Example uses**: transaction structure, capital structure, governance structure

---

## 3. The Density Decision

### 3.1 By exhibit type

| Exhibit                | Max comfortable density       |
|------------------------|-------------------------------|
| Comparison table       | 8 entities × 8 metrics        |
| Quote wall             | 8 quotes                      |
| Timeline               | 10 milestones                 |
| Bar chart              | 10 bars                       |
| Line chart             | 5 lines × 20 data points      |
| Pie chart              | 6 slices                      |
| Diagram                | 8 boxes                       |
| Scorecard              | 9 tiles                       |
| Football field         | 8 methods                     |
| Waterfall              | 8 steps                       |

### 3.2 When to split
Split when:
- Text shrinks below 9pt
- Two unrelated messages on one slide
- Reader needs to squint (squint test fails)
- More than 3 callouts competing for attention
- Layout feels forced

### 3.3 How to split
- **Slide 1**: Summary or top-N with the key message
- **Slide 2**: Detail or remainder (label "continued" or a related but distinct headline)
- Each slide still has its own action title

---

## 4. The "No Template" Workflow

When a user asks for a slide that doesn't match any standard template:

1. Write the message as one sentence (this becomes the action title)
2. Identify the data shape (Section 1, Q2)
3. Count the entities and metrics (Section 1, Q3-Q4)
4. Pick the archetype from Section 2 that fits
5. Check density limits (Section 3) - split if needed
6. Design the slide following the archetype layout
7. Apply formatting per `SPEC_powerpoint_formatting.md`
8. Add source line per `SPEC_powerpoint_engineering.md`
9. Verify

> **Key insight**: The **message** determines the exhibit type, not the topic.

---

## 5. Combining Exhibits on One Slide

Two compatible exhibits can coexist if they support the **same headline**.

### 5.1 Compatible combinations

| Primary exhibit (60-70%)     | Supporting exhibit (30-40%)     | When |
|------------------------------|---------------------------------|------|
| Bar chart                    | Small comparison table          | Chart shows trend; table shows values |
| Map                          | Small data table                | Map shows geography; table shows numbers |
| Diagram                      | Callout boxes                   | Diagram shows structure; callouts highlight points |
| Table                        | Small chart                     | Table shows data; chart shows pattern |
| Stat callout                 | Supporting bullets              | Big number is headline; bullets explain why |

### 5.2 Incompatible combinations (don't do these)
- Two charts of equal size
- Chart + long paragraph
- Two tables per slide (unless very small sidebar)
- Chart + unrelated chart

### 5.3 Layout for combined exhibits

[Action title]
[PRIMARY EXHIBIT]          [SUPPORTING EXHIBIT]
(chart, table, diagram)    (small table, callout, list)
Source: [sources]

---

## 6. Color and Emphasis Decisions

### 6.1 When to use color to highlight
- **Brand color**: target / subject company (always brand primary)
- **Gray**: peer companies, context data
- **Red (sparingly)**: negative outcomes, risks
- **Green (sparingly)**: positive outcomes, outperformance

### 6.2 When to use callout boxes
- To draw attention to one specific data point
- To add a "so what?" annotation
- Max 2-3 callouts per slide
- Use connector lines

### 6.3 When to use bold / italic / underline
- **Bold**: key numbers in body text
- **Italic**: source lines, footnotes, captions
- **Underline**: avoid (suggests hyperlinks); use bold + brand color instead

---

## 7. Handling Uncertainty

### 7.1 Partial data
- Show "n/a" or "Not disclosed"
- Include a footnote explaining why

### 7.2 Conflicting findings
- Show both sides
- Label as "Bullish" / "Bearish" / "Mixed"
- Headline should acknowledge the mixed signal

### 7.3 Preliminary / unconfirmed
- Mark with "(preliminary)" or "(estimate)"
- Footnote the basis
- Don't present estimates as facts without flagging

---

## 8. Common Slide Design Mistakes

- No action title
- Two messages on one slide
- Chart chosen poorly
- Overcrowded
- Inconsistent related slides
- No source line on a data slide
- Text-heavy when a chart would work better
- Chart-heavy when a table would work better
- Colors used decoratively
- Quote not verbatim
- Logo wall without structure
- Process diagram too complex
- Timeline too dense
- Supporting exhibit dominates

---

*End of document. Use with the companion engineering and formatting specs for consistent, high-impact ad-hoc slides.*
