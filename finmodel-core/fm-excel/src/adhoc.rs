//! Ad-hoc / benchmark research table — port of `src/research/output_writer.py`
//! (`pick_adhoc_layout`, `ColumnSpec`, `LayoutDecision`, `AdHocExcelWriter`).
//!
//! This is the "benchmark numbers from filings into Excel" engine: one row per
//! company (WIDE), one row per data point (LONG), one row per period
//! (TIME_SERIES) or one row per event (EVENT_LOG). The layout is chosen by the
//! SPEC decision tree; comparative tables get a MEDIAN/MEAN/MIN/MAX summary
//! block of live Excel formulas.
//!
//! Rendering targets the shared [`crate::model`] cell-grid + [`crate::render`]
//! engine, so fonts / colors / borders come for free. Content (value / formula /
//! fill) is gated cell-for-cell against a Python-generated oracle
//! (`tieout/build_adhoc_oracle.py` → `ADHOC_bench_snapshot.json`,
//! `tests/adhoc_parity.rs`). Per-cell source citations ride along as notes
//! (provenance), invisible to the content gate exactly like number formats.

use std::collections::HashMap;

use crate::model::{Cell, LABEL, Sheet, TAN, Value, cell_ref};

// ── Palette (mirrors output_writer.py SPEC colors, as 8-hex ARGB) ────────────
/// Title bar — SPEC `BRAND_BLUE` `#2558B3` (distinct from the model's `#255BE3`).
pub const ADHOC_TITLE: &str = "FF2558B3";
/// Column-header band — SPEC `INK` `#0F1632`.
pub const ADHOC_HEADER: &str = "FF0F1632";
/// Summary-stats band — SPEC `LIGHT_GRAY` `#E6EBED`.
pub const ADHOC_SUMMARY: &str = "FFE6EBED";
// Group banner uses SPEC `SAND` `#EAE0D3` == model `TAN`.

// ── Number formats — codes VERBATIM from SPEC §3 Number Formats ──────────────
// Four sections each: positive; (negative); zero "-"; @ text — with `_)` /
// trailing padding so zeros and text right-align under parenthesised negatives.
// Per the spec RULE, ordinary dollar rows are PLAIN (no `$`); `$` is reserved
// for price / per-share cells (NF_PRICE) and would need a section-first row
// selector to apply to statement headers — not done here (codes-only pass).
pub const NF_DOLLAR: &str = "#,##0_);(#,##0);\"-\"_);@_)";
pub const NF_PLAIN: &str = "#,##0_);(#,##0);\"-\"_);@_)";
pub const NF_PCT: &str = "0.0%_);(0.0%);\"-\"_);@_)";
pub const NF_MULT: &str = "0.0\"x\"_);(0.0\"x\");\"-\"_);@_)";
pub const NF_PRICE: &str = "$#,##0.00_);($#,##0.00);\"-\"_);@_)";
pub const NF_SHARES: &str = "#,##0.0,,;\"-\"";

/// A single value in an ad-hoc row.
#[derive(Clone, Debug, PartialEq)]
pub enum CellVal {
    Number(f64),
    Text(String),
    Empty,
}

/// Column data kind — drives the number format and summary-stat eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColKind {
    Text,
    Number,
    Dollar,
    Percent,
    Multiple,
    Price,
    Shares,
    Date,
    Url,
}

impl ColKind {
    /// Number format for a hardcoded input of this kind (`_KIND_TO_NF`).
    fn num_fmt(self) -> Option<&'static str> {
        match self {
            ColKind::Number => Some(NF_PLAIN),
            ColKind::Dollar => Some(NF_DOLLAR),
            ColKind::Percent => Some(NF_PCT),
            ColKind::Multiple => Some(NF_MULT),
            ColKind::Price => Some(NF_PRICE),
            ColKind::Shares => Some(NF_SHARES),
            ColKind::Text | ColKind::Date | ColKind::Url => None,
        }
    }
    /// Kinds excluded from summary statistics (label / non-numeric).
    fn is_qualitative(self) -> bool {
        matches!(self, ColKind::Text | ColKind::Url | ColKind::Date)
    }
}

