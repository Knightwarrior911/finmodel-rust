//! 6.4 — DrawingML deck emission (`PptxDeckWriter` + `BrandProfile`) plus the
//! `parse_deck_markdown` multi-document YAML parser (6.3).
//!
//! Reproduces the shape trees the Python `pptx_writer.PPTXDeckWriter` /
//! `pptx_output.ResearchPPTXWriter` emit for the archetypes the finmodel
//! outputs consume. Geometry is emitted in EMU exactly as python-pptx's
//! `Inches()` (`int(x*914400)`) and font sizes as centipoints (`pt*100`), so
//! the 6.4 gate — inspector shape-tree diff (geometry, text runs, fills) —
//! matches. Names/ids are cosmetic and excluded from that gate.

use serde::Deserialize;
use serde_json::Value;

use crate::writer::{fmt_grouped, normalize_heading};

/// `parse_deck_markdown(text)` — multi-document YAML stream -> slide specs.
pub fn parse_deck_markdown(text: &str) -> Result<Vec<Value>, String> {
    let mut specs = Vec::new();
    for de in serde_yaml::Deserializer::from_str(text) {
        let yv = serde_yaml::Value::deserialize(de).map_err(|e| format!("yaml parse: {e}"))?;
        if yv.is_null() {
            continue;
        }
        if !yv.is_mapping() {
            return Err("each YAML doc must be a mapping".to_string());
        }
        let jv: Value = serde_json::to_value(&yv).map_err(|e| format!("yaml->json: {e}"))?;
        if jv.get("type").is_none() {
            return Err(format!("slide spec missing 'type': {jv}"));
        }
        specs.push(jv);
    }
    Ok(specs)
}

// ── palette / typography (VERBATIM from pptx_writer.py) ────────────────────────

pub const BRAND_BLUE: &str = "255BE3";
pub const INK: &str = "0F1632";
pub const WHITE: &str = "FFFFFF";
pub const LIGHT_GRAY: &str = "E6EBED";
pub const MID_GRAY: &str = "D3DADD";
pub const BORDER_GRAY: &str = "A4ACAF";
pub const SAND: &str = "EAE0D3";
pub const ACCENT_RED: &str = "FF3C28";
pub const FOOTNOTE_GRAY: &str = "808080";
pub const FOREST: &str = "388A42";

/// Chart series palette (Section 4.2), priority order.
pub const SERIES_PALETTE: [&str; 10] = [
    "255BE3", "0F1632", "73C2FC", "A4ACAF", "388A42", "80CE84", "FAB728", "FFA15A", "8E319C",
    "D71671",
];

pub const FONT_HEADLINE: &str = "Arial";
pub const FONT_BODY: &str = "Arial";
const MARGIN_IN: f64 = 0.5;
const PT_FOOTNOTE: i64 = 8;
const PT_PAGE: i64 = 8;

/// `ASPECT_DIMS` — inches (width, height) per aspect ratio.
pub fn aspect_dims(aspect: &str) -> (f64, f64) {
    match aspect {
        "4:3" => (10.0, 7.5),
        "A4_LANDSCAPE" => (10.83, 7.5),
        _ => (13.333, 7.5),
    }
}

/// Firm brand overrides (colors are `#RRGGBB` or `RRGGBB`).
#[derive(Debug, Clone)]
pub struct BrandProfile {
    pub brand_primary: String,
    pub brand_dark: String,
    pub accent_cyan: String,
    pub accent_red: String,
    pub font_headline: String,
    pub font_body: String,
    pub headline_size: i64,
    pub body_size: i64,
    pub footnote_size: i64,
    pub headline_bold: bool,
    pub aspect_ratio: String,
}

impl Default for BrandProfile {
    fn default() -> Self {
        BrandProfile {
            brand_primary: "#255BE3".into(),
            brand_dark: "#0F1632".into(),
            accent_cyan: "#73C2FC".into(),
            accent_red: "#FF3C28".into(),
            font_headline: "Arial".into(),
            font_body: "Arial".into(),
            headline_size: 22,
            body_size: 11,
            footnote_size: 8,
            headline_bold: true,
            aspect_ratio: "16:9".into(),
        }
    }
}

// ── EMU / formatting helpers ──────────────────────────────────────────────────

fn emu(inches: f64) -> i64 {
    (inches * 914400.0) as i64
}

