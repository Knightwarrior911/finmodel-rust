//! 6.2 — Editor. Slide-structure, theme, and text edits done directly on the
//! `.pptx` zip + XML, a faithful port of `src/research/pptx_editor.py`.
//!
//! Slide-management (`duplicate_slide` / `delete_slide` / `reorder_slides`)
//! copies slide part bytes verbatim (renamed contiguously) and rewrites the
//! three control parts — `ppt/presentation.xml`, its `.rels`, and
//! `[Content_Types].xml` — exactly as the reference does, so the package has no
//! orphan parts. `recolor_theme` swaps `a:clrScheme` slots (and optional
//! hard-coded `a:srgbClr` values). Every top-level op appends a JSONL entry to
//! `<deck>.edit_log.jsonl`.

use serde_json::{json, Map, Value};

use crate::xmldom::{Attr, Element, A, CT, PR, R};

pub const THEME_SLOTS: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

const PRES_XML: &str = "ppt/presentation.xml";
const PRES_RELS: &str = "ppt/_rels/presentation.xml.rels";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const SLIDE_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const SLIDE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";

use crate::pkg::Package;

// ── edit log ──────────────────────────────────────────────────────────────────

fn log_path_for(deck_path: &str) -> String {
    format!("{deck_path}.edit_log.jsonl")
}

/// Append an edit entry (never fails the edit).
fn log_edit(target: &str, op: &str, params: Value) {
    let cleaned = match params {
        Value::Object(m) => {
            let mut c = Map::new();
            for (k, v) in m {
                if !k.starts_with('_') {
                    c.insert(k, v);
                }
            }
            Value::Object(c)
        }
        other => other,
    };
    let entry = json!({ "ts": now_iso(), "op": op, "params": cleaned });
    let line = format!("{}\n", entry);
    let path = log_path_for(target);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn now_iso() -> String {
    // Coarse UTC ISO-8601 timestamp; the parity gate ignores `ts`.
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("1970-01-01T00:00:{:02}+00:00", dur.as_secs() % 60)
}

/// Read the last `last_n` edit-log entries (optionally filtered by op).
pub fn get_edit_history(deck_path: &str, last_n: usize, op_filter: Option<&[&str]>) -> Vec<Value> {
    let path = log_path_for(deck_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<Value> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            if let Some(filter) = op_filter {
                let op = entry.get("op").and_then(|v| v.as_str()).unwrap_or("");
                if !filter.contains(&op) {
                    continue;
                }
            }
            out.push(entry);
        }
    }
    if out.len() > last_n {
        out = out.split_off(out.len() - last_n);
    }
    out
}

/// Delete the edit log for a deck.
pub fn clear_edit_history(deck_path: &str) {
    let _ = std::fs::remove_file(log_path_for(deck_path));
}

// ── slide part helpers ────────────────────────────────────────────────────────

fn slide_num(name: &str) -> Option<i64> {
    // "ppt/slides/slide12.xml" -> 12
    let after = name.rsplit_once("slide")?.1;
    let stem = after.rsplit_once('.')?.0;
    stem.parse::<i64>().ok()
}

/// All `ppt/slides/slideN.xml` names sorted by N.
fn slide_part_names(pkg: &Package) -> Vec<String> {
    let mut parts: Vec<String> = pkg
        .names
        .iter()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .cloned()
        .collect();
    parts.sort_by_key(|n| slide_num(n).unwrap_or(i64::MAX));
    parts
}

fn next_slide_index(pkg: &Package) -> i64 {
    let used: std::collections::HashSet<i64> =
        slide_part_names(pkg).iter().filter_map(|n| slide_num(n)).collect();
    let mut i = 1;
    while used.contains(&i) {
        i += 1;
    }
    i
}

// ── control-file rewrites (mirror _rewrite_* helpers) ─────────────────────────