/// One column in an ad-hoc table (`ColumnSpec`).
#[derive(Clone, Debug)]
pub struct ColumnSpec {
    pub key: String,
    pub header: String,
    pub kind: ColKind,
    pub width: u32,
    pub units: String,
    pub group: String,
    pub definition: String,
    pub is_label: bool,
}

impl ColumnSpec {
    pub fn label(key: &str, header: &str) -> Self {
        ColumnSpec {
            key: key.into(),
            header: header.into(),
            kind: ColKind::Text,
            width: 10,
            units: String::new(),
            group: String::new(),
            definition: String::new(),
            is_label: true,
        }
    }
    pub fn metric(key: &str, header: &str, kind: ColKind) -> Self {
        ColumnSpec {
            key: key.into(),
            header: header.into(),
            kind,
            width: 13,
            units: String::new(),
            group: String::new(),
            definition: String::new(),
            is_label: false,
        }
    }
    pub fn with_group(mut self, group: &str) -> Self {
        self.group = group.into();
        self
    }
    pub fn with_units(mut self, units: &str) -> Self {
        self.units = units.into();
        self
    }
    pub fn with_definition(mut self, def: &str) -> Self {
        self.definition = def.into();
        self
    }
}

/// Row-grain (Q1 of the decision tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
    Company,
    DataPoint,
    Period,
    Event,
    Mixed,
}

impl Grain {
    fn as_str(self) -> &'static str {
        match self {
            Grain::Company => "company",
            Grain::DataPoint => "data_point",
            Grain::Period => "period",
            Grain::Event => "event",
            Grain::Mixed => "mixed",
        }
    }
    fn base_layout(self) -> Layout {
        match self {
            Grain::Company => Layout::Wide,
            Grain::DataPoint => Layout::Long,
            Grain::Period => Layout::TimeSeries,
            Grain::Event => Layout::EventLog,
            Grain::Mixed => Layout::Dashboard,
        }
    }
}

/// Chosen table layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Wide,
    Long,
    TimeSeries,
    EventLog,
    Dashboard,
}

impl Layout {
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Wide => "wide",
            Layout::Long => "long",
            Layout::TimeSeries => "time_series",
            Layout::EventLog => "event_log",
            Layout::Dashboard => "dashboard",
        }
    }
    /// Worksheet name for this layout (`write_research` sheet_name map).
    fn sheet_name(self) -> &'static str {
        match self {
            Layout::Wide => "Comparison",
            Layout::Long => "Findings",
            Layout::TimeSeries => "Time Series",
            Layout::EventLog => "Events",
            // DASHBOARD falls back before reaching here.
            Layout::Dashboard => "Comparison",
        }
    }
    fn default_units(self) -> &'static str {
        match self {
            Layout::Wide => "(comparable peers - units per column header)",
            Layout::TimeSeries => "(per-period values - units per column header)",
            Layout::EventLog => "(events sorted by date - most recent first)",
            _ => "(research findings - one row per data point)",
        }
    }
}

/// Output of [`pick_adhoc_layout`] — the SPEC Section-1 answers.
#[derive(Clone, Debug)]
pub struct LayoutDecision {
    pub layout: Layout,
    pub multi_tab: bool,
    pub freeze_first_col: bool,
    pub use_autofilter: bool,
    pub summary_stats: bool,
    pub section_dividers: bool,
    pub qualitative_handling: String,
    pub rationale: Vec<String>,
}