fn hex6(c: &str) -> String {
    c.trim_start_matches('#').to_uppercase()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Python `value_format.format(v)` for the format strings the deck uses.
fn py_value_format(fmt: &str, v: f64) -> String {
    if fmt.contains(":+,.0f") {
        fmt_grouped(v, 0, true)
    } else if fmt.contains(":,.0f") {
        fmt_grouped(v, 0, false)
    } else if fmt.contains(":,.1f") {
        fmt_grouped(v, 1, false)
    } else {
        fmt_grouped(v, 0, false)
    }
}

/// `pptx_output._fmt_v(v, currency)`.
fn fmt_v(v: Option<f64>, currency: &str) -> String {
    match v {
        None => "—".to_string(),
        Some(v) => {
            if v.abs() >= 1e9 {
                format!("{currency} {:.1}B", v / 1e9)
            } else if v.abs() >= 1e6 {
                format!("{currency} {}M", fmt_grouped(v / 1e6, 0, false))
            } else {
                format!("{currency} {}", fmt_grouped(v, 0, false))
            }
        }
    }
}

// ── deck writer ───────────────────────────────────────────────────────────────

/// A scorecard tile.
#[derive(Debug, Clone, Default)]
pub struct ScorecardTile {
    pub metric: String,
    pub value: String,
    pub rating: i64,
    pub sub: String,
}

/// Faithful port of `pptx_writer.PPTXDeckWriter` (archetype subset).
pub struct PptxDeckWriter {
    brand_primary: String,
    brand_dark: String,
    #[allow(dead_code)]
    accent_cyan: String,
    accent_red: String,
    font_headline: String,
    font_body: String,
    headline_size: i64,
    #[allow(dead_code)]
    body_size: i64,
    #[allow(dead_code)]
    footnote_size: i64,
    headline_bold: bool,
    confidentiality: String,
    slide_w_in: f64,
    slide_h_in: f64,
    page: i64,
    /// Cover date text (pinned for determinism; matches `date.today()` path).
    deck_date: String,
    slides: Vec<Vec<String>>,
    next_id: i64,
}

impl PptxDeckWriter {
    /// Match `PPTXDeckWriter(firm, project, confidentiality="CONFIDENTIAL")`.
    pub fn new(brand: &BrandProfile, confidentiality: &str, deck_date: &str) -> Self {
        let (w, h) = aspect_dims(&brand.aspect_ratio);
        PptxDeckWriter {
            brand_primary: hex6(&brand.brand_primary),
            brand_dark: hex6(&brand.brand_dark),
            accent_cyan: hex6(&brand.accent_cyan),
            accent_red: hex6(&brand.accent_red),
            font_headline: brand.font_headline.clone(),
            font_body: brand.font_body.clone(),
            headline_size: brand.headline_size,
            body_size: brand.body_size,
            footnote_size: brand.footnote_size,
            headline_bold: brand.headline_bold,
            confidentiality: confidentiality.to_string(),
            slide_w_in: w,
            slide_h_in: h,
            page: 0,
            deck_date: deck_date.to_string(),
            slides: Vec::new(),
            next_id: 2,
        }
    }

    fn blank_slide(&mut self) {
        self.slides.push(Vec::new());
        self.next_id = 2;
    }

    fn cur(&mut self) -> &mut Vec<String> {
        self.slides.last_mut().expect("no current slide")
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ── primitives ────────────────────────────────────────────────────────────

    /// `_add_rect` / `_add_rounded_rect` — a rounded rectangle (adj 0.04).
    fn add_rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: &str,
        line: Option<&str>,
        no_line: bool,
    ) {
        let id = self.take_id();
        let ln = if no_line {
            "<a:ln><a:noFill/></a:ln>".to_string()
        } else if let Some(c) = line {
            format!(
                "<a:ln><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln>",
                hex6(c)
            )
        } else {
            "<a:ln><a:noFill/></a:ln>".to_string()
        };
        let sp = format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Rounded Rectangle {}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>\
<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val 4000\"/></a:avLst></a:prstGeom>\
<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>{ln}</p:spPr>\
{STYLE_AUTOSHAPE}\
<p:txBody><a:bodyPr rtlCol=\"0\" anchor=\"ctr\"/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/></a:p></p:txBody></p:sp>",
            id - 1,
            emu(x),
            emu(y),
            emu(w),
            emu(h),
            hex6(fill),
        );
        self.cur().push(sp);
    }

    /// `_add_oval`.
    fn add_oval(&mut self, x: f64, y: f64, w: f64, h: f64, fill: &str) {
        let id = self.take_id();
        let sp = format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Oval {}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>\
<a:prstGeom prst=\"ellipse\"><a:avLst/></a:prstGeom>\
<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr>\
{STYLE_AUTOSHAPE}\
<p:txBody><a:bodyPr rtlCol=\"0\" anchor=\"ctr\"/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/></a:p></p:txBody></p:sp>",
            id - 1,
            emu(x),
            emu(y),
            emu(w),
            emu(h),
            hex6(fill),
        );
        self.cur().push(sp);
    }

    /// `_add_text` — fixed-size textbox (multi-line supported).
    #[allow(clippy::too_many_arguments)]
    fn add_text(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        text: &str,
        font_size: i64,
        bold: bool,
        italic: bool,
        color: Option<&str>,
        align: &str,
        font: Option<&str>,
        anchor: Option<&str>,
    ) {
        let id = self.take_id();
        let color = hex6(color.unwrap_or(&self.brand_dark));
        let font = font
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.font_body.clone());
        let b = if bold { 1 } else { 0 };
        let i = if italic { 1 } else { 0 };
        let mut paras = String::new();
        for line in text.split('\n') {
            paras.push_str(&format!(
                "<a:p><a:pPr algn=\"{align}\"/><a:r><a:rPr sz=\"{}\" b=\"{b}\" i=\"{i}\">\
<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill><a:latin typeface=\"{font}\"/></a:rPr>\
<a:t>{}</a:t></a:r></a:p>",
                font_size * 100,
                esc(line),
            ));
        }
        let anchor_attr = anchor
            .map(|a| format!(" anchor=\"{a}\""))
            .unwrap_or_default();
        let sp = format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"TextBox {}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>\
<p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" rIns=\"0\" tIns=\"0\" bIns=\"0\"{anchor_attr}><a:spAutoFit/></a:bodyPr>\
<a:lstStyle/>{paras}</p:txBody></p:sp>",
            id - 1,
            emu(x),
            emu(y),
            emu(w),
            emu(h),
        );
        self.cur().push(sp);
    }

    /// `_add_line` — straight connector.
    fn add_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: &str, weight: f64) {
        let id = self.take_id();
        let (ex1, ey1, ex2, ey2) = (emu(x1), emu(y1), emu(x2), emu(y2));
        let flip_h = ex1 > ex2;
        let flip_v = ey1 > ey2;
        let x = ex1.min(ex2);
        let y = ey1.min(ey2);
        let cx = (ex2 - ex1).abs();
        let cy = (ey2 - ey1).abs();
        let flip = format!(
            "{}{}",
            if flip_h { " flipH=\"1\"" } else { "" },
            if flip_v { " flipV=\"1\"" } else { "" }
        );
        let w = (weight * 12700.0) as i64;
        let sp = format!(
            "<p:cxnSp><p:nvCxnSpPr><p:cNvPr id=\"{id}\" name=\"Connector {}\"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr>\
<p:spPr><a:xfrm{flip}><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom>\
<a:ln w=\"{w}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln></p:spPr>\
{STYLE_CONNECTOR}</p:cxnSp>",
            id - 1,
            hex6(color),
        );
        self.cur().push(sp);
    }

    // ── content-slide scaffolding ───────────────────────────────────────────────

    fn validate_action_title(&self, t: &str) -> Result<String, String> {
        if t.trim().is_empty() {
            return Err("action title required (engineering R1)".into());
        }
        if t.split_whitespace().count() < 3 {
            return Err(format!(
                "action title '{t}' too short - use 6-14 words conveying the takeaway"
            ));
        }
        Ok(normalize_heading(t))
    }

    /// `_content_slide_with_title` (no-logo path).
    fn content_slide_with_title(&mut self, action_title: &str) {
        self.blank_slide();
        let headline_w = self.slide_w_in - 0.6;
        let wc = action_title.split_whitespace().count() as i64;
        let mut size = self.headline_size;
        if wc > 8 {
            size = (self.headline_size - ((wc - 8) / 2)).max(16);
        }
        let (bp, fh, bold) = (
            self.brand_primary.clone(),
            self.font_headline.clone(),
            self.headline_bold,
        );
        self.add_text(
            0.3,
            0.18,
            headline_w,
            0.45,
            action_title,
            size,
            bold,
            false,
            Some(&bp),
            "l",
            Some(&fh),
            None,
        );
        let sw = self.slide_w_in;
        self.add_line(0.3, 0.72, sw - 0.3, 0.72, MID_GRAY, 0.5);
    }

    fn add_footer(&mut self) {
        let (sw, sh, page) = (self.slide_w_in, self.slide_h_in, self.page);
        self.add_text(
            sw - 1.0,
            sh - 0.3,
            0.6,
            0.25,
            &page.to_string(),
            PT_PAGE,
            false,
            false,
            Some(FOOTNOTE_GRAY),
            "r",
            None,
            None,
        );
        if !self.confidentiality.is_empty() {
            let conf = self.confidentiality.clone();
            self.add_text(
                MARGIN_IN,
                sh - 0.3,
                3.0,
                0.25,
                &conf,
                PT_PAGE,
                true,
                false,
                Some(FOOTNOTE_GRAY),
                "l",
                None,
                None,
            );
        }
    }

    fn add_source_line(&mut self, source: &str, notes: &str) {
        let y = self.slide_h_in - 0.45;
        let mut parts: Vec<String> = Vec::new();
        if !source.is_empty() {
            parts.push(format!("Source: {source}"));
        } else if notes.is_empty() {
            parts.push("Source: [TBD]".to_string());
        }
        if !notes.is_empty() {
            parts.push(format!("Note: {notes}"));
        }
        let text = parts.join("   ");
        let sw = self.slide_w_in;
        self.add_text(
            0.3,
            y,
            sw - 0.6,
            0.25,
            &text,
            PT_FOOTNOTE,
            false,
            true,
            Some(FOOTNOTE_GRAY),
            "l",
            None,
            None,
        );
    }

    // ── public archetypes ───────────────────────────────────────────────────────

    /// `add_cover`.
    pub fn add_cover(&mut self, title: &str, subtitle: &str) {
        self.blank_slide();
        let sh = self.slide_h_in;
        let sw = self.slide_w_in;
        self.add_rect(0.0, 0.0, 0.25, sh, &self.brand_primary.clone(), None, true);
        let bp = self.brand_primary.clone();
        let fh = self.font_headline.clone();
        let bold = self.headline_bold;
        self.add_text(
            MARGIN_IN + 0.3,
            sh * 0.32,
            sw - MARGIN_IN - 0.8,
            2.4,
            title,
            44,
            bold,
            false,
            Some(&bp),
            "l",
            Some(&fh),
            None,
        );
        if !subtitle.is_empty() {
            let bd = self.brand_dark.clone();
            self.add_text(
                MARGIN_IN + 0.3,
                sh * 0.55,
                sw - MARGIN_IN - 0.8,
                0.6,
                subtitle,
                22,
                false,
                false,
                Some(&bd),
                "l",
                Some(&fh),
                None,
            );
        }
        let date_str = self.deck_date.clone();
        self.add_text(
            MARGIN_IN + 0.3,
            sh - 1.1,
            sw - MARGIN_IN - 0.8,
            0.4,
            &date_str,
            14,
            false,
            false,
            Some(FOOTNOTE_GRAY),
            "l",
            None,
            None,
        );
        if !self.confidentiality.is_empty() {
            let conf = self.confidentiality.clone();
            self.add_text(
                sw - 2.5,
                sh - 0.5,
                2.0,
                0.3,
                &conf,
                PT_FOOTNOTE,
                true,
                false,
                Some(FOOTNOTE_GRAY),
                "r",
                None,
                None,
            );
        }
        self.page = 0;
    }

    /// `add_section_divider`.
    pub fn add_section_divider(&mut self, section_num: &str, title: &str) {
        self.content_slide_divider(section_num, title);
    }

    fn content_slide_divider(&mut self, section_num: &str, title: &str) {
        self.blank_slide();
        let (sw, sh) = (self.slide_w_in, self.slide_h_in);
        self.add_rect(0.0, 0.0, 0.25, sh, &self.brand_primary.clone(), None, true);
        let (bp, fh) = (self.brand_primary.clone(), self.font_headline.clone());
        self.add_text(
            MARGIN_IN,
            sh * 0.30,
            sw - 2.0 * MARGIN_IN,
            1.6,
            section_num,
            80,
            true,
            false,
            Some(&bp),
            "l",
            Some(&fh),
            None,
        );
        self.add_line(MARGIN_IN, sh * 0.50, MARGIN_IN + 1.2, sh * 0.50, &bp, 2.5);
        let bd = self.brand_dark.clone();
        self.add_text(
            MARGIN_IN,
            sh * 0.54,
            sw - 2.0 * MARGIN_IN,
            1.0,
            title,
            36,
            true,
            false,
            Some(&bd),
            "l",
            Some(&fh),
            None,
        );
        self.page += 1;
        self.add_footer();
    }

    /// `add_scorecard`.
    pub fn add_scorecard(
        &mut self,
        action_title: &str,
        tiles: &[ScorecardTile],
        source: &str,
        notes: &str,
    ) -> Result<(), String> {
        let at = self.validate_action_title(action_title)?;
        let n = tiles.len();
        if !(1..=9).contains(&n) {
            return Err(format!("scorecard requires 1-9 tiles, got {n}"));
        }
        self.content_slide_with_title(&at);
        let (cols, rows) = if n <= 3 {
            (n, 1)
        } else if n <= 6 {
            (3, 2)
        } else {
            (3, 3)
        };
        let avail_w = self.slide_w_in - 2.0 * MARGIN_IN;
        let avail_h = 4.8;
        let gap = 0.2;
        let tile_w = (avail_w - gap * (cols as f64 - 1.0)) / cols as f64;
        let tile_h = (avail_h - gap * (rows as f64 - 1.0)) / rows as f64;
        let top0 = 1.5;
        let left0 = MARGIN_IN;
        for (i, tile) in tiles.iter().enumerate() {
            let r = i / cols;
            let c = i % cols;
            let left = left0 + c as f64 * (tile_w + gap);
            let top = top0 + r as f64 * (tile_h + gap);
            self.draw_tile(left, top, tile_w, tile_h, tile);
        }
        self.add_source_line(source, notes);
        self.page += 1;
        self.add_footer();
        Ok(())
    }

    /// `_draw_tile`.
    fn draw_tile(&mut self, left: f64, top: f64, w: f64, h: f64, tile: &ScorecardTile) {
        self.add_rect(left, top, w, h, LIGHT_GRAY, Some(BORDER_GRAY), false);
        let bd = self.brand_dark.clone();
        self.add_text(
            left + 0.15,
            top + 0.1,
            w - 0.3,
            0.4,
            &tile.metric,
            11,
            false,
            false,
            Some(&bd),
            "l",
            None,
            None,
        );
        let bp = self.brand_primary.clone();
        self.add_text(
            left + 0.15,
            top + 0.5,
            w - 0.3,
            h - 1.0,
            &tile.value,
            24,
            true,
            false,
            Some(&bp),
            "l",
            None,
            None,
        );
        if tile.rating > 0 {
            let dot_y = top + h - 0.45;
            let r = tile.rating.clamp(0, 5);
            for i in 0..5 {
                let fill = if i < r {
                    self.brand_primary.clone()
                } else {
                    MID_GRAY.to_string()
                };
                self.add_oval(left + 0.15 + i as f64 * 0.22, dot_y, 0.15, 0.15, &fill);
            }
        }
        if !tile.sub.is_empty() {
            self.add_text(
                left + 0.15,
                top + h - 0.25,
                w - 0.3,
                0.2,
                &tile.sub,
                8,
                false,
                true,
                Some(FOOTNOTE_GRAY),
                "l",
                None,
                None,
            );
        }
    }

    /// `add_waterfall` (shape-based; broken-axis supported).
    #[allow(clippy::too_many_arguments)]
    pub fn add_waterfall(
        &mut self,
        action_title: &str,
        segments: &[WaterfallSeg],
        value_format: &str,
        y_label: &str,
        source: &str,
        notes: &str,
        broken_axis: bool,
    ) -> Result<(), String> {
        let broken_axis_threshold = 5.0;
        let at = self.validate_action_title(action_title)?;
        let n = segments.len();
        if !(2..=12).contains(&n) {
            return Err(format!("waterfall requires 2-12 segments, got {n}"));
        }
        self.content_slide_with_title(&at);

        let body_top = 0.95;
        let body_bottom = self.slide_h_in - 0.85;
        let plot_left = 0.9;
        let plot_right = self.slide_w_in - 0.4;
        let plot_w = plot_right - plot_left;
        let plot_top = body_top + 0.4;
        let plot_bot = body_bottom - 0.3;
        let plot_h = plot_bot - plot_top;

        // Running cumulative + bar (lo, hi).
        let mut bars: Vec<(String, f64, f64)> = Vec::new();
        let mut running = 0.0f64;
        for s in segments {
            let v = s.value;
            let (lo, hi) = match s.kind.as_str() {
                "start" | "total" => {
                    running = v;
                    (0.0, v)
                }
                "plus" => {
                    let lohi = (running, running + v);
                    running += v;
                    lohi
                }
                _ => {
                    let lohi = (running + v, running);
                    running += v;
                    lohi
                }
            };
            bars.push((s.kind.clone(), lo, hi));
        }

        let mut floor_v = 0.0;
        let mut do_break = false;
        let bar_max = bars.iter().map(|b| b.2).fold(f64::MIN, f64::max);
        let bar_min_lo = bars.iter().map(|b| b.1).fold(f64::MAX, f64::min);
        let deltas: Vec<f64> = segments
            .iter()
            .filter(|s| s.kind == "plus" || s.kind == "minus")
            .map(|s| s.value.abs())
            .collect();
        let biggest_baseline = segments
            .iter()
            .filter(|s| s.kind == "start" || s.kind == "total")
            .map(|s| s.value.abs())
            .fold(0.0, f64::max);
        let biggest_delta = deltas.iter().cloned().fold(0.0, f64::max);
        if broken_axis
            && !deltas.is_empty()
            && biggest_baseline > 0.0
            && biggest_baseline / biggest_delta.max(1e-9) >= broken_axis_threshold
        {
            do_break = true;
            let min_bottom = bars
                .iter()
                .filter(|b| b.0 == "plus" || b.0 == "minus")
                .map(|b| b.1)
                .fold(f64::MAX, f64::min);
            floor_v = bar_min_lo.max(min_bottom) * 0.92;
            floor_v = floor_v.min(bar_min_lo - biggest_delta * 0.5);
            if floor_v <= 0.0 {
                floor_v = bar_min_lo * 0.85;
            }
        }

        let (y_min, y_max0) = if do_break {
            (floor_v, bar_max)
        } else {
            (0.0f64.min(bar_min_lo), bar_max)
        };
        let mut rng = (y_max0 - y_min).max(f64::MIN_POSITIVE);
        if (y_max0 - y_min) == 0.0 {
            rng = 1.0;
        }
        let y_max = y_max0 + rng * 0.10;
        let rng = y_max - y_min;

        let y_to_in = |v: f64| -> f64 {
            let vc = v.min(y_max).max(y_min);
            plot_bot - (vc - y_min) / rng * plot_h
        };

        let baseline_y = y_to_in(y_min);
        self.add_line(
            plot_left,
            baseline_y,
            plot_right,
            baseline_y,
            BORDER_GRAY,
            0.75,
        );

        let gap = 0.15;
        let bar_w = (plot_w - gap * (n as f64 - 1.0)) / n as f64;

        let mut prev_top_y: Option<f64> = None;
        let mut prev_x_right: Option<f64> = None;
        for (i, ((kind, lo, hi), seg)) in bars.iter().zip(segments.iter()).enumerate() {
            let x = plot_left + i as f64 * (bar_w + gap);
            let disp_lo = if do_break && (kind == "start" || kind == "total") {
                lo.max(y_min)
            } else {
                *lo
            };
            let top_y = y_to_in(disp_lo.max(*hi));
            let bot_y = y_to_in(disp_lo.min(*hi));
            let h = (bot_y - top_y).max(0.04);
            let color = match kind.as_str() {
                "start" | "total" => self.brand_dark.clone(),
                "plus" => FOREST.to_string(),
                _ => self.accent_red.clone(),
            };
            self.add_rect(x, top_y, bar_w, h, &color, None, true);

            if do_break && (kind == "start" || kind == "total") {
                self.draw_break_marker(x, bar_w, plot_bot - 0.05);
            }

            if let (Some(pty), Some(pxr)) = (prev_top_y, prev_x_right) {
                let cy = if kind == "plus" || kind == "minus" {
                    y_to_in(*lo)
                } else {
                    y_to_in(lo.max(*hi))
                };
                self.add_line(pxr, pty, x, cy, BORDER_GRAY, 0.5);
            }

            let label_y = top_y - 0.28;
            let vlabel = py_value_format(value_format, seg.value);
            let fb = self.font_body.clone();
            self.add_text(
                x - 0.1,
                label_y,
                bar_w + 0.2,
                0.22,
                &vlabel,
                9,
                true,
                false,
                Some(&color),
                "ctr",
                Some(&fb),
                None,
            );

            let bd = self.brand_dark.clone();
            self.add_text(
                x - 0.1,
                plot_bot + 0.1,
                bar_w + 0.2,
                0.4,
                &seg.label,
                9,
                false,
                false,
                Some(&bd),
                "ctr",
                Some(&fb),
                None,
            );

            prev_top_y = Some(if kind != "minus" {
                y_to_in(*hi)
            } else {
                y_to_in(*lo)
            });
            prev_x_right = Some(x + bar_w);
        }

        let fb = self.font_body.clone();
        if !y_label.is_empty() {
            let label = if do_break {
                format!("{y_label} (axis truncated)")
            } else {
                y_label.to_string()
            };
            self.add_text(
                0.3,
                body_top + 0.05,
                2.5,
                0.22,
                &label,
                9,
                false,
                true,
                Some(FOOTNOTE_GRAY),
                "l",
                Some(&fb),
                None,
            );
        } else if do_break {
            self.add_text(
                0.3,
                body_top + 0.05,
                2.5,
                0.22,
                "(axis truncated)",
                9,
                false,
                true,
                Some(FOOTNOTE_GRAY),
                "l",
                Some(&fb),
                None,
            );
        }

        self.add_source_line(source, notes);
        self.page += 1;
        self.add_footer();
        Ok(())
    }

    /// `_draw_break_marker`.
    fn draw_break_marker(&mut self, bar_x: f64, bar_w: f64, band_top: f64) {
        let band_h = 0.18;
        self.add_rect(
            bar_x - 0.02,
            band_top,
            bar_w + 0.04,
            band_h,
            WHITE,
            None,
            true,
        );
        let zig_y_top = band_top + 0.02;
        let zig_y_bot = band_top + band_h - 0.02;
        let steps = 6;
        let mut last_x = bar_x - 0.02;
        let mut last_y = zig_y_top;
        for k in 1..=steps {
            let nx = bar_x - 0.02 + (bar_w + 0.04) * k as f64 / steps as f64;
            let ny = if k % 2 == 1 { zig_y_bot } else { zig_y_top };
            self.add_line(last_x, last_y, nx, ny, BORDER_GRAY, 0.75);
            last_x = nx;
            last_y = ny;
        }
    }

    /// `add_bar_chart` (shape-based, sorted desc).
    #[allow(clippy::too_many_arguments)]
    pub fn add_bar_chart(
        &mut self,
        action_title: &str,
        labels: &[String],
        values: &[f64],
        value_format: &str,
        target_label: &str,
        x_label: &str,
        source: &str,
        notes: &str,
    ) -> Result<(), String> {
        let at = self.validate_action_title(action_title)?;
        if labels.len() != values.len() {
            return Err("labels and values must match length".into());
        }
        let n = labels.len();
        if !(1..=12).contains(&n) {
            return Err(format!("bar chart requires 1-12 bars, got {n}"));
        }
        self.content_slide_with_title(&at);
        let mut pairs: Vec<(String, f64)> =
            labels.iter().cloned().zip(values.iter().cloned()).collect();
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let body_top = 0.95;
        let body_bottom = self.slide_h_in - 0.7;
        let body_h = body_bottom - body_top;
        let label_w = 1.6;
        let value_w = 1.0;
        let chart_left = 0.3 + label_w;
        let chart_right = self.slide_w_in - 0.3 - value_w;
        let chart_w = chart_right - chart_left;
        let max_val = pairs.iter().map(|p| p.1.abs()).fold(0.0, f64::max).max(1.0);
        let bar_h = (0.45f64).min((body_h - (n as f64 - 1.0) * 0.12) / n as f64);
        let gap = (0.08f64).max((body_h - n as f64 * bar_h) / (n as f64 - 1.0).max(1.0));

        for (i, (lbl, val)) in pairs.iter().enumerate() {
            let y = body_top + i as f64 * (bar_h + gap);
            let bd = self.brand_dark.clone();
            let fb = self.font_body.clone();
            self.add_text(
                0.3,
                y + bar_h * 0.1,
                label_w - 0.1,
                bar_h * 0.9,
                lbl,
                11,
                false,
                false,
                Some(&bd),
                "r",
                Some(&fb),
                None,
            );
            let bw = if max_val != 0.0 {
                chart_w * (val / max_val)
            } else {
                0.0
            };
            let color = if lbl == target_label {
                self.brand_primary.clone()
            } else {
                self.brand_dark.clone()
            };
            self.add_rect(chart_left, y, (0.05f64).max(bw), bar_h, &color, None, true);
            let vlabel = py_value_format(value_format, *val);
            self.add_text(
                chart_left + bw + 0.05,
                y + bar_h * 0.1,
                value_w,
                bar_h * 0.9,
                &vlabel,
                11,
                false,
                false,
                Some(&bd),
                "l",
                Some(&fb),
                None,
            );
        }
        if !x_label.is_empty() {
            self.add_text(
                chart_left,
                body_bottom + 0.05,
                chart_w,
                0.25,
                x_label,
                9,
                false,
                true,
                Some(FOOTNOTE_GRAY),
                "ctr",
                None,
                None,
            );
        }
        self.add_source_line(source, notes);
        self.page += 1;
        self.add_footer();
        Ok(())
    }

    /// `add_table_of_contents`.
    pub fn add_table_of_contents(
        &mut self,
        action_title: &str,
        entries: &[(String, Option<i64>, i64)],
    ) -> Result<(), String> {
        let at = self.validate_action_title(action_title)?;
        if entries.is_empty() {
            return Err("table_of_contents requires entries".into());
        }
        self.content_slide_with_title(&at);
        let body_top = 1.0;
        let body_bottom = self.slide_h_in - 0.7;
        let avail_h = body_bottom - body_top;
        let line_h = (0.4f64).min(avail_h / (entries.len() as f64).max(1.0));
        let mut counters = [0i64; 3];
        let roman = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
        let body_w = self.slide_w_in - 0.6;
        for (i, (text, page, level)) in entries.iter().enumerate() {
            let lvl = (*level).clamp(1, 3);
            counters[(lvl - 1) as usize] += 1;
            for k in lvl..3 {
                counters[k as usize] = 0;
            }
            let (num, indent, size, bold, color) = if lvl == 1 {
                (
                    format!("{}.", counters[0]),
                    0.0,
                    14,
                    true,
                    self.brand_primary.clone(),
                )
            } else if lvl == 2 {
                (
                    format!("{}.", (b'A' + (counters[1] - 1) as u8) as char),
                    0.4,
                    12,
                    true,
                    self.brand_dark.clone(),
                )
            } else {
                (
                    format!("{}.", roman[(counters[2] - 1).min(9) as usize]),
                    0.8,
                    11,
                    false,
                    self.brand_dark.clone(),
                )
            };
            let y = body_top + i as f64 * line_h;
            let fh = self.font_headline.clone();
            self.add_text(
                0.3 + indent,
                y,
                0.5,
                line_h,
                &num,
                size,
                bold,
                false,
                Some(&color),
                "l",
                Some(&fh),
                None,
            );
            let fb = self.font_body.clone();
            self.add_text(
                0.3 + indent + 0.5,
                y,
                body_w - indent - 1.5,
                line_h,
                text,
                size,
                bold,
                false,
                Some(&color),
                "l",
                Some(&fb),
                None,
            );
            if let Some(p) = page {
                self.add_text(
                    self.slide_w_in - 0.9,
                    y,
                    0.6,
                    line_h,
                    &p.to_string(),
                    size,
                    bold,
                    false,
                    Some(&color),
                    "r",
                    Some(&fb),
                    None,
                );
            }
        }
        Ok(())
    }

    /// `add_table` — a simple grid: header row (brand-dark fill, white bold) over
    /// body rows alternating WHITE / LIGHT_GRAY with hairline row rules. Columns
    /// are evenly divided across the content width.
    pub fn add_table(
        &mut self,
        action_title: &str,
        headers: &[String],
        rows: &[Vec<String>],
        source: &str,
    ) -> Result<(), String> {
        let at = self.validate_action_title(action_title)?;
        if headers.is_empty() {
            return Err("table requires at least one column".into());
        }
        if headers.len() > 8 {
            return Err(format!(
                "table supports up to 8 columns, got {}",
                headers.len()
            ));
        }
        if rows.len() > 14 {
            return Err(format!("table supports up to 14 rows, got {}", rows.len()));
        }
        self.content_slide_with_title(&at);
        let ncols = headers.len();
        let table_left = MARGIN_IN;
        let table_w = self.slide_w_in - 2.0 * MARGIN_IN;
        let col_w = table_w / ncols as f64;
        let top0 = 1.2;
        let row_h = 0.4;
        // Header row.
        let dark = self.brand_dark.clone();
        self.add_rect(table_left, top0, table_w, row_h, &dark, None, true);
        for (c, h) in headers.iter().enumerate() {
            let x = table_left + c as f64 * col_w;
            self.add_text(
                x + 0.08,
                top0,
                col_w - 0.16,
                row_h,
                h,
                11,
                true,
                false,
                Some(WHITE),
                "l",
                None,
                Some("ctr"),
            );
        }
        // Body rows.
        for (r, row) in rows.iter().enumerate() {
            let y = top0 + (r as f64 + 1.0) * row_h;
            let fill = if r % 2 == 0 { WHITE } else { LIGHT_GRAY };
            self.add_rect(table_left, y, table_w, row_h, fill, None, true);
            for (c, cell) in row.iter().take(ncols).enumerate() {
                let x = table_left + c as f64 * col_w;
                let bd = self.brand_dark.clone();
                self.add_text(
                    x + 0.08,
                    y,
                    col_w - 0.16,
                    row_h,
                    cell,
                    10,
                    false,
                    false,
                    Some(&bd),
                    "l",
                    None,
                    Some("ctr"),
                );
            }
            self.add_line(table_left, y, table_left + table_w, y, BORDER_GRAY, 0.5);
        }
        self.add_source_line(source, "");
        self.page += 1;
        self.add_footer();
        Ok(())
    }

    // ── package assembly ────────────────────────────────────────────────────────

    /// Build the deck into an in-memory OOXML package.
    pub fn build(&self) -> crate::pkg::Package {
        crate::writer::pkgbuild::build_package(self.slide_w_in, self.slide_h_in, &self.slides)
    }

    /// Save the deck to `path` (`.pptx` appended if missing).
    pub fn save(&self, path: &str) -> Result<String, String> {
        let out = if path.to_lowercase().ends_with(".pptx") {
            path.to_string()
        } else {
            format!("{path}.pptx")
        };
        self.build().write(&out)?;
        Ok(out)
    }
}

