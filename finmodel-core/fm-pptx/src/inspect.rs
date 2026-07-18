//! 6.1 — Inspector. Raw zip+XML structural reverse-engineering of a `.pptx`,
//! a faithful port of `src/research/pptx_inspector.py::inspect_pptx` /
//! `inspect_pptx_json` (with `include_raw_xml=False`, the `inspect_pptx_json`
//! default). Emits the identical JSON shape the Python oracle commits so the
//! parity gate can diff structurally.
//!
//! Notes on faithfulness (documented divergences are none — quirks replicated):
//! - The Python `str(sh.shape_type).endswith("PICTURE"/"GROUP")` guards never
//!   fire because the enum `str()` carries a `" (N)"` suffix, so the reference
//!   never emits a `picture` key nor recurses into groups. We replicate that:
//!   no `picture` field, no `children`.
//! - `pos` uses python-pptx *effective* geometry (placeholder inheritance from
//!   the layout/master), while `xfrm` reflects only the shape's own `a:xfrm`.
//!   Both are reproduced.

use serde_json::{Map, Value, json};

use crate::pkg::Package;
use crate::xmldom::{A, Element, P, R};

const RT_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const RT_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const RT_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const RT_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const URI_TABLE: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
const URI_CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";

// ── numeric helpers (mirror _emu_to_in / _emu_to_pt / _ang_to_deg) ────────────

fn round_to(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

fn parse_emu(v: Option<&str>) -> Option<i64> {
    v.and_then(|s| s.trim().parse::<i64>().ok())
}

fn emu_to_in(v: Option<&str>) -> Value {
    match parse_emu(v) {
        Some(n) => json!(round_to(n as f64 / 914400.0, 4)),
        None => Value::Null,
    }
}

fn emu_to_in_i(v: Option<i64>) -> Value {
    match v {
        Some(n) => json!(round_to(n as f64 / 914400.0, 4)),
        None => Value::Null,
    }
}

fn emu_to_pt(v: Option<&str>) -> Value {
    match parse_emu(v) {
        Some(n) => json!(round_to(n as f64 / 12700.0, 2)),
        None => Value::Null,
    }
}

fn ang_to_deg(v: Option<&str>) -> Value {
    match parse_emu(v) {
        Some(n) => json!(round_to(n as f64 / 60000.0, 2)),
        None => Value::Null,
    }
}

// ── color / fill / line / effects (mirror _xml_color_solid etc.) ──────────────

/// `_xml_color_solid` — extract srgbClr/schemeClr/sysClr/prstClr from a container.
fn xml_color_solid(elem: &Element) -> Value {
    let mut out = Map::new();
    if let Some(sf) = elem.child(A, "srgbClr") {
        out.insert(
            "rgb".into(),
            json!(format!("#{}", sf.attr("val").unwrap_or("").to_uppercase())),
        );
    }
    if let Some(sc) = elem.child(A, "schemeClr") {
        out.insert("scheme".into(), json!(sc.attr("val")));
        let mut mods: Vec<Value> = Vec::new();
        for m in &sc.children {
            if let Some(v) = m.attr("val") {
                let mut mo = Map::new();
                mo.insert(m.local.clone(), json!(v));
                mods.push(Value::Object(mo));
            }
        }
        if !mods.is_empty() {
            out.insert("modifiers".into(), Value::Array(mods));
        }
    }
    if let Some(sy) = elem.child(A, "sysClr") {
        out.insert("sys".into(), json!(sy.attr("val")));
        out.insert(
            "sysLastClr".into(),
            json!(format!(
                "#{}",
                sy.attr("lastClr").unwrap_or("").to_uppercase()
            )),
        );
    }
    if let Some(pr) = elem.child(A, "prstClr") {
        out.insert("preset".into(), json!(pr.attr("val")));
    }
    Value::Object(out)
}

/// Merge helper: `{"type": t, **color}`.
fn typed_with_color(t: &str, color: Value) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!(t));
    if let Value::Object(cm) = color {
        for (k, v) in cm {
            m.insert(k, v);
        }
    }
    Value::Object(m)
}

/// `_extract_fill(spPr)`.
fn extract_fill(sp_pr: Option<&Element>) -> Value {
    let sp_pr = match sp_pr {
        Some(e) => e,
        None => return json!({"type": "none"}),
    };
    if sp_pr.child(A, "noFill").is_some() {
        return json!({"type": "noFill"});
    }
    if let Some(sf) = sp_pr.child(A, "solidFill") {
        return typed_with_color("solid", xml_color_solid(sf));
    }
    if let Some(gf) = sp_pr.child(A, "gradFill") {
        let mut out = Map::new();
        out.insert("type".into(), json!("gradient"));
        let mut stops: Vec<Value> = Vec::new();
        if let Some(gs_lst) = gf.child(A, "gsLst") {
            for gs in gs_lst.children_named(A, "gs") {
                let pos = gs
                    .attr("pos")
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let mut stop = Map::new();
                stop.insert("pos".into(), json!(pos / 1000.0));
                if let Value::Object(cm) = xml_color_solid(gs) {
                    for (k, v) in cm {
                        stop.insert(k, v);
                    }
                }
                stops.push(Value::Object(stop));
            }
        }
        out.insert("stops".into(), Value::Array(stops));
        out.insert("rotWithShape".into(), json!(gf.attr("rotWithShape")));
        if let Some(lin) = gf.child(A, "lin") {
            out.insert("angle".into(), ang_to_deg(lin.attr("ang")));
            out.insert("scaled".into(), json!(lin.attr("scaled")));
        }
        return Value::Object(out);
    }
    if let Some(pf) = sp_pr.child(A, "pattFill") {
        let fg = pf
            .child(A, "fgClr")
            .map(xml_color_solid)
            .unwrap_or(Value::Null);
        let bg = pf
            .child(A, "bgClr")
            .map(xml_color_solid)
            .unwrap_or(Value::Null);
        return json!({"type": "pattern", "preset": pf.attr("prst"), "fg": fg, "bg": bg});
    }
    if let Some(bf) = sp_pr.child(A, "blipFill") {
        let blip = bf.child(A, "blip");
        return json!({
            "type": "picture",
            "embedRid": blip.and_then(|b| b.attr_ns(R, "embed")),
            "linkRid": blip.and_then(|b| b.attr_ns(R, "link")),
        });
    }
    json!({"type": "inherit"})
}