/// Port of `pick_adhoc_layout` — the SPEC Section-1 decision tree, including the
/// verbatim rationale strings (gated: they render into the decision footer).
pub fn pick_adhoc_layout(
    grain: Grain,
    n_metrics: usize,
    n_entities: usize,
    qualitative_max_chars: usize,
    needs_sort_filter: bool,
    is_comparative: bool,
) -> LayoutDecision {
    let mut rationale = Vec::new();
    let mut layout = grain.base_layout();
    rationale.push(format!(
        "Q1 grain={} -> {}",
        grain.as_str(),
        layout.as_str()
    ));

    if n_metrics >= 20 && layout != Layout::Long {
        layout = Layout::Dashboard;
        rationale.push(format!(
            "Q2 metrics={n_metrics} >=20 -> escalate to multi-tab"
        ));
    }
    let freeze_first_col = (9..=20).contains(&n_metrics);
    if freeze_first_col {
        rationale.push(format!(
            "Q2 metrics={n_metrics} in [9,20] -> freeze first col"
        ));
    }

    let qual = if qualitative_max_chars == 0 {
        "none"
    } else if qualitative_max_chars < 50 {
        "in_cell"
    } else if qualitative_max_chars < 200 {
        "in_comment"
    } else {
        "separate_column"
    };
    rationale.push(format!("Q3 max_text={qualitative_max_chars} -> {qual}"));

    let section_dividers = n_entities > 15 && n_entities <= 50;
    if n_entities > 50 && layout == Layout::Wide {
        layout = Layout::Dashboard;
        rationale.push(format!(
            "Q4 entities={n_entities} >50 -> split to multi-tab"
        ));
    }

    let use_autofilter = needs_sort_filter;
    if use_autofilter {
        rationale.push("Q5 -> AutoFilter on".to_string());
    }

    let summary_stats = is_comparative && matches!(layout, Layout::Wide | Layout::TimeSeries);
    if summary_stats {
        rationale.push("S7 comparative -> summary stats row".to_string());
    }

    LayoutDecision {
        multi_tab: layout == Layout::Dashboard,
        layout,
        freeze_first_col,
        use_autofilter,
        summary_stats,
        section_dividers,
        qualitative_handling: qual.to_string(),
        rationale,
    }
}

/// Compute the value of an Excel summary function over the present values, for
/// caching into the formula cell (empty → `None`). `MEDIAN`/`AVERAGE`/`MIN`/`MAX`
/// mirror Excel's semantics (blanks already excluded by the caller).
fn stat_value(func: &str, vals: &[f64]) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    match func {
        "AVERAGE" => Some(vals.iter().sum::<f64>() / vals.len() as f64),
        "MIN" => vals
            .iter()
            .cloned()
            .fold(None, |a, x| Some(a.map_or(x, |m: f64| m.min(x)))),
        "MAX" => vals
            .iter()
            .cloned()
            .fold(None, |a, x| Some(a.map_or(x, |m: f64| m.max(x)))),
        "MEDIAN" => {
            let mut v = vals.to_vec();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = v.len();
            Some(if n % 2 == 0 {
                (v[n / 2 - 1] + v[n / 2]) / 2.0
            } else {
                v[n / 2]
            })
        }
        _ => None,
    }
}

/// A full ad-hoc research table (the writer input).
#[derive(Clone, Debug)]
pub struct AdHocTable {
    pub title: String,
    /// Units line under the title; empty → layout default.
    pub units: String,
    pub columns: Vec<ColumnSpec>,
    /// One map per entity/data-point/period, keyed by `ColumnSpec.key`.
    pub rows: Vec<HashMap<String, CellVal>>,
    /// `(label_value, column_key)` → citation, rendered as a cell note.
    pub sources: HashMap<(String, String), String>,
    pub grain: Grain,
    pub is_comparative: bool,
    pub needs_sort_filter: bool,
    /// Force a layout, bypassing the decision tree (rare).
    pub layout_override: Option<Layout>,
}