/// One waterfall segment.
#[derive(Debug, Clone)]
pub struct WaterfallSeg {
    pub label: String,
    pub value: f64,
    /// "start" | "plus" | "minus" | "total".
    pub kind: String,
}

// python-pptx default autoshape / connector style blocks (constant).
const STYLE_AUTOSHAPE: &str = "<p:style><a:lnRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"3\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></p:style>";
const STYLE_CONNECTOR: &str = "<p:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></a:fontRef></p:style>";

// ── ResearchPPTXWriter (pptx_output.py) ───────────────────────────────────────

/// Fixed EV-bridge inputs (mirror of `kb.ev_bridge.EVBridgeInput` fields used).
#[derive(Debug, Clone, Default)]
pub struct EvBridgeInput {
    pub company: String,
    pub period: String,
    pub currency: String,
    pub market_cap: Option<f64>,
    pub share_price: Option<f64>,
    pub shares_outstanding: Option<f64>,
    pub total_debt: Option<f64>,
    pub finance_leases: Option<f64>,
    pub operating_leases: Option<f64>,
    pub underfunded_pension: Option<f64>,
    pub minority_interest: Option<f64>,
    pub preferred_stock: Option<f64>,
    pub cash: Option<f64>,
    pub short_term_investments: Option<f64>,
    pub equity_investments: Option<f64>,
    pub ltm_revenue: Option<f64>,
    pub ltm_ebitda: Option<f64>,
}