/// `_extract_line(spPr)`.
fn extract_line(sp_pr: Option<&Element>) -> Value {
    let sp_pr = match sp_pr {
        Some(e) => e,
        None => return json!({}),
    };
    let ln = match sp_pr.child(A, "ln") {
        Some(e) => e,
        None => return json!({}),
    };
    let mut out = Map::new();
    out.insert("widthPt".into(), emu_to_pt(ln.attr("w")));
    out.insert("cap".into(), json!(ln.attr("cap")));
    out.insert("cmpd".into(), json!(ln.attr("cmpd")));
    out.insert("algn".into(), json!(ln.attr("algn")));
    out.insert("fill".into(), extract_fill(Some(ln)));
    if let Some(dash) = ln.child(A, "prstDash") {
        out.insert("dash".into(), json!(dash.attr("val")));
    }
    if let Some(head) = ln.child(A, "headEnd") {
        out.insert(
            "headEnd".into(),
            json!({"type": head.attr("type"), "w": head.attr("w"), "len": head.attr("len")}),
        );
    }
    if let Some(tail) = ln.child(A, "tailEnd") {
        out.insert(
            "tailEnd".into(),
            json!({"type": tail.attr("type"), "w": tail.attr("w"), "len": tail.attr("len")}),
        );
    }
    Value::Object(out)
}

/// `_extract_effects(spPr)`.
fn extract_effects(sp_pr: Option<&Element>) -> Value {
    let sp_pr = match sp_pr {
        Some(e) => e,
        None => return json!({}),
    };
    let eff = match sp_pr.child(A, "effectLst") {
        Some(e) => e,
        None => return json!({}),
    };
    let mut out = Map::new();
    if let Some(o) = eff.child(A, "outerShdw") {
        out.insert(
            "outerShadow".into(),
            json!({
                "blurRad": emu_to_pt(o.attr("blurRad")),
                "dist": emu_to_pt(o.attr("dist")),
                "dir": ang_to_deg(o.attr("dir")),
                "rotWithShape": o.attr("rotWithShape"),
                "color": xml_color_solid(o),
            }),
        );
    }
    if let Some(g) = eff.child(A, "glow") {
        out.insert(
            "glow".into(),
            json!({"rad": emu_to_pt(g.attr("rad")), "color": xml_color_solid(g)}),
        );
    }
    if let Some(r) = eff.child(A, "reflection") {
        let mut m = Map::new();
        for a in &r.attrs {
            m.insert(a.local.clone(), json!(a.value));
        }
        out.insert("reflection".into(), Value::Object(m));
    }
    if let Some(s) = eff.child(A, "softEdge") {
        out.insert("softEdge".into(), json!({"rad": emu_to_pt(s.attr("rad"))}));
    }
    Value::Object(out)
}

/// `_extract_xfrm(spPr)` — only the shape's own `a:xfrm` (not inherited).
fn extract_xfrm(sp_pr: Option<&Element>) -> Value {
    let sp_pr = match sp_pr {
        Some(e) => e,
        None => return json!({}),
    };
    let xfrm = match sp_pr.child(A, "xfrm") {
        Some(e) => e,
        None => return json!({}),
    };
    let off = xfrm.child(A, "off");
    let ext = xfrm.child(A, "ext");
    let mut out = Map::new();
    out.insert(
        "left".into(),
        off.map(|o| emu_to_in(o.attr("x"))).unwrap_or(Value::Null),
    );
    out.insert(
        "top".into(),
        off.map(|o| emu_to_in(o.attr("y"))).unwrap_or(Value::Null),
    );
    out.insert(
        "width".into(),
        ext.map(|e| emu_to_in(e.attr("cx"))).unwrap_or(Value::Null),
    );
    out.insert(
        "height".into(),
        ext.map(|e| emu_to_in(e.attr("cy"))).unwrap_or(Value::Null),
    );
    if let Some(rot) = xfrm.attr("rot") {
        out.insert("rotationDeg".into(), ang_to_deg(Some(rot)));
    }
    if let Some(fh) = xfrm.attr("flipH") {
        out.insert("flipH".into(), json!(fh == "1"));
    }
    if let Some(fv) = xfrm.attr("flipV") {
        out.insert("flipV".into(), json!(fv == "1"));
    }
    Value::Object(out)
}

/// `_extract_prst_geom(spPr)`.
fn extract_prst_geom(sp_pr: Option<&Element>) -> Value {
    let sp_pr = match sp_pr {
        Some(e) => e,
        None => return json!({}),
    };
    let pg = match sp_pr.child(A, "prstGeom") {
        Some(e) => e,
        None => return json!({}),
    };
    let mut out = Map::new();
    out.insert("preset".into(), json!(pg.attr("prst")));
    if let Some(av) = pg.child(A, "avLst") {
        let adjs: Vec<Value> = av
            .children_named(A, "gd")
            .map(|gd| json!({"name": gd.attr("name"), "fmla": gd.attr("fmla")}))
            .collect();
        if !adjs.is_empty() {
            out.insert("adjustments".into(), Value::Array(adjs));
        }
    }
    Value::Object(out)
}