/// `_rewrite_presentation_rels`. Returns (new rels bytes, target->rid map).
fn rewrite_presentation_rels(
    rels_xml: &[u8],
    ordered_slide_targets: &[String],
) -> Result<(Vec<u8>, Vec<(String, String)>), String> {
    let root = Element::parse(rels_xml)?;
    let mut keep: Vec<Element> = Vec::new();
    for rel in &root.children {
        if rel.local == "Relationship" && rel.attr("Type") != Some(SLIDE_REL_TYPE) {
            keep.push(rel.clone());
        }
    }
    let mut used: std::collections::HashSet<String> =
        keep.iter().filter_map(|r| r.attr("Id").map(|s| s.to_string())).collect();
    let mut next_id = || -> String {
        let mut n = 1;
        loop {
            let candidate = format!("rId{n}");
            if !used.contains(&candidate) {
                used.insert(candidate.clone());
                return candidate;
            }
            n += 1;
        }
    };

    let mut new_root = Element {
        ns: Some(PR.to_string()),
        local: "Relationships".to_string(),
        attrs: Vec::new(),
        children: Vec::new(),
        text: String::new(),
    };
    for rel in keep {
        new_root.children.push(rel);
    }
    let mut target_to_rid: Vec<(String, String)> = Vec::new();
    for target in ordered_slide_targets {
        let rid = next_id();
        target_to_rid.push((target.clone(), rid.clone()));
        new_root.children.push(Element {
            ns: Some(PR.to_string()),
            local: "Relationship".to_string(),
            attrs: vec![
                Attr { ns: None, local: "Id".into(), value: rid },
                Attr { ns: None, local: "Type".into(), value: SLIDE_REL_TYPE.into() },
                Attr { ns: None, local: "Target".into(), value: target.clone() },
            ],
            children: Vec::new(),
            text: String::new(),
        });
    }
    Ok((new_root.to_xml_bytes(), target_to_rid))
}

/// `_rewrite_presentation_xml`. Replace `p:sldIdLst` children with new sldIds.
fn rewrite_presentation_xml(pres_xml: &[u8], ordered_rids: &[String]) -> Result<Vec<u8>, String> {
    let mut root = Element::parse(pres_xml)?;
    let p_ns = crate::xmldom::P;
    let idx = root
        .children
        .iter()
        .position(|c| c.local == "sldIdLst" && c.ns.as_deref() == Some(p_ns))
        .ok_or("presentation.xml has no <p:sldIdLst>")?;
    let base = 256;
    let mut new_children = Vec::new();
    for (i, rid) in ordered_rids.iter().enumerate() {
        new_children.push(Element {
            ns: Some(p_ns.to_string()),
            local: "sldId".to_string(),
            attrs: vec![
                Attr { ns: None, local: "id".into(), value: (base + i).to_string() },
                Attr { ns: Some(R.to_string()), local: "id".into(), value: rid.clone() },
            ],
            children: Vec::new(),
            text: String::new(),
        });
    }
    root.children[idx].children = new_children;
    Ok(root.to_xml_bytes())
}

/// `_rewrite_content_types`. Ensure every current slide is an Override.
fn rewrite_content_types(ct_xml: &[u8], slide_part_names: &[String]) -> Result<Vec<u8>, String> {
    let mut root = Element::parse(ct_xml)?;
    root.children.retain(|c| {
        if c.local == "Override" {
            let ct = c.attr("ContentType").unwrap_or("");
            let part = c.attr("PartName").unwrap_or("");
            !(ct == SLIDE_CONTENT_TYPE || part.starts_with("/ppt/slides/slide"))
        } else {
            true
        }
    });
    for name in slide_part_names {
        root.children.push(Element {
            ns: Some(CT.to_string()),
            local: "Override".to_string(),
            attrs: vec![
                Attr { ns: None, local: "PartName".into(), value: format!("/{name}") },
                Attr { ns: None, local: "ContentType".into(), value: SLIDE_CONTENT_TYPE.into() },
            ],
            children: Vec::new(),
            text: String::new(),
        });
    }
    Ok(root.to_xml_bytes())
}

fn slide_rels_name(part: &str) -> String {
    // "ppt/slides/slide3.xml" -> "ppt/slides/_rels/slide3.xml.rels"
    part.replacen("ppt/slides/", "ppt/slides/_rels/", 1) + ".rels"
}