impl EvBridgeInput {
    fn computed_market_cap(&self) -> f64 {
        if let Some(mc) = self.market_cap {
            return mc;
        }
        match (self.share_price, self.shares_outstanding) {
            (Some(p), Some(s)) => p * s,
            _ => 0.0,
        }
    }
}

/// `ResearchPPTXWriter.write_ev_bridge_deck` — cover + waterfall + scorecard.
pub fn write_ev_bridge_deck(ev: &EvBridgeInput, deck_date: &str) -> Result<PptxDeckWriter, String> {
    let company = if ev.company.is_empty() {
        "Company"
    } else {
        &ev.company
    };
    let period = if ev.period.is_empty() {
        "LTM"
    } else {
        &ev.period
    };
    let currency = if ev.currency.is_empty() {
        "USD"
    } else {
        &ev.currency
    };
    let mc = ev.computed_market_cap();

    let mut segments: Vec<WaterfallSeg> = vec![WaterfallSeg {
        label: "Market Cap".into(),
        value: mc,
        kind: "start".into(),
    }];
    for (value, label) in [
        (ev.total_debt, "Total Debt"),
        (ev.operating_leases, "Operating Leases"),
        (ev.finance_leases, "Finance Leases"),
        (ev.underfunded_pension, "Underfunded Pension"),
        (ev.minority_interest, "Minority Interest"),
        (ev.preferred_stock, "Preferred Stock"),
    ] {
        if let Some(v) = value {
            if v > 0.0 {
                segments.push(WaterfallSeg {
                    label: label.into(),
                    value: v,
                    kind: "plus".into(),
                });
            }
        }
    }
    for (value, label) in [
        (ev.cash, "Cash & Equivalents"),
        (ev.short_term_investments, "Short-term Investments"),
        (ev.equity_investments, "Equity Investments"),
    ] {
        if let Some(v) = value {
            if v > 0.0 {
                segments.push(WaterfallSeg {
                    label: label.into(),
                    value: -v,
                    kind: "minus".into(),
                });
            }
        }
    }
    let mut running = mc;
    for s in &segments[1..] {
        running += s.value;
    }
    let ev_val = running;
    segments.push(WaterfallSeg {
        label: "Enterprise Value".into(),
        value: ev_val,
        kind: "total".into(),
    });
    let use_segments = segments.len() >= 3;

    let mut tiles: Vec<ScorecardTile> = Vec::new();
    if mc != 0.0 {
        tiles.push(ScorecardTile {
            metric: "Market Cap".into(),
            value: fmt_v(Some(mc), currency),
            ..Default::default()
        });
    }
    if ev_val != 0.0 {
        tiles.push(ScorecardTile {
            metric: "Enterprise Value".into(),
            value: fmt_v(Some(ev_val), currency),
            ..Default::default()
        });
    }
    let net_debt =
        ev.total_debt.unwrap_or(0.0) + ev.operating_leases.unwrap_or(0.0) - ev.cash.unwrap_or(0.0);
    tiles.push(ScorecardTile {
        metric: "Net Debt".into(),
        value: fmt_v(Some(net_debt), currency),
        sub: "Debt + Leases − Cash".into(),
        ..Default::default()
    });
    if let Some(eb) = ev.ltm_ebitda {
        if eb > 0.0 {
            tiles.push(ScorecardTile {
                metric: "EV/EBITDA".into(),
                value: format!("{:.1}x", ev_val / eb),
                sub: format!("{period} EBITDA = {}", fmt_v(Some(eb), currency)),
                ..Default::default()
            });
        }
    }
    if let Some(rev) = ev.ltm_revenue {
        if rev > 0.0 {
            tiles.push(ScorecardTile {
                metric: "EV/Revenue".into(),
                value: format!("{:.1}x", ev_val / rev),
                sub: format!("{period} Revenue = {}", fmt_v(Some(rev), currency)),
                ..Default::default()
            });
        }
    }

    let mut deck = PptxDeckWriter::new(&BrandProfile::default(), "CONFIDENTIAL", deck_date);
    deck.add_cover(
        &format!("{company} — Enterprise Value Bridge"),
        &format!("{period} | {currency}"),
    );
    if use_segments {
        let scale = if mc.abs().max(ev_val.abs()) < 1e9 {
            "millions"
        } else {
            "billions"
        };
        deck.add_waterfall(
            &format!(
                "{company} EV bridge: {} market cap → {} enterprise value",
                fmt_v(Some(mc), currency),
                fmt_v(Some(ev_val), currency)
            ),
            &segments,
            "{:+,.0f}",
            &format!("{currency} ({scale})"),
            "Bloomberg, company filings, SEC EDGAR",
            "",
            true,
        )?;
    }
    if !tiles.is_empty() {
        deck.add_scorecard(
            &format!("{company} key valuation metrics ({period})"),
            &tiles,
            "Bloomberg, SEC EDGAR",
            "",
        )?;
    }
    Ok(deck)
}