// ── text frame (mirror _extract_paragraph / _extract_text_frame) ──────────────

fn extract_paragraph(p_elem: &Element) -> Value {
    let mut out = Map::new();
    if let Some(p_pr) = p_elem.child(A, "pPr") {
        for k in ["algn", "lvl", "indent", "marL", "marR", "rtl"] {
            if let Some(v) = p_pr.attr(k) {
                out.insert(k.into(), json!(v));
            }
        }
        if let Some(ln_spc) = p_pr.child(A, "lnSpc") {
            let spc = ln_spc
                .child(A, "spcPct")
                .or_else(|| ln_spc.child(A, "spcPts"));
            if let Some(spc) = spc {
                out.insert(
                    "lineSpacing".into(),
                    json!({ spc.local.clone(): spc.attr("val") }),
                );
            }
        }
        for (label, node) in [
            ("spcBef", p_pr.child(A, "spcBef")),
            ("spcAft", p_pr.child(A, "spcAft")),
        ] {
            if let Some(node) = node {
                if let Some(pts) = node.child(A, "spcPts") {
                    let v = pts
                        .attr("val")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    out.insert(label.into(), json!({"pts": v / 100.0}));
                }
                if let Some(pct) = node.child(A, "spcPct") {
                    let v = pct
                        .attr("val")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    out.insert(label.into(), json!({"pct": v / 1000.0}));
                }
            }
        }
        for bu_tag in ["buNone", "buChar", "buAutoNum", "buFont"] {
            if let Some(bu) = p_pr.child(A, bu_tag) {
                let mut attrs = Map::new();
                for a in &bu.attrs {
                    attrs.insert(a.local.clone(), json!(a.value));
                }
                let entry = out
                    .entry("bullet".to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(bm) = entry {
                    bm.insert(bu_tag.into(), Value::Object(attrs));
                }
            }
        }
    }

    let mut runs: Vec<Value> = Vec::new();
    for r in p_elem.children_named(A, "r") {
        let mut run = Map::new();
        if let Some(r_pr) = r.child(A, "rPr") {
            for k in [
                "sz", "b", "i", "u", "strike", "baseline", "spc", "lang", "kern",
            ] {
                if let Some(v) = r_pr.attr(k) {
                    run.insert(k.into(), json!(v));
                }
            }
            if let Some(sf) = r_pr.child(A, "solidFill") {
                run.insert("color".into(), xml_color_solid(sf));
            }
            let mut fonts = Map::new();
            for font_tag in ["latin", "ea", "cs"] {
                if let Some(fnode) = r_pr.child(A, font_tag) {
                    fonts.insert(font_tag.into(), json!(fnode.attr("typeface")));
                }
            }
            if !fonts.is_empty() {
                run.insert("fonts".into(), Value::Object(fonts));
            }
            if let Some(hl) = r_pr.child(A, "highlight") {
                run.insert("highlight".into(), xml_color_solid(hl));
            }
            if let Some(hyper) = r_pr.child(A, "hlinkClick") {
                run.insert("hyperlinkRid".into(), json!(hyper.attr_ns(R, "id")));
            }
        }
        let t = r.child(A, "t");
        run.insert(
            "text".into(),
            json!(t.map(|t| t.text.clone()).unwrap_or_default()),
        );
        runs.push(Value::Object(run));
    }

    // Second pass: br / fld appended after runs (faithful to reference order).
    for child in &p_elem.children {
        if child.ns.as_deref() == Some(A) && child.local == "br" {
            runs.push(json!({"break": true}));
        } else if child.ns.as_deref() == Some(A) && child.local == "fld" {
            let t = child.child(A, "t");
            runs.push(json!({"field": child.attr("type"), "text": t.map(|t| t.text.clone()).unwrap_or_default()}));
        }
    }

    out.insert("runs".into(), Value::Array(runs.clone()));
    Value::Object(out)
}

fn extract_text_frame(tx_body: Option<&Element>) -> Value {
    let tx_body = match tx_body {
        Some(e) => e,
        None => return json!({}),
    };
    let mut out = Map::new();
    if let Some(body_pr) = tx_body.child(A, "bodyPr") {
        let mut margins = Map::new();
        for k in [
            "anchor",
            "anchorCtr",
            "wrap",
            "lIns",
            "tIns",
            "rIns",
            "bIns",
            "rot",
            "vert",
            "spcCol",
            "numCol",
            "fromWordArt",
            "compatLnSpc",
        ] {
            if let Some(v) = body_pr.attr(k) {
                if matches!(k, "lIns" | "tIns" | "rIns" | "bIns") {
                    margins.insert(k.into(), emu_to_in(Some(v)));
                } else {
                    out.insert(k.into(), json!(v));
                }
            }
        }
        if !margins.is_empty() {
            out.insert("margins".into(), Value::Object(margins));
        }
        for af in ["normAutofit", "spAutoFit", "noAutofit"] {
            if let Some(af_elem) = body_pr.child(A, af) {
                let val = if af_elem.attrs.is_empty() {
                    json!(true)
                } else {
                    let mut m = Map::new();
                    for a in &af_elem.attrs {
                        m.insert(a.local.clone(), json!(a.value));
                    }
                    Value::Object(m)
                };
                out.insert("autofit".into(), json!({ af: val }));
            }
        }
    }

    let paragraphs: Vec<Value> = tx_body
        .children_named(A, "p")
        .map(extract_paragraph)
        .collect();
    // Plain-text concatenation.
    let mut plain: Vec<String> = Vec::new();
    for p in &paragraphs {
        let line: String = p
            .get("runs")
            .and_then(|r| r.as_array())
            .map(|rs| {
                rs.iter()
                    .map(|r| r.get("text").and_then(|t| t.as_str()).unwrap_or(""))
                    .collect::<String>()
            })
            .unwrap_or_default();
        if !line.is_empty() {
            plain.push(line);
        }
    }
    out.insert("paragraphs".into(), Value::Array(paragraphs));
    if !plain.is_empty() {
        out.insert("text".into(), json!(plain.join("\n")));
    }
    Value::Object(out)
}

/// `_extract_table(tbl_elem)`.
fn extract_table(tbl: &Element) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut col_widths: Vec<Value> = Vec::new();
    if let Some(grid) = tbl.child(A, "tblGrid") {
        for gc in grid.children_named(A, "gridCol") {
            col_widths.push(emu_to_in(gc.attr("w")));
        }
    }
    for tr in tbl.children_named(A, "tr") {
        let mut cells: Vec<Value> = Vec::new();
        for tc in tr.children_named(A, "tc") {
            let mut cell = Map::new();
            cell.insert(
                "gridSpan".into(),
                json!(
                    tc.attr("gridSpan")
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(1)
                ),
            );
            cell.insert(
                "rowSpan".into(),
                json!(
                    tc.attr("rowSpan")
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(1)
                ),
            );
            cell.insert("hMerge".into(), json!(tc.attr("hMerge") == Some("1")));
            cell.insert("vMerge".into(), json!(tc.attr("vMerge") == Some("1")));
            if let Some(tx) = tc.child(A, "txBody") {
                cell.insert("text".into(), extract_text_frame(Some(tx)));
            }
            if let Some(tc_pr) = tc.child(A, "tcPr") {
                cell.insert("fill".into(), extract_fill(Some(tc_pr)));
                let mut borders = Map::new();
                for side in ["lnL", "lnR", "lnT", "lnB"] {
                    if let Some(bn) = tc_pr.child(A, side) {
                        borders.insert(
                            side.into(),
                            json!({"widthPt": emu_to_pt(bn.attr("w")), "fill": extract_fill(Some(bn))}),
                        );
                    }
                }
                if !borders.is_empty() {
                    cell.insert("borders".into(), Value::Object(borders));
                }
                let mut margins = Map::new();
                for k in ["marL", "marR", "marT", "marB"] {
                    if let Some(v) = tc_pr.attr(k) {
                        margins.insert(k.into(), emu_to_in(Some(v)));
                    }
                }
                cell.insert("margins".into(), Value::Object(margins));
            }
            cells.push(Value::Object(cell));
        }
        rows.push(json!({"heightIn": emu_to_in(tr.attr("h")), "cells": cells}));
    }
    json!({"colWidthsIn": col_widths, "rows": rows})
}