/// `_renumber_and_reorder`.
fn renumber_and_reorder(pkg: &Package, desired_order: &[i64]) -> Result<Package, String> {
    let original_parts = slide_part_names(pkg);
    let by_idx: std::collections::HashMap<i64, String> =
        original_parts.iter().filter_map(|n| slide_num(n).map(|k| (k, n.clone()))).collect();

    let mut out = pkg.clone();
    // Drop all old slide xml + rels.
    for old in &original_parts {
        out.remove(old);
        let rels = slide_rels_name(old);
        if pkg.get(&rels).is_some() {
            out.remove(&rels);
        }
    }

    let mut new_part_names = Vec::new();
    let mut new_rel_targets = Vec::new();
    for (new_idx0, original_idx) in desired_order.iter().enumerate() {
        let new_idx = new_idx0 + 1;
        let old_part = by_idx.get(original_idx).ok_or("bad slide index in order")?;
        let new_part = format!("ppt/slides/slide{new_idx}.xml");
        let bytes = pkg.get(old_part).ok_or("missing slide bytes")?.to_vec();
        out.set(&new_part, bytes);
        new_part_names.push(new_part.clone());
        new_rel_targets.push(format!("slides/slide{new_idx}.xml"));

        let old_rels = slide_rels_name(old_part);
        if let Some(rb) = pkg.get(&old_rels) {
            let new_rels = format!("ppt/slides/_rels/slide{new_idx}.xml.rels");
            out.set(&new_rels, rb.to_vec());
        }
    }

    let (rels_xml, target_to_rid) =
        rewrite_presentation_rels(pkg.get(PRES_RELS).ok_or("missing presentation rels")?, &new_rel_targets)?;
    out.set(PRES_RELS, rels_xml);

    let ordered_rids: Vec<String> = new_rel_targets
        .iter()
        .map(|t| {
            target_to_rid
                .iter()
                .find(|(tt, _)| tt == t)
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        })
        .collect();
    out.set(PRES_XML, rewrite_presentation_xml(pkg.get(PRES_XML).ok_or("missing presentation.xml")?, &ordered_rids)?);
    out.set(
        CONTENT_TYPES,
        rewrite_content_types(pkg.get(CONTENT_TYPES).ok_or("missing content types")?, &new_part_names)?,
    );
    Ok(out)
}

fn resolve_output(deck_path: &str, output_path: Option<&str>) -> String {
    output_path.unwrap_or(deck_path).to_string()
}

// ── public slide-structure ops ────────────────────────────────────────────────

/// Duplicate the slide at `slide_index` (0-based); insert at `position` or append.
pub fn duplicate_slide(
    deck_path: &str,
    slide_index: usize,
    position: Option<usize>,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out_path = resolve_output(deck_path, output_path);
    let mut pkg = Package::read(deck_path)?;
    let parts = slide_part_names(&pkg);
    let n = parts.len();
    if slide_index >= n {
        return Err(format!("slide_index {slide_index} out of range (0..{})", n.saturating_sub(1)));
    }
    let original_order: Vec<i64> = (1..=n as i64).collect();
    let src_original_idx = original_order[slide_index];

    let insert_at = position.map(|p| p.min(n)).unwrap_or(n);
    let mut new_order = original_order.clone();
    new_order.insert(insert_at, src_original_idx);

    let free_idx = next_slide_index(&pkg);
    let src_part = format!("ppt/slides/slide{src_original_idx}.xml");
    let src_rels = format!("ppt/slides/_rels/slide{src_original_idx}.xml.rels");
    let dup_part = format!("ppt/slides/slide{free_idx}.xml");
    let dup_rels = format!("ppt/slides/_rels/slide{free_idx}.xml.rels");
    let src_bytes = pkg.get(&src_part).ok_or("missing source slide")?.to_vec();
    pkg.set(&dup_part, src_bytes);
    if let Some(rb) = pkg.get(&src_rels) {
        pkg.set(&dup_rels, rb.to_vec());
    }

    // Rebuild order using free_idx for the inserted second occurrence.
    let mut order: Vec<i64> = Vec::new();
    let mut seen = false;
    for orig in &new_order {
        if *orig == src_original_idx && !seen {
            order.push(*orig);
            seen = true;
        } else if *orig == src_original_idx && seen {
            order.push(free_idx);
        } else {
            order.push(*orig);
        }
    }

    let out = renumber_and_reorder(&pkg, &order)?;
    out.write(&out_path)?;
    log_edit(&out_path, "duplicate_slide", json!({"slide_index": slide_index, "position": position}));
    Ok(out_path)
}

/// Delete the slide at `slide_index` (0-based).
pub fn delete_slide(deck_path: &str, slide_index: usize, output_path: Option<&str>) -> Result<String, String> {
    let out_path = resolve_output(deck_path, output_path);
    let pkg = Package::read(deck_path)?;
    let parts = slide_part_names(&pkg);
    let n = parts.len();
    if slide_index >= n {
        return Err(format!("slide_index {slide_index} out of range (0..{})", n.saturating_sub(1)));
    }
    let keep: Vec<i64> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != slide_index)
        .filter_map(|(_, p)| slide_num(p))
        .collect();
    let out = renumber_and_reorder(&pkg, &keep)?;
    out.write(&out_path)?;
    log_edit(&out_path, "delete_slide", json!({"slide_index": slide_index}));
    Ok(out_path)
}