impl AdHocTable {
    /// Validate the table (exactly one label column, non-empty rows).
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.rows.is_empty() {
            return Err("rows must be non-empty".into());
        }
        let n_label = self.columns.iter().filter(|c| c.is_label).count();
        if n_label != 1 {
            return Err(format!(
                "exactly one label column required, found {n_label}"
            ));
        }
        Ok(())
    }

    fn n_metrics(&self) -> usize {
        self.columns.iter().filter(|c| !c.is_label).count()
    }

    /// Longest text-kind value across all rows (`write_research` max_text).
    fn qualitative_max_chars(&self) -> usize {
        let mut m = 0;
        for r in &self.rows {
            for c in &self.columns {
                if c.kind == ColKind::Text {
                    if let Some(CellVal::Text(t)) = r.get(&c.key) {
                        m = m.max(t.chars().count());
                    }
                }
            }
        }
        m
    }

    /// Serialize the table's entity rows to CSV (header = column headers, one
    /// line per data row; summary-stat / footer rows excluded). Monetary /
    /// ratio values are emitted verbatim as computed, for drop-in use in a
    /// banker's own model. RFC-4180 quoting for fields with `,"`/newlines.
    pub fn to_csv(&self) -> String {
        fn esc(s: &str) -> String {
            if s.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        }
        let mut out = String::new();
        let header: Vec<String> = self.columns.iter().map(|c| esc(&c.header)).collect();
        out.push_str(&header.join(","));
        out.push('\n');
        for r in &self.rows {
            let line: Vec<String> = self
                .columns
                .iter()
                .map(|c| match r.get(&c.key) {
                    Some(CellVal::Number(n)) => format!("{n}"),
                    Some(CellVal::Text(t)) => esc(t),
                    _ => String::new(),
                })
                .collect();
            out.push_str(&line.join(","));
            out.push('\n');
        }
        out
    }

    /// Run the decision tree for this table (with DASHBOARD fallback applied,
    /// mirroring `write_research`).
    pub fn decision(&self) -> LayoutDecision {
        let mut d = pick_adhoc_layout(
            self.grain,
            self.n_metrics(),
            self.rows.len(),
            self.qualitative_max_chars(),
            self.needs_sort_filter,
            self.is_comparative,
        );
        if let Some(l) = self.layout_override {
            d.layout = l;
            d.rationale.push(format!("override -> {}", l.as_str()));
        }
        // Multi-tab DASHBOARD not implemented — fall back to single tab.
        if d.layout == Layout::Dashboard {
            d.layout = if self.grain == Grain::DataPoint {
                Layout::Long
            } else {
                Layout::Wide
            };
            d.rationale
                .push("DASHBOARD not yet implemented - falling back to single-tab".to_string());
        }
        d
    }

    /// Build the [`Sheet`] cell-model. `generated` is the footer stamp
    /// (`"Generated: … | Source: …"`); the gate pins it for determinism.
    pub fn build_sheet(&self, generated: &str) -> Sheet {
        let decision = self.decision();
        let mut s = Sheet::new(decision.layout.sheet_name());

        // ── Title + units (setup_table) ──────────────────────────────────
        s.merge(
            2,
            LABEL,
            Cell {
                value: Some(Value::Text(self.title.clone())),
                fill: Some(ADHOC_TITLE.to_string()),
                ..Default::default()
            },
        );
        let units = if self.units.is_empty() {
            decision.layout.default_units().to_string()
        } else {
            self.units.clone()
        };
        s.text(5, LABEL, units);

        let mut row = 7u32; // content start

        // ── Group banner (only if any column carries a group) ─────────────
        if self.columns.iter().any(|c| !c.group.is_empty()) {
            let n = self.columns.len();
            let mut i = 0usize;
            while i < n {
                let grp = &self.columns[i].group;
                let mut j = i;
                while j < n && &self.columns[j].group == grp {
                    j += 1;
                }
                if !grp.is_empty() {
                    // Merged (span>1) or single: only the top-left cell carries
                    // value + fill (openpyxl reports merged fill on top-left).
                    s.merge(
                        row,
                        LABEL + i as u32,
                        Cell {
                            value: Some(Value::Text(grp.to_uppercase())),
                            fill: Some(TAN.to_string()),
                            ..Default::default()
                        },
                    );
                }
                i = j;
            }
            row += 1;
        }

        // ── Header row (INK band across every column) ─────────────────────
        for (i, c) in self.columns.iter().enumerate() {
            let mut note_parts: Vec<String> = Vec::new();
            if !c.definition.is_empty() {
                note_parts.push(c.definition.clone());
            }
            if !c.units.is_empty() {
                note_parts.push(format!("Units: {}", c.units));
            }
            s.merge(
                row,
                LABEL + i as u32,
                Cell {
                    value: Some(Value::Text(c.header.clone())),
                    fill: Some(ADHOC_HEADER.to_string()),
                    comment: (!note_parts.is_empty()).then(|| note_parts.join("\n")),
                    ..Default::default()
                },
            );
        }
        row += 1;
        let data_start = row;

        // Label column index (validated to exist exactly once).
        let label_offset = self.columns.iter().position(|c| c.is_label).unwrap_or(0);
        let label_key = &self.columns[label_offset].key;

        // ── Data rows ─────────────────────────────────────────────────────
        for r in &self.rows {
            let label_value = match r.get(label_key) {
                Some(CellVal::Text(t)) => t.clone(),
                Some(CellVal::Number(n)) => format!("{n}"),
                _ => String::new(),
            };
            for (i, c) in self.columns.iter().enumerate() {
                let col = LABEL + i as u32;
                let v = r.get(&c.key).cloned().unwrap_or(CellVal::Empty);
                self.write_value_cell(&mut s, row, col, &v, c, &label_value);
            }
            row += 1;
        }
        let data_end = row - 1;

        // ── Summary stats (comparative WIDE / TIME_SERIES) ────────────────
        if decision.summary_stats && data_end >= data_start {
            row += 1; // blank separator
            for (label, func) in [
                ("Median", "MEDIAN"),
                ("Mean", "AVERAGE"),
                ("Min", "MIN"),
                ("Max", "MAX"),
            ] {
                let stat_row = row;
                s.merge(
                    stat_row,
                    LABEL + label_offset as u32,
                    Cell {
                        value: Some(Value::Text(label.to_string())),
                        fill: Some(ADHOC_SUMMARY.to_string()),
                        ..Default::default()
                    },
                );
                for (i, c) in self.columns.iter().enumerate() {
                    if c.is_label || c.kind.is_qualitative() {
                        continue;
                    }
                    let col = LABEL + i as u32;
                    let rng = format!("{}:{}", cell_ref(data_start, col), cell_ref(data_end, col));
                    // Cache the computed statistic so offline viewers
                    // (LibreOffice pre-recalc) show a number, not 0.
                    let vals: Vec<f64> = self
                        .rows
                        .iter()
                        .filter_map(|r| match r.get(&c.key) {
                            Some(CellVal::Number(n)) => Some(*n),
                            _ => None,
                        })
                        .collect();
                    s.merge(
                        stat_row,
                        col,
                        Cell {
                            formula: Some(format!("={func}({rng})")),
                            cached: stat_value(func, &vals),
                            fill: Some(ADHOC_SUMMARY.to_string()),
                            num_fmt: c.kind.num_fmt(),
                            ..Default::default()
                        },
                    );
                }
                row += 1;
            }
        }

        // ── Decision footer + generated stamp ─────────────────────────────
        row += 2;
        s.text(row, LABEL, format!("Layout: {}", decision.layout.as_str()));
        row += 1;
        for line in &decision.rationale {
            s.text(row, LABEL, format!("  - {line}"));
            row += 1;
        }
        row += 1; // _footer spacer
        s.text(row, LABEL, generated.to_string());

        s
    }

    fn write_value_cell(
        &self,
        s: &mut Sheet,
        row: u32,
        col: u32,
        v: &CellVal,
        spec: &ColumnSpec,
        label_value: &str,
    ) {
        // Blank cells contribute nothing (write_blank → no gated content).
        let is_blank = matches!(v, CellVal::Empty) || matches!(v, CellVal::Text(t) if t.is_empty());
        if is_blank {
            return;
        }

        let src = self
            .sources
            .get(&(label_value.to_string(), spec.key.clone()));

        if spec.is_label {
            if let CellVal::Text(t) = v {
                s.text(row, col, t.clone());
            } else if let CellVal::Number(n) = v {
                s.number(row, col, *n);
            }
            return;
        }

        match spec.kind {
            ColKind::Url => {
                // write_url renders "link" as the visible value.
                s.text(row, col, "link");
            }
            ColKind::Text | ColKind::Date => {
                if let CellVal::Text(t) = v {
                    s.text(row, col, t.clone());
                } else if let CellVal::Number(n) = v {
                    s.number(row, col, *n);
                }
            }
            _ => {
                if let CellVal::Number(n) = v {
                    s.number(row, col, *n);
                    if let Some(cell) = s.cells.get_mut(&(row, col)) {
                        cell.num_fmt = spec.kind.num_fmt();
                    }
                } else if let CellVal::Text(t) = v {
                    s.text(row, col, t.clone());
                }
            }
        }

        // Provenance citation as a note (not gated).
        if let Some(cite) = src {
            if let Some(cell) = s.cells.get_mut(&(row, col)) {
                cell.comment = Some(format!("Source: {cite}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_maps_to_base_layout() {
        let d = pick_adhoc_layout(Grain::Company, 3, 5, 0, false, false);
        assert_eq!(d.layout, Layout::Wide);
        assert_eq!(d.layout.sheet_name(), "Comparison");
        assert_eq!(
            pick_adhoc_layout(Grain::DataPoint, 3, 5, 0, false, false).layout,
            Layout::Long
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Period, 3, 5, 0, false, false).layout,
            Layout::TimeSeries
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Event, 3, 5, 0, false, false).layout,
            Layout::EventLog
        );
    }

    #[test]
    fn freeze_first_col_only_between_9_and_20_metrics() {
        assert!(!pick_adhoc_layout(Grain::Company, 8, 5, 0, false, false).freeze_first_col);
        assert!(pick_adhoc_layout(Grain::Company, 9, 5, 0, false, false).freeze_first_col);
        assert!(pick_adhoc_layout(Grain::Company, 20, 5, 0, false, false).freeze_first_col);
        // 21 metrics escalates to dashboard AND is > 20 so no freeze.
        assert!(!pick_adhoc_layout(Grain::Company, 21, 5, 0, false, false).freeze_first_col);
    }

    #[test]
    fn many_metrics_escalate_to_dashboard_but_long_stays() {
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 19, 5, 0, false, false).layout,
            Layout::Wide
        );
        // n_metrics >= 20 escalates WIDE → DASHBOARD.
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 20, 5, 0, false, false).layout,
            Layout::Dashboard
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 25, 5, 0, false, false).layout,
            Layout::Dashboard
        );
        // LONG never escalates on metric count.
        assert_eq!(
            pick_adhoc_layout(Grain::DataPoint, 25, 5, 0, false, false).layout,
            Layout::Long
        );
    }

    #[test]
    fn many_entities_split_wide_to_dashboard() {
        assert!(pick_adhoc_layout(Grain::Company, 3, 30, 0, false, false).section_dividers);
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 3, 60, 0, false, false).layout,
            Layout::Dashboard
        );
    }

    #[test]
    fn qualitative_thresholds() {
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 3, 5, 0, false, false).qualitative_handling,
            "none"
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 3, 5, 49, false, false).qualitative_handling,
            "in_cell"
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 3, 5, 100, false, false).qualitative_handling,
            "in_comment"
        );
        assert_eq!(
            pick_adhoc_layout(Grain::Company, 3, 5, 250, false, false).qualitative_handling,
            "separate_column"
        );
    }

    #[test]
    fn summary_stats_gated_on_comparative_and_layout() {
        assert!(pick_adhoc_layout(Grain::Company, 3, 5, 0, false, true).summary_stats);
        assert!(pick_adhoc_layout(Grain::Period, 3, 5, 0, false, true).summary_stats);
        assert!(!pick_adhoc_layout(Grain::Company, 3, 5, 0, false, false).summary_stats);
        // LONG grain is comparative but not eligible for the stats block.
        assert!(!pick_adhoc_layout(Grain::DataPoint, 3, 5, 0, false, true).summary_stats);
    }

    fn wide_row(ticker: &str, a: f64, b: f64) -> HashMap<String, CellVal> {
        let mut m = HashMap::new();
        m.insert("t".to_string(), CellVal::Text(ticker.into()));
        m.insert("a".to_string(), CellVal::Number(a));
        m.insert("b".to_string(), CellVal::Number(b));
        m
    }

    #[test]
    fn build_sheet_emits_summary_formulas_over_data_range() {
        let table = AdHocTable {
            title: "T".into(),
            units: String::new(),
            columns: vec![
                ColumnSpec::label("t", "Name"),
                ColumnSpec::metric("a", "A", ColKind::Dollar).with_group("Scale"),
                ColumnSpec::metric("b", "B", ColKind::Percent).with_group("Ratios"),
            ],
            rows: vec![wide_row("X", 1.0, 0.1), wide_row("Y", 2.0, 0.2)],
            sources: HashMap::new(),
            grain: Grain::Company,
            is_comparative: true,
            needs_sort_filter: false,
            layout_override: None,
        };
        table.validate().unwrap();
        let sheet = table.build_sheet("Generated: X | Source: Y");
        // Data rows land at Excel rows 10-11 (0-based 9-10): banner row 8, header row 9.
        let median_cell = sheet
            .cells
            .values()
            .find(|c| c.formula.as_deref() == Some("=MEDIAN(D10:D11)"))
            .expect("MEDIAN(D10:D11) present");
        assert_eq!(median_cell.cached, Some(1.5)); // median(1.0, 2.0)
        // Cached results for the full stat block (offline-viewer correctness).
        let cached_of = |f: &str| {
            sheet
                .cells
                .values()
                .find(|c| c.formula.as_deref() == Some(f))
                .and_then(|c| c.cached)
        };
        assert_eq!(cached_of("=AVERAGE(D10:D11)"), Some(1.5));
        assert_eq!(cached_of("=MIN(D10:D11)"), Some(1.0));
        assert_eq!(cached_of("=MAX(D10:D11)"), Some(2.0));
        // Label column never gets a stat formula.
        assert!(!sheet.cells.values().any(|c| {
            c.formula
                .as_deref()
                .map(|f| f.contains("C10:C11"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn validate_requires_exactly_one_label() {
        let mut t = AdHocTable {
            title: "T".into(),
            units: String::new(),
            columns: vec![ColumnSpec::metric("a", "A", ColKind::Dollar)],
            rows: vec![wide_row("X", 1.0, 0.1)],
            sources: HashMap::new(),
            grain: Grain::Company,
            is_comparative: false,
            needs_sort_filter: false,
            layout_override: None,
        };
        assert!(t.validate().is_err()); // zero labels
        t.columns.insert(0, ColumnSpec::label("t", "Name"));
        assert!(t.validate().is_ok());
        t.rows.clear();
        assert!(t.validate().is_err()); // empty rows
    }

    #[test]
    fn to_csv_emits_header_and_entity_rows() {
        let table = AdHocTable {
            title: "T".into(),
            units: String::new(),
            columns: vec![
                ColumnSpec::label("t", "Name"),
                ColumnSpec::metric("a", "A, x", ColKind::Dollar), // header needs quoting
                ColumnSpec::metric("b", "B", ColKind::Percent),
            ],
            rows: vec![wide_row("X", 1.0, 0.1), wide_row("Y", 2.0, 0.2)],
            sources: HashMap::new(),
            grain: Grain::Company,
            is_comparative: true,
            needs_sort_filter: false,
            layout_override: None,
        };
        let csv = table.to_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Name,\"A, x\",B"); // comma in header quoted
        assert_eq!(lines[1], "X,1,0.1");
        assert_eq!(lines[2], "Y,2,0.2");
        assert_eq!(lines.len(), 3); // header + 2 entities, no stat/footer rows
    }
}