/// Inputs for `write_ifrs_bridge_deck`.
#[derive(Debug, Clone, Default)]
pub struct IfrsInput {
    pub accounting_standard: String,
    pub reported_ebitda: f64,
    pub rou_depreciation: f64,
    pub lease_interest: f64,
    pub short_term_rent: f64,
    pub adjusted_ebitda: Option<f64>,
}

/// `ResearchPPTXWriter.write_ifrs_bridge_deck` — cover + waterfall + scorecard.
pub fn write_ifrs_bridge_deck(
    inp: &IfrsInput,
    company: &str,
    period: &str,
    revenue: f64,
    deck_date: &str,
) -> Result<PptxDeckWriter, String> {
    let standard = if inp.accounting_standard.is_empty() {
        "IFRS"
    } else {
        &inp.accounting_standard
    };
    let is_ifrs_to_gaap = standard == "IFRS";
    let reported = inp.reported_ebitda;
    let adjusted = inp.adjusted_ebitda.unwrap_or(reported);
    let rou = inp.rou_depreciation;
    let lease_int = inp.lease_interest;
    let short_r = inp.short_term_rent;

    let mut segments = vec![WaterfallSeg {
        label: "Reported EBITDA".into(),
        value: reported,
        kind: "start".into(),
    }];
    if is_ifrs_to_gaap {
        if rou > 0.0 {
            segments.push(WaterfallSeg {
                label: "Less: ROU Depreciation".into(),
                value: -rou,
                kind: "minus".into(),
            });
        }
        if lease_int > 0.0 {
            segments.push(WaterfallSeg {
                label: "Less: Lease Interest".into(),
                value: -lease_int,
                kind: "minus".into(),
            });
        }
        if short_r > 0.0 {
            segments.push(WaterfallSeg {
                label: "Less: Short-term Rent".into(),
                value: -short_r,
                kind: "minus".into(),
            });
        }
    } else {
        if rou > 0.0 {
            segments.push(WaterfallSeg {
                label: "Add: ROU Depreciation".into(),
                value: rou,
                kind: "plus".into(),
            });
        }
        if lease_int > 0.0 {
            segments.push(WaterfallSeg {
                label: "Add: Lease Interest".into(),
                value: lease_int,
                kind: "plus".into(),
            });
        }
        if short_r > 0.0 {
            segments.push(WaterfallSeg {
                label: "Less: Cash Rent".into(),
                value: -short_r,
                kind: "minus".into(),
            });
        }
    }
    segments.push(WaterfallSeg {
        label: "Adj. EBITDA".into(),
        value: adjusted,
        kind: "total".into(),
    });

    let mut tiles: Vec<ScorecardTile> = Vec::new();
    if reported != 0.0 {
        tiles.push(ScorecardTile {
            metric: "Reported EBITDA".into(),
            value: format!("{}M", fmt_grouped(reported / 1e6, 0, false)),
            sub: period.into(),
            ..Default::default()
        });
    }
    if adjusted != 0.0 && adjusted != reported {
        let delta = adjusted - reported;
        tiles.push(ScorecardTile {
            metric: "Adj. EBITDA".into(),
            value: format!("{}M", fmt_grouped(adjusted / 1e6, 0, false)),
            sub: format!("Δ = {}M", fmt_grouped(delta / 1e6, 0, true)),
            ..Default::default()
        });
    }
    if revenue != 0.0 && adjusted != 0.0 {
        tiles.push(ScorecardTile {
            metric: "Adj. EBITDA Margin".into(),
            value: format!("{:.1}%", adjusted / revenue * 100.0),
            sub: format!("Revenue = {}M", fmt_grouped(revenue / 1e6, 0, false)),
            ..Default::default()
        });
    }
    if rou != 0.0 {
        tiles.push(ScorecardTile {
            metric: "ROU Depreciation".into(),
            value: format!("{}M", fmt_grouped(rou / 1e6, 0, false)),
            sub: "Annual report lease note".into(),
            ..Default::default()
        });
    }
    if lease_int != 0.0 {
        tiles.push(ScorecardTile {
            metric: "Lease Interest".into(),
            value: format!("{}M", fmt_grouped(lease_int / 1e6, 0, false)),
            sub: "Annual report lease note".into(),
            ..Default::default()
        });
    }

    let mut deck = PptxDeckWriter::new(&BrandProfile::default(), "CONFIDENTIAL", deck_date);
    deck.add_cover(
        &format!("{company} — IFRS 16 Lease Adjustment"),
        &format!("{period} | {standard} Bridge"),
    );
    if segments.len() >= 3 {
        deck.add_waterfall(
            &format!(
                "{company} EBITDA bridge: reported {}M → adjusted {}M ({period})",
                fmt_grouped(reported / 1e6, 0, false),
                fmt_grouped(adjusted / 1e6, 0, false)
            ),
            &segments,
            "{:+,.0f}",
            standard,
            "Annual report — lease note (IFRS 16 / ASC 842)",
            "",
            false,
        )?;
    }
    if !tiles.is_empty() {
        deck.add_scorecard(
            &format!("{company} IFRS 16 adjustment summary ({period})"),
            &tiles,
            "Annual report, company filings",
            "",
        )?;
    }
    Ok(deck)
}