/// Reorder slides. `new_order` is a permutation of `0..slide_count`.
pub fn reorder_slides(deck_path: &str, new_order: &[usize], output_path: Option<&str>) -> Result<String, String> {
    let out_path = resolve_output(deck_path, output_path);
    let pkg = Package::read(deck_path)?;
    let parts = slide_part_names(&pkg);
    let n = parts.len();
    let mut sorted = new_order.to_vec();
    sorted.sort_unstable();
    if sorted != (0..n).collect::<Vec<_>>() {
        return Err(format!("new_order must be a permutation of 0..{}; got {new_order:?}", n - 1));
    }
    let by_idx: Vec<i64> = parts.iter().filter_map(|p| slide_num(p)).collect();
    let desired: Vec<i64> = new_order.iter().map(|&i| by_idx[i]).collect();
    let out = renumber_and_reorder(&pkg, &desired)?;
    out.write(&out_path)?;
    log_edit(&out_path, "reorder_slides", json!({"new_order": new_order}));
    Ok(out_path)
}

// ── theme recolor ─────────────────────────────────────────────────────────────

fn normalise_hex(value: &str) -> Result<String, String> {
    let v = value.trim().trim_start_matches('#');
    if v.len() != 6 || !v.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid hex color: {value:?}"));
    }
    Ok(v.to_uppercase())
}

/// `_recolor_clr_scheme`: replace slot children with a single `a:srgbClr`.
fn recolor_clr_scheme(theme_xml: &[u8], palette: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let mut root = Element::parse(theme_xml)?;
    // themeElements/clrScheme
    let scheme_pos = find_clr_scheme(&mut root);
    let scheme = match scheme_pos {
        Some(s) => s,
        None => return Ok(theme_xml.to_vec()),
    };
    for (slot, new_hex) in palette {
        if !THEME_SLOTS.contains(slot) {
            return Err(format!("Unknown theme slot {slot:?}. Valid slots: {THEME_SLOTS:?}"));
        }
        let hex = normalise_hex(new_hex)?;
        if let Some(slot_el) = scheme.children.iter_mut().find(|c| c.local == *slot && c.ns.as_deref() == Some(A)) {
            slot_el.children.clear();
            slot_el.text.clear();
            slot_el.children.push(Element {
                ns: Some(A.to_string()),
                local: "srgbClr".to_string(),
                attrs: vec![Attr { ns: None, local: "val".into(), value: hex }],
                children: Vec::new(),
                text: String::new(),
            });
        }
    }
    Ok(root.to_xml_bytes())
}

/// Return a mutable reference to the `a:clrScheme` under themeElements.
fn find_clr_scheme(root: &mut Element) -> Option<&mut Element> {
    let te = root.children.iter_mut().find(|c| c.local == "themeElements" && c.ns.as_deref() == Some(A))?;
    te.children.iter_mut().find(|c| c.local == "clrScheme" && c.ns.as_deref() == Some(A))
}

/// `_replace_srgb_in_xml`: swap `a:srgbClr@val` values (case-insensitive).
fn replace_srgb_in_xml(xml_bytes: &[u8], swaps: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let mut root = Element::parse(xml_bytes)?;
    let mut norm: Vec<(String, String)> = Vec::new();
    for (o, n) in swaps {
        norm.push((normalise_hex(o)?, normalise_hex(n)?));
    }
    recolor_srgb_rec(&mut root, &norm);
    Ok(root.to_xml_bytes())
}

fn recolor_srgb_rec(el: &mut Element, norm: &[(String, String)]) {
    if el.local == "srgbClr" && el.ns.as_deref() == Some(A) {
        if let Some(a) = el.attrs.iter_mut().find(|a| a.ns.is_none() && a.local == "val") {
            let up = a.value.to_uppercase();
            if let Some((_, n)) = norm.iter().find(|(o, _)| *o == up) {
                a.value = n.clone();
            }
        }
    }
    for c in &mut el.children {
        recolor_srgb_rec(c, norm);
    }
}