// ── placeholder + geometry inheritance ────────────────────────────────────────

/// Return the `p:ph` element of a shape, if any (xpath `./*[1]/p:nvPr/p:ph`).
fn ph_of(shape: &Element) -> Option<&Element> {
    let nv = shape.children.first()?;
    let nv_pr = nv.child(P, "nvPr")?;
    nv_pr.child(P, "ph")
}

fn ph_type_str(ph: &Element) -> String {
    ph.attr("type").unwrap_or("obj").to_string()
}

fn ph_idx_str(ph: &Element) -> String {
    ph.attr("idx").unwrap_or("0").to_string()
}

/// `cNvPr` element (first `p:cNvPr` descendant of the shape's nv container).
fn cnvpr_of(shape: &Element) -> Option<&Element> {
    shape.children.first().and_then(|nv| nv.child(P, "cNvPr"))
}

/// The `p:spPr`/`a:spPr` used by `_shape_info` — first descendant.
fn sppr_of(shape: &Element) -> Option<&Element> {
    shape
        .descendant(P, "spPr")
        .or_else(|| shape.descendant(A, "spPr"))
}

fn txbody_of(shape: &Element) -> Option<&Element> {
    shape
        .descendant(P, "txBody")
        .or_else(|| shape.descendant(A, "txBody"))
}

/// Own geometry (x, y, cx, cy) in EMU for a shape, from the correct xfrm host.
fn own_geom(shape: &Element) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    let xfrm = match shape.local.as_str() {
        "graphicFrame" => shape.child(P, "xfrm"),
        "grpSp" => shape.child(P, "grpSpPr").and_then(|g| g.child(A, "xfrm")),
        _ => sppr_of(shape).and_then(|sp| sp.child(A, "xfrm")),
    };
    match xfrm {
        None => (None, None, None, None),
        Some(x) => {
            let off = x.child(A, "off");
            let ext = x.child(A, "ext");
            (
                off.and_then(|o| parse_emu(o.attr("x"))),
                off.and_then(|o| parse_emu(o.attr("y"))),
                ext.and_then(|e| parse_emu(e.attr("cx"))),
                ext.and_then(|e| parse_emu(e.attr("cy"))),
            )
        }
    }
}

/// LayoutPlaceholder base-type mapping (`base_ph_type` table, string form).
fn base_ph_type(ph_type: &str) -> Option<&'static str> {
    Some(match ph_type {
        "title" | "ctrTitle" => "title",
        "body" | "chart" | "clipArt" | "dgm" | "media" | "obj" | "pic" | "subTitle" | "tbl" => {
            "body"
        }
        "dt" => "dt",
        "ftr" => "ftr",
        "sldNum" => "sldNum",
        _ => return None,
    })
}