/// Inputs for [`write_model_deck`] — a one-click summary of a built model.
#[derive(Debug, Clone, Default)]
pub struct ModelDeckInput {
    pub ticker: String,
    pub company: String,
    pub currency: String,
    pub periods: Vec<String>,
    pub revenue: Vec<f64>,
    pub ebitda: Vec<f64>,
    pub hist_n: usize,
    pub implied_price: f64,
    pub current_price: f64,
    pub upside_pct: f64,
    pub wacc: f64,
    pub ev: f64,
    pub tv_method: String,
    pub comps_headers: Vec<String>,
    pub comps_rows: Vec<Vec<String>>,
}

/// Parse the leading numeric value out of a formatted cell (e.g. "23.4%",
/// "$1,234M" → 23.4 / 1234.0). Returns `None` when no number is present.
fn parse_leading_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse::<f64>().ok()
}

/// One-click model summary deck: cover, valuation scorecard, revenue + EBITDA
/// trajectory charts, and an optional trading-comps table.
pub fn write_model_deck(inp: &ModelDeckInput, deck_date: &str) -> Result<PptxDeckWriter, String> {
    let company = if inp.company.is_empty() {
        inp.ticker.as_str()
    } else {
        inp.company.as_str()
    };
    let currency = if inp.currency.is_empty() {
        "USD"
    } else {
        inp.currency.as_str()
    };
    let mut deck = PptxDeckWriter::new(&BrandProfile::default(), "CONFIDENTIAL", deck_date);
    deck.add_cover(
        &format!("{} — Financial model summary", inp.ticker),
        &format!("{company} | {deck_date}"),
    );

    let tiles = vec![
        ScorecardTile {
            metric: "Implied price".into(),
            value: fmt_v(Some(inp.implied_price), currency),
            ..Default::default()
        },
        ScorecardTile {
            metric: "Current price".into(),
            value: fmt_v(Some(inp.current_price), currency),
            ..Default::default()
        },
        ScorecardTile {
            metric: "Upside / downside".into(),
            value: format!("{:+.1}%", inp.upside_pct),
            ..Default::default()
        },
        ScorecardTile {
            metric: "WACC".into(),
            value: format!("{:.1}%", inp.wacc * 100.0),
            ..Default::default()
        },
        ScorecardTile {
            metric: "Enterprise value".into(),
            value: fmt_v(Some(inp.ev), currency),
            ..Default::default()
        },
        ScorecardTile {
            metric: "TV method".into(),
            value: inp.tv_method.clone(),
            ..Default::default()
        },
    ];
    deck.add_scorecard(
        &format!("{company} valuation snapshot at a glance"),
        &tiles,
        "finmodel — SEC EDGAR, Yahoo Finance",
        "",
    )?;

    // Highlight the forecast boundary (first projected period) on both charts.
    let boundary = inp.periods.get(inp.hist_n).cloned().unwrap_or_default();
    if !inp.periods.is_empty() && inp.periods.len() == inp.revenue.len() && inp.periods.len() <= 12
    {
        deck.add_bar_chart(
            &format!("{company} revenue trajectory across the forecast"),
            &inp.periods,
            &inp.revenue,
            "{:,.0f}",
            &boundary,
            currency,
            "finmodel projection",
            "",
        )?;
    }
    if !inp.periods.is_empty() && inp.periods.len() == inp.ebitda.len() && inp.periods.len() <= 12 {
        deck.add_bar_chart(
            &format!("{company} EBITDA trajectory across the forecast"),
            &inp.periods,
            &inp.ebitda,
            "{:,.0f}",
            &boundary,
            currency,
            "finmodel projection",
            "",
        )?;
    }

    if !inp.comps_rows.is_empty() && !inp.comps_headers.is_empty() {
        deck.add_table(
            &format!("{company} trading comparables versus its peer set"),
            &inp.comps_headers,
            &inp.comps_rows,
            "finmodel — SEC EDGAR, Yahoo Finance",
        )?;
    }
    Ok(deck)
}