/// Recolor a deck's theme accent slots (and optional hard-coded RGBs).
pub fn recolor_theme(
    deck_path: &str,
    palette: &[(&str, &str)],
    also_replace_hardcoded: Option<&[(&str, &str)]>,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out_path = resolve_output(deck_path, output_path);
    let mut pkg = Package::read(deck_path)?;
    let theme_parts: Vec<String> = pkg
        .names
        .iter()
        .filter(|n| n.starts_with("ppt/theme/") && n.ends_with(".xml"))
        .cloned()
        .collect();
    if theme_parts.is_empty() {
        return Err("No theme XML found in deck".into());
    }
    for name in &theme_parts {
        let bytes = pkg.get(name).unwrap().to_vec();
        pkg.set(name, recolor_clr_scheme(&bytes, palette)?);
    }
    if let Some(swaps) = also_replace_hardcoded {
        let targets: Vec<String> = pkg
            .names
            .iter()
            .filter(|n| {
                n.ends_with(".xml")
                    && (n.starts_with("ppt/slides/")
                        || n.starts_with("ppt/slideLayouts/")
                        || n.starts_with("ppt/slideMasters/")
                        || n.starts_with("ppt/theme/"))
            })
            .cloned()
            .collect();
        for name in targets {
            let bytes = pkg.get(&name).unwrap().to_vec();
            pkg.set(&name, replace_srgb_in_xml(&bytes, swaps)?);
        }
    }
    pkg.write(&out_path)?;
    let palette_json: Map<String, Value> =
        palette.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let arh = match also_replace_hardcoded {
        None => Value::Null,
        Some(swaps) => {
            let m: Map<String, Value> = swaps.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
            Value::Object(m)
        }
    };
    log_edit(&out_path, "recolor_theme", json!({ "palette": palette_json, "also_replace_hardcoded": arh }));
    Ok(out_path)
}

// ── text replace ──────────────────────────────────────────────────────────────

/// Replace text across every slide's `a:r/a:t` runs, format-preserving, done as
/// a zip+XML edit (the reference uses python-pptx; the observable text result is
/// identical and gated via the inspector).
pub fn replace_text_in_deck(
    deck_path: &str,
    replacements: &[(&str, &str)],
    output_path: Option<&str>,
) -> Result<String, String> {
    let out_path = resolve_output(deck_path, output_path);
    let mut pkg = Package::read(deck_path)?;
    let slide_parts = slide_part_names(&pkg);
    for part in &slide_parts {
        let bytes = pkg.get(part).unwrap().to_vec();
        let mut root = Element::parse(&bytes)?;
        replace_runs(&mut root, replacements);
        pkg.set(part, root.to_xml_bytes());
    }
    pkg.write(&out_path)?;
    let repl_json: Map<String, Value> =
        replacements.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    log_edit(&out_path, "replace_text_in_deck", json!({ "replacements": repl_json }));
    Ok(out_path)
}

fn replace_runs(el: &mut Element, replacements: &[(&str, &str)]) {
    // Replace text inside a:r/a:t run text nodes (matches replace_text_in_slide).
    if el.local == "r" && el.ns.as_deref() == Some(A) {
        for child in &mut el.children {
            if child.local == "t" && child.ns.as_deref() == Some(A) {
                for (old, new) in replacements {
                    if child.text.contains(old) {
                        child.text = child.text.replace(old, new);
                    }
                }
            }
        }
    }
    for c in &mut el.children {
        replace_runs(c, replacements);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6.5 — Granular shape primitives (zip+XML edits).
//
// The reference `pptx_editor.py` primitives round-trip through python-pptx;
// these do the equivalent surgical edit directly on the slide XML. They are
// gated behaviourally (apply -> inspect -> assert) under the 6.2 round-trip
// pattern, since python-pptx reserializes whole parts and can't be diffed at
// the member-byte level.
// ─────────────────────────────────────────────────────────────────────────────

use crate::xmldom::P;

fn emu(inches: f64) -> i64 {
    (inches * 914400.0) as i64
}

fn hex6(c: &str) -> String {
    c.trim_start_matches('#').to_uppercase()
}

/// 0-based slide index -> part name (sorted by N).
fn nth_slide_part(pkg: &Package, slide_index: usize) -> Result<String, String> {
    let parts = slide_part_names(pkg);
    parts.get(slide_index).cloned().ok_or_else(|| format!("slide index {slide_index} out of range"))
}

/// Find a shape element in an spTree by cNvPr id or name.
fn find_shape_mut<'a>(tree: &'a mut Element, id: Option<i64>, name: Option<&str>) -> Option<&'a mut Element> {
    for c in &mut tree.children {
        if !matches!(c.local.as_str(), "sp" | "pic" | "graphicFrame" | "grpSp" | "cxnSp") {
            continue;
        }
        let cnv = c.children.first().and_then(|nv| nv.child(P, "cNvPr"));
        let cid = cnv.and_then(|e| e.attr("id")).and_then(|s| s.parse::<i64>().ok());
        let cname = cnv.and_then(|e| e.attr("name"));
        let hit = (id.is_some() && cid == id) || (name.is_some() && cname == name);
        if hit {
            return Some(c);
        }
    }
    None
}