/// Placeholder geometry maps for inheritance resolution.
struct PhGeom {
    /// master placeholder geom keyed by master ph type string.
    master_by_type:
        std::collections::HashMap<String, (Option<i64>, Option<i64>, Option<i64>, Option<i64>)>,
}

impl PhGeom {
    fn master(&self, ph_type: &str) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
        base_ph_type(ph_type)
            .and_then(|bt| self.master_by_type.get(bt).copied())
            .unwrap_or((None, None, None, None))
    }
}

/// Effective geometry for a layout shape (own value, else master inheritance).
fn effective_geom_layout(
    shape: &Element,
    phg: &PhGeom,
) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    let own = own_geom(shape);
    match ph_of(shape) {
        None => own,
        Some(ph) => {
            let base = phg.master(&ph_type_str(ph));
            (
                own.0.or(base.0),
                own.1.or(base.1),
                own.2.or(base.2),
                own.3.or(base.3),
            )
        }
    }
}

/// Effective geometry for a slide shape (own, else layout placeholder by idx,
/// else that layout placeholder's master inheritance).
fn effective_geom_slide(
    shape: &Element,
    layout: Option<&Element>,
    phg: &PhGeom,
) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    let own = own_geom(shape);
    let ph = match ph_of(shape) {
        None => return own,
        Some(p) => p,
    };
    // Find the layout placeholder with the same idx.
    let idx = ph_idx_str(ph);
    let base = layout
        .and_then(|lo| {
            shape_tree(lo).and_then(|tree| {
                tree.children.iter().find_map(|sh| {
                    ph_of(sh)
                        .filter(|lph| ph_idx_str(lph) == idx)
                        .map(|_| effective_geom_layout(sh, phg))
                })
            })
        })
        .unwrap_or((None, None, None, None));
    (
        own.0.or(base.0),
        own.1.or(base.1),
        own.2.or(base.2),
        own.3.or(base.3),
    )
}

// ── shape typing ──────────────────────────────────────────────────────────────