/// One-click benchmark deck: cover + peer table + (when present) an EBITDA-margin
/// dispersion bar chart across peers.
pub fn write_benchmark_deck(
    title: &str,
    headers: &[String],
    rows: &[Vec<String>],
    deck_date: &str,
) -> Result<PptxDeckWriter, String> {
    let mut deck = PptxDeckWriter::new(&BrandProfile::default(), "CONFIDENTIAL", deck_date);
    deck.add_cover(title, "Peer benchmarking");
    if !headers.is_empty() && !rows.is_empty() {
        deck.add_table(
            &format!("{title}: side-by-side peer comparison"),
            headers,
            rows,
            "finmodel — SEC EDGAR, Yahoo Finance",
        )?;
    }
    // Optional EBITDA-margin dispersion chart (col 0 assumed to be the label).
    if let Some(col) = headers
        .iter()
        .position(|h| h.to_lowercase().contains("ebitda margin"))
    {
        let labels: Vec<String> = rows
            .iter()
            .map(|r| r.first().cloned().unwrap_or_default())
            .collect();
        let values: Vec<f64> = rows
            .iter()
            .map(|r| {
                r.get(col)
                    .and_then(|s| parse_leading_number(s))
                    .unwrap_or(0.0)
            })
            .collect();
        if !labels.is_empty() && labels.len() == values.len() && labels.len() <= 12 {
            // Best-effort: a malformed chart must not sink the whole deck.
            let _ = deck.add_bar_chart(
                &format!("{title}: EBITDA margin dispersion across peers"),
                &labels,
                &values,
                "{:,.1f}",
                "",
                "EBITDA margin (%)",
                "finmodel — SEC EDGAR filings",
                "",
            );
        }
    }
    Ok(deck)
}