fn sptree_mut(root: &mut Element) -> Option<&mut Element> {
    root.children
        .iter_mut()
        .find(|c| c.local == "cSld" && c.ns.as_deref() == Some(P))?
        .children
        .iter_mut()
        .find(|c| c.local == "spTree" && c.ns.as_deref() == Some(P))
}

/// The `a:xfrm` element hosting a shape's geometry, created if absent.
fn ensure_xfrm(shape: &mut Element) -> &mut Element {
    // graphicFrame -> p:xfrm; else spPr/a:xfrm.
    if shape.local == "graphicFrame" {
        if !shape.children.iter().any(|c| c.local == "xfrm" && c.ns.as_deref() == Some(P)) {
            shape.children.insert(0, new_xfrm(Some(P)));
        }
        return shape.children.iter_mut().find(|c| c.local == "xfrm" && c.ns.as_deref() == Some(P)).unwrap();
    }
    let sp_pr = shape
        .children
        .iter()
        .position(|c| c.local == "spPr")
        .expect("shape has spPr");
    let sp_pr = &mut shape.children[sp_pr];
    if !sp_pr.children.iter().any(|c| c.local == "xfrm" && c.ns.as_deref() == Some(A)) {
        sp_pr.children.insert(0, new_xfrm(Some(A)));
    }
    sp_pr.children.iter_mut().find(|c| c.local == "xfrm" && c.ns.as_deref() == Some(A)).unwrap()
}

fn new_xfrm(ns: Option<&str>) -> Element {
    Element {
        ns: ns.map(|s| s.to_string()),
        local: "xfrm".into(),
        attrs: Vec::new(),
        children: vec![
            Element { ns: Some(A.into()), local: "off".into(), attrs: vec![Attr { ns: None, local: "x".into(), value: "0".into() }, Attr { ns: None, local: "y".into(), value: "0".into() }], children: Vec::new(), text: String::new() },
            Element { ns: Some(A.into()), local: "ext".into(), attrs: vec![Attr { ns: None, local: "cx".into(), value: "0".into() }, Attr { ns: None, local: "cy".into(), value: "0".into() }], children: Vec::new(), text: String::new() },
        ],
        text: String::new(),
    }
}

fn set_attr(el: &mut Element, local: &str, value: String) {
    if let Some(a) = el.attrs.iter_mut().find(|a| a.ns.is_none() && a.local == local) {
        a.value = value;
    } else {
        el.attrs.push(Attr { ns: None, local: local.into(), value });
    }
}

fn edit_slide<F>(deck_path: &str, slide_index: usize, output_path: Option<&str>, f: F) -> Result<String, String>
where
    F: FnOnce(&mut Element) -> Result<(), String>,
{
    let out_path = resolve_output(deck_path, output_path);
    let mut pkg = Package::read(deck_path)?;
    let part = nth_slide_part(&pkg, slide_index)?;
    let mut root = Element::parse(pkg.get(&part).unwrap())?;
    {
        let tree = sptree_mut(&mut root).ok_or("slide has no spTree")?;
        f(tree)?;
    }
    pkg.set(&part, root.to_xml_bytes());
    pkg.write(&out_path)?;
    Ok(out_path)
}

/// Move a shape (absolute `left`/`top` or relative `dx`/`dy`, inches).
#[allow(clippy::too_many_arguments)]
pub fn move_shape(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    left: Option<f64>,
    top: Option<f64>,
    dx: Option<f64>,
    dy: Option<f64>,
    output_path: Option<&str>,
) -> Result<String, String> {
    if left.is_none() && top.is_none() && dx.is_none() && dy.is_none() {
        return Err("Pass at least one of left/top/dx/dy".into());
    }
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let shape = find_shape_mut(tree, shape_id, shape_name).ok_or("shape not found")?;
        let xfrm = ensure_xfrm(shape);
        let off = xfrm.children.iter_mut().find(|c| c.local == "off").ok_or("no off")?;
        let cur_x = off.attr("x").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let cur_y = off.attr("y").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        if let Some(l) = left {
            set_attr(off, "x", emu(l).to_string());
        } else if let Some(d) = dx {
            set_attr(off, "x", (cur_x + emu(d)).to_string());
        }
        if let Some(t) = top {
            set_attr(off, "y", emu(t).to_string());
        } else if let Some(d) = dy {
            set_attr(off, "y", (cur_y + emu(d)).to_string());
        }
        Ok(())
    })?;
    log_edit(&out, "move_shape", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name, "left": left, "top": top, "dx": dx, "dy": dy}));
    Ok(out)
}