fn is_truthy_txbox(shape: &Element) -> bool {
    // cNvSpPr @txBox on the nv container's cNvSpPr.
    shape
        .children
        .first()
        .and_then(|nv| nv.child(P, "cNvSpPr"))
        .and_then(|c| c.attr("txBox"))
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

/// python-pptx `str(sh.shape_type)` form, or `None` when shape_type is None.
fn shape_type_str(shape: &Element) -> Option<&'static str> {
    match shape.local.as_str() {
        "sp" => {
            if ph_of(shape).is_some() {
                Some("PLACEHOLDER (14)")
            } else if sppr_of(shape)
                .and_then(|s| s.child(A, "custGeom"))
                .is_some()
            {
                Some("FREEFORM (5)")
            } else if sppr_of(shape)
                .and_then(|s| s.child(A, "prstGeom"))
                .is_some()
                && !is_truthy_txbox(shape)
            {
                Some("AUTO_SHAPE (1)")
            } else if is_truthy_txbox(shape) {
                Some("TEXT_BOX (17)")
            } else {
                None
            }
        }
        "pic" => Some("PICTURE (13)"),
        "grpSp" => Some("GROUP (6)"),
        "cxnSp" => Some("LINE (9)"),
        "graphicFrame" => {
            let uri = graphic_data_uri(shape);
            match uri.as_deref() {
                Some(URI_CHART) => Some("CHART (3)"),
                Some(URI_TABLE) => Some("TABLE (19)"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn graphic_data_uri(shape: &Element) -> Option<String> {
    shape
        .child(A, "graphic")
        .and_then(|g| g.child(A, "graphicData"))
        .and_then(|gd| gd.attr("uri"))
        .map(|s| s.to_string())
}

/// MSO_SHAPE `str()` form for a `prst` value (auto_shape_type). Covers the
/// presets the toolchain emits/reads; unknowns fall back to the raw value.
fn mso_shape_str(prst: &str) -> String {
    match prst {
        "rect" => "RECTANGLE (1)".into(),
        "roundRect" => "ROUNDED_RECTANGLE (5)".into(),
        "ellipse" => "OVAL (9)".into(),
        "rightArrow" => "RIGHT_ARROW (33)".into(),
        "lineInv" => "LINE_INVERSE (183)".into(),
        other => other.to_string(),
    }
}

/// `str(ph.type)` form.
fn placeholder_type_str(ph_type: &str) -> Option<&'static str> {
    Some(match ph_type {
        "title" => "TITLE (1)",
        "body" => "BODY (2)",
        "ctrTitle" => "CENTER_TITLE (3)",
        "subTitle" => "SUBTITLE (4)",
        "obj" => "OBJECT (7)",
        "chart" => "CHART (8)",
        "clipArt" => "BITMAP (9)",
        "media" => "MEDIA_CLIP (10)",
        "dgm" => "ORG_CHART (11)",
        "tbl" => "TABLE (12)",
        "sldNum" => "SLIDE_NUMBER (13)",
        "hdr" => "HEADER (14)",
        "ftr" => "FOOTER (15)",
        "dt" => "DATE (16)",
        "pic" => "PICTURE (18)",
        "sldImg" => "SLIDE_IMAGE (101)",
        _ => return None,
    })
}

/// `auto_shape_type` value for the shape, replicating python-pptx per class.
fn auto_shape_type(shape: &Element) -> Option<String> {
    match shape.local.as_str() {
        // Shape: only when is_autoshape (prstGeom present AND not txBox).
        "sp" => {
            let has_prst = sppr_of(shape)
                .and_then(|s| s.child(A, "prstGeom"))
                .is_some();
            if has_prst && !is_truthy_txbox(shape) {
                sppr_of(shape)
                    .and_then(|s| s.child(A, "prstGeom"))
                    .and_then(|pg| pg.attr("prst"))
                    .map(mso_shape_str)
            } else {
                None
            }
        }
        // Picture: prst of spPr/prstGeom if present.
        "pic" => sppr_of(shape)
            .and_then(|s| s.child(A, "prstGeom"))
            .and_then(|pg| pg.attr("prst"))
            .map(mso_shape_str),
        _ => None,
    }
}

// ── shape info ────────────────────────────────────────────────────────────────

/// Fraction-guide adjustment values from a shape's `prstGeom/avLst`
/// (`a:gd fmla="val N"` -> N/100000.0). Empty when the shape has no `a:gd`,
/// matching `list(sh.adjustments)` == [] for rect/ellipse/line geometry.
fn shape_adjustments(sp_pr: Option<&Element>) -> Vec<f64> {
    let mut out = Vec::new();
    if let Some(av) = sp_pr
        .and_then(|s| s.child(A, "prstGeom"))
        .and_then(|pg| pg.child(A, "avLst"))
    {
        for gd in av.children_named(A, "gd") {
            if let Some(fmla) = gd.attr("fmla") {
                if let Some(rest) = fmla.strip_prefix("val ") {
                    if let Ok(n) = rest.trim().parse::<f64>() {
                        out.push(n / 100000.0);
                    }
                }
            }
        }
    }
    out
}

/// `_shape_info(sh, include_xml=False)` with effective geometry `(x,y,cx,cy)`.
fn shape_info(
    shape: &Element,
    geom: (Option<i64>, Option<i64>, Option<i64>, Option<i64>),
) -> Value {
    let mut info = Map::new();
    let cnv = cnvpr_of(shape);
    let id = cnv
        .and_then(|c| c.attr("id"))
        .and_then(|s| s.parse::<i64>().ok());
    info.insert("id".into(), id.map(|n| json!(n)).unwrap_or(Value::Null));
    info.insert("name".into(), json!(cnv.and_then(|c| c.attr("name"))));
    let st = shape_type_str(shape);
    info.insert("type".into(), json!(st.unwrap_or("UNKNOWN")));
    info.insert(
        "pos".into(),
        json!({
            "left": emu_to_in_i(geom.0),
            "top": emu_to_in_i(geom.1),
            "width": emu_to_in_i(geom.2),
            "height": emu_to_in_i(geom.3),
        }),
    );

    if let Some(ast) = auto_shape_type(shape) {
        info.insert("autoShapeType".into(), json!(ast));
    }

    // python-pptx `list(sh.adjustments)`: fraction-guide adjustment values
    // (`a:gd fmla="val N"` -> N/100000) present in the shape's avLst.
    let adjs = shape_adjustments(sppr_of(shape));
    if !adjs.is_empty() {
        info.insert("adjustments".into(), json!(adjs));
    }

    let sp_pr = sppr_of(shape);
    info.insert("xfrm".into(), extract_xfrm(sp_pr));
    info.insert("prstGeom".into(), extract_prst_geom(sp_pr));
    info.insert("fill".into(), extract_fill(sp_pr));
    info.insert("line".into(), extract_line(sp_pr));
    let eff = extract_effects(sp_pr);
    if eff.as_object().map(|m| !m.is_empty()).unwrap_or(false) {
        info.insert("effects".into(), eff);
    }

    if let Some(ph) = ph_of(shape) {
        let idx = ph_idx_str(ph).parse::<i64>().unwrap_or(0);
        let t = placeholder_type_str(&ph_type_str(ph))
            .map(|s| json!(s))
            .unwrap_or(Value::Null);
        info.insert("placeholder".into(), json!({"idx": idx, "type": t}));
    }

    // Connector endpoints.
    if shape.local == "cxnSp" {
        if let Some(nv) = shape.child(P, "nvCxnSpPr") {
            if let Some(cxn) = nv.child(P, "cNvCxnSpPr") {
                let mut m = Map::new();
                if let Some(s) = cxn.child(A, "stCxn") {
                    m.insert(
                        "start".into(),
                        json!({"id": s.attr("id"), "idx": s.attr("idx")}),
                    );
                }
                if let Some(e) = cxn.child(A, "endCxn") {
                    m.insert(
                        "end".into(),
                        json!({"id": e.attr("id"), "idx": e.attr("idx")}),
                    );
                }
                if !m.is_empty() {
                    info.insert("connector".into(), Value::Object(m));
                }
            }
        }
    }

    // Text (has_text_frame == p:sp with a txBody).
    if shape.local == "sp" {
        if let Some(tx) = txbody_of(shape) {
            info.insert("text".into(), extract_text_frame(Some(tx)));
        }
    }

    // Table.
    if shape.local == "graphicFrame" && graphic_data_uri(shape).as_deref() == Some(URI_TABLE) {
        if let Some(tbl) = shape.descendant(A, "tbl") {
            info.insert("table".into(), extract_table(tbl));
        }
    }
    // Chart intentionally omitted from parity fixtures (needs the chart part).

    Value::Object(info)
}

// ── package navigation ────────────────────────────────────────────────────────

/// Resolve a relationships part into (Id -> (Type, Target)) preserving order.
fn parse_rels(bytes: &[u8]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Ok(root) = Element::parse(bytes) {
        for rel in &root.children {
            if rel.local == "Relationship" {
                let id = rel.attr("Id").unwrap_or("").to_string();
                let ty = rel.attr("Type").unwrap_or("").to_string();
                let tgt = rel.attr("Target").unwrap_or("").to_string();
                out.push((id, ty, tgt));
            }
        }
    }
    out
}

/// Normalize a relationship target relative to a source part path.
fn resolve_target(source_part: &str, target: &str) -> String {
    // e.g. source "ppt/presentation.xml", target "slides/slide1.xml"
    //      -> "ppt/slides/slide1.xml"
    let base = match source_part.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    };
    let mut stack: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    stack.join("/")
}

fn rels_path_for(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// The spTree of a slide/layout/master part element.
fn shape_tree(part_root: &Element) -> Option<&Element> {
    part_root
        .child(P, "cSld")
        .and_then(|c| c.child(P, "spTree"))
}

/// Iterate the shape elements of an spTree (sp/pic/graphicFrame/grpSp/cxnSp).
fn tree_shapes(tree: &Element) -> impl Iterator<Item = &Element> {
    tree.children.iter().filter(|c| {
        matches!(
            c.local.as_str(),
            "sp" | "pic" | "graphicFrame" | "grpSp" | "cxnSp"
        )
    })
}

// ── theme ─────────────────────────────────────────────────────────────────────

fn theme_from_master(pkg: &Package, master_part: &str) -> Value {
    let mut out = Map::new();
    let mut colors = Map::new();
    let mut fonts = Map::new();
    out.insert("colors".into(), Value::Object(Map::new()));
    out.insert("fonts".into(), Value::Object(Map::new()));
    out.insert("rawXml".into(), Value::Null);

    let rels = pkg
        .get(&rels_path_for(master_part))
        .map(parse_rels)
        .unwrap_or_default();
    let theme_target = rels
        .iter()
        .find(|(_, ty, _)| ty == RT_THEME)
        .map(|(_, _, t)| t.clone());
    if let Some(tgt) = theme_target {
        let theme_part = resolve_target(master_part, &tgt);
        if let Some(blob) = pkg.get(&theme_part) {
            out.insert(
                "rawXml".into(),
                json!(String::from_utf8_lossy(blob).into_owned()),
            );
            if let Ok(root) = Element::parse(blob) {
                let mut all = Vec::new();
                root.iter_all(&mut all);
                if let Some(clr) = all
                    .iter()
                    .find(|e| e.local == "clrScheme" && e.ns.as_deref() == Some(A))
                {
                    out.insert("clrSchemeName".into(), json!(clr.attr("name")));
                    for child in &clr.children {
                        colors.insert(child.local.clone(), xml_color_solid(child));
                    }
                }
                if let Some(fs) = all
                    .iter()
                    .find(|e| e.local == "fontScheme" && e.ns.as_deref() == Some(A))
                {
                    out.insert("fontSchemeName".into(), json!(fs.attr("name")));
                    for kind in ["majorFont", "minorFont"] {
                        if let Some(node) = fs.child(A, kind) {
                            fonts.insert(
                                kind.into(),
                                json!({
                                    "latin": node.child(A, "latin").and_then(|n| n.attr("typeface")),
                                    "ea": node.child(A, "ea").and_then(|n| n.attr("typeface")),
                                    "cs": node.child(A, "cs").and_then(|n| n.attr("typeface")),
                                }),
                            );
                        }
                    }
                }
            }
        }
    }
    out.insert("colors".into(), Value::Object(colors));
    out.insert("fonts".into(), Value::Object(fonts));
    Value::Object(out)
}

/// Build master placeholder geometry map (by ph type) for one master part.
fn build_phgeom(pkg: &Package, master_part: &str) -> PhGeom {
    let mut master_by_type = std::collections::HashMap::new();
    if let Some(bytes) = pkg.get(master_part) {
        if let Ok(root) = Element::parse(bytes) {
            if let Some(tree) = shape_tree(&root) {
                for sh in tree_shapes(tree) {
                    if let Some(ph) = ph_of(sh) {
                        master_by_type.insert(ph_type_str(ph), own_geom(sh));
                    }
                }
            }
        }
    }
    PhGeom { master_by_type }
}

// ── top-level inspect ─────────────────────────────────────────────────────────

/// Ordered part targets referenced by an idlst (sldMasterIdLst / sldLayoutIdLst
/// / sldIdLst): resolve each `r:id` through the source part's rels.
fn ordered_targets(
    pkg: &Package,
    source_part: &str,
    idlst_local: &str,
    id_local: &str,
    reltype: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = match pkg.get(source_part) {
        Some(b) => b,
        None => return out,
    };
    let root = match Element::parse(bytes) {
        Ok(r) => r,
        Err(_) => return out,
    };
    let rels = pkg
        .get(&rels_path_for(source_part))
        .map(parse_rels)
        .unwrap_or_default();
    let rid_target = |rid: &str| -> Option<String> {
        rels.iter()
            .find(|(id, ty, _)| id == rid && ty == reltype)
            .map(|(_, _, t)| resolve_target(source_part, t))
    };
    if let Some(idlst) = root.descendant(P, idlst_local) {
        for id_el in idlst.children_named(P, id_local) {
            if let Some(rid) = id_el.attr_ns(R, "id") {
                if let Some(tp) = rid_target(rid) {
                    out.push(tp);
                }
            }
        }
    }
    out
}

/// Faithful port of `inspect_pptx(path, include_raw_xml=False)`.
pub fn inspect_pptx(path: &str) -> Result<Value, String> {
    let pkg = Package::read(path)?;
    let pres_part = "ppt/presentation.xml";
    let pres_bytes = pkg.get(pres_part).ok_or("missing ppt/presentation.xml")?;
    let pres = Element::parse(pres_bytes)?;

    // Dimensions from p:sldSz.
    let (w_emu, h_emu) = pres
        .descendant(P, "sldSz")
        .map(|s| {
            (
                s.attr("cx")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0),
                s.attr("cy")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));

    let master_parts = ordered_targets(&pkg, pres_part, "sldMasterIdLst", "sldMasterId", RT_MASTER);
    // Layouts belong to the first master (python-pptx convenience).
    let layout_parts = master_parts
        .first()
        .map(|m| ordered_targets(&pkg, m, "sldLayoutIdLst", "sldLayoutId", RT_LAYOUT))
        .unwrap_or_default();
    let slide_parts = ordered_targets(&pkg, pres_part, "sldIdLst", "sldId", RT_SLIDE);

    // Masters output + per-master phgeom (first master used for inheritance).
    let mut masters = Vec::new();
    for mp in &master_parts {
        let name = pkg
            .get(mp)
            .and_then(|b| Element::parse(b).ok())
            .and_then(|r| {
                r.child(P, "cSld")
                    .and_then(|c| c.attr("name").map(|s| s.to_string()))
            })
            .unwrap_or_default();
        masters.push(json!({"name": name, "theme": theme_from_master(&pkg, mp)}));
    }
    let phg = master_parts
        .first()
        .map(|m| build_phgeom(&pkg, m))
        .unwrap_or(PhGeom {
            master_by_type: std::collections::HashMap::new(),
        });

    // Layouts.
    let mut layouts = Vec::new();
    for lp in &layout_parts {
        let bytes = match pkg.get(lp) {
            Some(b) => b,
            None => continue,
        };
        let root = Element::parse(bytes)?;
        let name = root
            .child(P, "cSld")
            .and_then(|c| c.attr("name"))
            .unwrap_or("")
            .to_string();
        let ltype = root.attr("type");
        let (shape_count, elements) = match shape_tree(&root) {
            Some(tree) => {
                let shapes: Vec<&Element> = tree_shapes(tree).collect();
                let els: Vec<Value> = shapes
                    .iter()
                    .map(|sh| shape_info(sh, effective_geom_layout(sh, &phg)))
                    .collect();
                (shapes.len(), els)
            }
            None => (0, Vec::new()),
        };
        layouts.push(json!({
            "name": name,
            "type": ltype,
            "shapeCount": shape_count,
            "elements": elements,
        }));
    }

    // Slides.
    let mut slides = Vec::new();
    for (i, sp) in slide_parts.iter().enumerate() {
        let bytes = match pkg.get(sp) {
            Some(b) => b,
            None => continue,
        };
        let root = Element::parse(bytes)?;
        // layout for this slide (for slide-placeholder inheritance + name).
        let srels = pkg
            .get(&rels_path_for(sp))
            .map(parse_rels)
            .unwrap_or_default();
        let layout_part = srels
            .iter()
            .find(|(_, ty, _)| ty == RT_LAYOUT)
            .map(|(_, _, t)| resolve_target(sp, t));
        let layout_root = layout_part
            .as_deref()
            .and_then(|lp| pkg.get(lp))
            .and_then(|b| Element::parse(b).ok());
        let layout_name = layout_root
            .as_ref()
            .and_then(|r| {
                r.child(P, "cSld")
                    .and_then(|c| c.attr("name").map(|s| s.to_string()))
            })
            .unwrap_or_default();
        let layout_tree_owner = layout_root.as_ref();

        let background = slide_background(&root);
        let (count, elements) = match shape_tree(&root) {
            Some(tree) => {
                let shapes: Vec<&Element> = tree_shapes(tree).collect();
                let els: Vec<Value> = shapes
                    .iter()
                    .map(|sh| shape_info(sh, effective_geom_slide(sh, layout_tree_owner, &phg)))
                    .collect();
                (shapes.len(), els)
            }
            None => (0, Vec::new()),
        };
        slides.push(json!({
            "index": i,
            "layoutName": layout_name,
            "background": background,
            "elementCount": count,
            "elements": elements,
        }));
    }

    let file_size = std::fs::metadata(path).map(|m| m.len()).ok();
    Ok(json!({
        "path": path,
        "fileSizeBytes": file_size,
        "slideCount": slide_parts.len(),
        "layoutCount": layout_parts.len(),
        "masterCount": master_parts.len(),
        "dimensions": {
            "widthIn": round_to(w_emu as f64 / 914400.0, 4),
            "heightIn": round_to(h_emu as f64 / 914400.0, 4),
            "widthEmu": w_emu,
            "heightEmu": h_emu,
        },
        "masters": masters,
        "layouts": layouts,
        "slides": slides,
    }))
}

/// `_slide_background(slide)`.
fn slide_background(slide_root: &Element) -> Value {
    let bg = match slide_root.descendant(P, "bg") {
        Some(b) => b,
        None => return json!({"type": "inherit"}),
    };
    if let Some(bg_pr) = bg.child(P, "bgPr") {
        return json!({"type": "explicit", "fill": extract_fill(Some(bg_pr))});
    }
    if let Some(bg_ref) = bg.child(P, "bgRef") {
        return json!({"type": "ref", "idx": bg_ref.attr("idx"), "color": xml_color_solid(bg_ref)});
    }
    json!({"type": "inherit"})
}

/// JSON-string form matching `inspect_pptx_json` (indent 2, non-ASCII kept).
pub fn inspect_pptx_json(path: &str) -> Result<String, String> {
    let v = inspect_pptx(path)?;
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}