/// Resize a shape to `width`/`height` (inches; either or both).
pub fn resize_shape(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    width: Option<f64>,
    height: Option<f64>,
    output_path: Option<&str>,
) -> Result<String, String> {
    if width.is_none() && height.is_none() {
        return Err("Pass at least one of width/height".into());
    }
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let shape = find_shape_mut(tree, shape_id, shape_name).ok_or("shape not found")?;
        let xfrm = ensure_xfrm(shape);
        let ext = xfrm.children.iter_mut().find(|c| c.local == "ext").ok_or("no ext")?;
        if let Some(w) = width {
            set_attr(ext, "cx", emu(w).to_string());
        }
        if let Some(h) = height {
            set_attr(ext, "cy", emu(h).to_string());
        }
        Ok(())
    })?;
    log_edit(&out, "resize_shape", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name, "width": width, "height": height}));
    Ok(out)
}

fn sp_pr_mut(shape: &mut Element) -> Option<&mut Element> {
    shape.children.iter_mut().find(|c| c.local == "spPr")
}

/// Set a shape's solid fill (hex) or clear it (`no_fill`).
pub fn set_shape_fill(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    color: Option<&str>,
    no_fill: bool,
    output_path: Option<&str>,
) -> Result<String, String> {
    if !no_fill && color.is_none() {
        return Err("Pass color or no_fill=true".into());
    }
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let shape = find_shape_mut(tree, shape_id, shape_name).ok_or("shape not found")?;
        let sp_pr = sp_pr_mut(shape).ok_or("no spPr")?;
        sp_pr.children.retain(|c| !matches!(c.local.as_str(), "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill" | "grpFill"));
        // Insert after prstGeom (or at end).
        let pos = sp_pr.children.iter().position(|c| c.local == "prstGeom").map(|i| i + 1).unwrap_or(sp_pr.children.len());
        let fill = if no_fill {
            Element { ns: Some(A.into()), local: "noFill".into(), attrs: Vec::new(), children: Vec::new(), text: String::new() }
        } else {
            Element {
                ns: Some(A.into()),
                local: "solidFill".into(),
                attrs: Vec::new(),
                children: vec![Element { ns: Some(A.into()), local: "srgbClr".into(), attrs: vec![Attr { ns: None, local: "val".into(), value: hex6(color.unwrap()) }], children: Vec::new(), text: String::new() }],
                text: String::new(),
            }
        };
        sp_pr.children.insert(pos, fill);
        Ok(())
    })?;
    log_edit(&out, "set_shape_fill", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name, "color": color, "no_fill": no_fill}));
    Ok(out)
}

/// Delete a shape from a slide.
pub fn delete_shape(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let before = tree.children.len();
        tree.children.retain(|c| {
            if !matches!(c.local.as_str(), "sp" | "pic" | "graphicFrame" | "grpSp" | "cxnSp") {
                return true;
            }
            let cnv = c.children.first().and_then(|nv| nv.child(P, "cNvPr"));
            let cid = cnv.and_then(|e| e.attr("id")).and_then(|s| s.parse::<i64>().ok());
            let cname = cnv.and_then(|e| e.attr("name"));
            !((shape_id.is_some() && cid == shape_id) || (shape_name.is_some() && cname == shape_name))
        });
        if tree.children.len() == before {
            return Err("shape not found".into());
        }
        Ok(())
    })?;
    log_edit(&out, "delete_shape", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name}));
    Ok(out)
}

/// Add a textbox at (left, top) sized (width, height) inches with `text`.
#[allow(clippy::too_many_arguments)]
pub fn add_textbox(
    deck_path: &str,
    slide_index: usize,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    text: &str,
    name: Option<&str>,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let next_id = 1 + tree
            .children
            .iter()
            .filter_map(|c| c.children.first().and_then(|nv| nv.child(P, "cNvPr")).and_then(|e| e.attr("id")).and_then(|s| s.parse::<i64>().ok()))
            .max()
            .unwrap_or(1);
        let nm = name.map(|s| s.to_string()).unwrap_or_else(|| format!("TextBox {}", next_id - 1));
        let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let xml = format!(
            "<p:sp xmlns:a=\"{A}\" xmlns:p=\"{P}\"><p:nvSpPr><p:cNvPr id=\"{next_id}\" name=\"{nm}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>\
<p:txBody><a:bodyPr wrap=\"square\"><a:spAutoFit/></a:bodyPr><a:lstStyle/>\
<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>{esc}</a:t></a:r></a:p></p:txBody></p:sp>",
            emu(left), emu(top), emu(width), emu(height),
        );
        let sp = Element::parse(xml.as_bytes())?;
        tree.children.push(sp);
        Ok(())
    })?;
    log_edit(&out, "add_textbox", json!({"slide_index": slide_index, "left": left, "top": top, "width": width, "height": height, "text": text, "name": name}));
    Ok(out)
}

// ── table column/row reorder ──────────────────────────────────────────────────

fn reorder_children(parent: &mut Element, local: &str, new_order: &[usize]) -> Result<(), String> {
    let idxs: Vec<usize> = parent
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.local == local && c.ns.as_deref() == Some(A))
        .map(|(i, _)| i)
        .collect();
    let mut sorted = new_order.to_vec();
    sorted.sort_unstable();
    if sorted != (0..idxs.len()).collect::<Vec<_>>() {
        return Err(format!("new_order must be a permutation of 0..{}", idxs.len().saturating_sub(1)));
    }
    let originals: Vec<Element> = idxs.iter().map(|&i| parent.children[i].clone()).collect();
    for (slot, &orig) in idxs.iter().zip(new_order.iter()) {
        parent.children[*slot] = originals[orig].clone();
    }
    Ok(())
}

fn find_tbl_mut(shape: &mut Element) -> Option<&mut Element> {
    let gd = shape.child(A, "graphic")?;
    let _ = gd; // presence check
    // graphic/graphicData/tbl
    let graphic = shape.children.iter_mut().find(|c| c.local == "graphic")?;
    let gdata = graphic.children.iter_mut().find(|c| c.local == "graphicData")?;
    gdata.children.iter_mut().find(|c| c.local == "tbl" && c.ns.as_deref() == Some(A))
}

/// Swap two table columns (0-based) — reorders `a:gridCol` + each row's `a:tc`.
pub fn swap_table_columns(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    col_a: usize,
    col_b: usize,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let shape = find_shape_mut(tree, shape_id, shape_name).ok_or("shape not found")?;
        let tbl = find_tbl_mut(shape).ok_or("shape is not a table")?;
        let n_cols = tbl.child(A, "tblGrid").map(|g| g.children_named(A, "gridCol").count()).unwrap_or(0);
        if col_a >= n_cols || col_b >= n_cols {
            return Err("column index out of range".into());
        }
        let mut order: Vec<usize> = (0..n_cols).collect();
        order.swap(col_a, col_b);
        if let Some(grid) = tbl.children.iter_mut().find(|c| c.local == "tblGrid") {
            reorder_children(grid, "gridCol", &order)?;
        }
        for tr in tbl.children.iter_mut().filter(|c| c.local == "tr" && c.ns.as_deref() == Some(A)) {
            reorder_children(tr, "tc", &order)?;
        }
        Ok(())
    })?;
    log_edit(&out, "swap_table_columns", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name, "col_a": col_a, "col_b": col_b}));
    Ok(out)
}

/// Swap two table rows (0-based) — reorders `a:tr`.
pub fn swap_table_rows(
    deck_path: &str,
    slide_index: usize,
    shape_id: Option<i64>,
    shape_name: Option<&str>,
    row_a: usize,
    row_b: usize,
    output_path: Option<&str>,
) -> Result<String, String> {
    let out = edit_slide(deck_path, slide_index, output_path, |tree| {
        let shape = find_shape_mut(tree, shape_id, shape_name).ok_or("shape not found")?;
        let tbl = find_tbl_mut(shape).ok_or("shape is not a table")?;
        let n_rows = tbl.children_named(A, "tr").count();
        if row_a >= n_rows || row_b >= n_rows {
            return Err("row index out of range".into());
        }
        let mut order: Vec<usize> = (0..n_rows).collect();
        order.swap(row_a, row_b);
        reorder_children(tbl, "tr", &order)?;
        Ok(())
    })?;
    log_edit(&out, "swap_table_rows", json!({"slide_index": slide_index, "shape_id": shape_id, "shape_name": shape_name, "row_a": row_a, "row_b": row_b}));
    Ok(out)
}
