//! Business-segment revenue from the XBRL instance document.
//!
//! EDGAR's companyfacts API carries NO dimensional facts, so segment revenue
//! (Automotive vs Energy, Products vs Services…) only exists in the filing's
//! XBRL instance XML. This module fetches the latest 10-K/20-F instance and
//! extracts per-segment revenue with a deliberately conservative parser:
//!
//! - Only contexts with EXACTLY ONE dimension — the business-segments axis —
//!   are used. Filers double-tag facts along segment+product or segment+geo
//!   axes; including those double-counts revenue.
//! - Eliminations / intersegment members are kept but LABELED — an analyst
//!   reconciling to the consolidated total needs them visible, not silently
//!   dropped.
//! - The revenue concept list mirrors the annual-spread extractor's order:
//!   the first concept with any segment facts for the latest period wins
//!   (never mixed across concepts, which would also double-count).

use crate::market::FetchError;

/// One segment's revenue for a period.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SegmentFact {
    /// Human label derived from the member name ("Automotive", "Energy
    /// Generation and Storage", "Intersegment eliminations").
    pub segment: String,
    /// Raw member QName for provenance (e.g. "tsla:AutomotiveSegmentMember").
    pub member: String,
    pub value: f64,
    /// Period end (ISO date).
    pub period_end: String,
    /// True for eliminations / intersegment members (reconciling items).
    pub eliminations: bool,
}

const SEGMENT_AXIS: &str = "StatementBusinessSegmentsAxis";

/// Revenue concepts in the same precedence order as the annual spread.
const REVENUE_CONCEPTS: &[&str] = &[
    "us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax",
    "us-gaap:RevenueFromContractWithCustomerIncludingAssessedTax",
    "us-gaap:Revenues",
    "us-gaap:SalesRevenueNet",
];

/// Fetch per-segment revenue for the latest annual filing of `cik`.
/// Returns segments for the most recent period found, sorted by value
/// descending with eliminations last. Empty when the filer reports a single
/// segment (or tags none).
pub fn fetch_segment_revenue(cik: &str) -> Result<Vec<SegmentFact>, FetchError> {
    let filings = crate::edgar::search_filings(cik, &["10-K", "20-F"], 1)
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let filing = filings
        .into_iter()
        .next()
        .ok_or_else(|| FetchError::Network("no annual filing".into()))?;
    let instance_url = instance_url_for(&filing.url, &filing.accession_number, cik)?;
    let xml = crate::edgar::fetch_url_text(&instance_url)?;
    Ok(parse_segment_revenue(&xml))
}

/// The instance document lives beside the primary doc as `<primary>_htm.xml`.
fn instance_url_for(primary_url: &str, _accession: &str, _cik: &str) -> Result<String, FetchError> {
    let base = primary_url
        .strip_suffix(".htm")
        .or_else(|| primary_url.strip_suffix(".html"))
        .ok_or_else(|| FetchError::Network("unexpected primary doc url".into()))?;
    Ok(format!("{base}_htm.xml"))
}

/// Pure instance-XML parser (unit-tested against a fixture).
pub fn parse_segment_revenue(xml: &str) -> Vec<SegmentFact> {
    // 1. Contexts whose segment container holds EXACTLY ONE explicitMember,
    //    on the business-segments axis: context id -> (member, period_end).
    let mut ctx: std::collections::HashMap<&str, (String, String)> =
        std::collections::HashMap::new();
    for block in split_blocks(xml, "context") {
        let Some(id) = attr_of(block.open, "id") else { continue };
        // Element blocks, not substring hits — a substring count would also
        // match the CLOSING tag and double-count every member.
        let members = split_blocks(block.body, "explicitMember");
        if members.len() != 1 {
            continue; // zero dims = consolidated; 2+ dims = double-tagged
        }
        let m = members[0].open;
        let Some(dim) = attr_of(m, "dimension") else { continue };
        if !dim.contains(SEGMENT_AXIS) {
            continue;
        }
        let member = text_of(m).trim().to_string();
        if member.is_empty() {
            continue;
        }
        let period_end = tag_text(block.body, "endDate")
            .or_else(|| tag_text(block.body, "instant"))
            .unwrap_or_default();
        ctx.insert(id, (member, period_end));
    }
    if ctx.is_empty() {
        return Vec::new();
    }

    // 2. First revenue concept with any segment facts wins (no mixing).
    for concept in REVENUE_CONCEPTS {
        let tag = concept.split(':').nth(1).unwrap_or(concept);
        let mut facts: Vec<SegmentFact> = Vec::new();
        for block in split_blocks(xml, tag) {
            let Some(cref) = attr_of(block.open, "contextRef") else { continue };
            let Some((member, period_end)) = ctx.get(cref) else { continue };
            let Ok(value) = text_of(block.open).trim().parse::<f64>() else { continue };
            let lower = member.to_lowercase();
            let eliminations =
                lower.contains("elimination") || lower.contains("intersegment");
            facts.push(SegmentFact {
                segment: humanize_member(member),
                member: member.clone(),
                value,
                period_end: period_end.clone(),
                eliminations,
            });
        }
        if facts.is_empty() {
            continue;
        }
        // Latest period only; biggest first; eliminations last.
        let latest = facts
            .iter()
            .map(|f| f.period_end.clone())
            .max()
            .unwrap_or_default();
        facts.retain(|f| f.period_end == latest);
        // Same member can be tagged in several units/decimals blocks — dedupe.
        facts.sort_by(|a, b| a.member.cmp(&b.member));
        facts.dedup_by(|a, b| a.member == b.member && a.value == b.value);
        facts.sort_by(|a, b| {
            a.eliminations
                .cmp(&b.eliminations)
                .then(b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal))
        });
        return facts;
    }
    Vec::new()
}

/// "tsla:AutomotiveSegmentMember" → "Automotive";
/// "us-gaap:IntersegmentEliminationMember" → "Intersegment Elimination".
fn humanize_member(qname: &str) -> String {
    let local = qname.split(':').next_back().unwrap_or(qname);
    let stem = local
        .strip_suffix("SegmentMember")
        .or_else(|| local.strip_suffix("Member"))
        .unwrap_or(local);
    // Split CamelCase into words.
    let mut out = String::new();
    for (i, c) in stem.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

// ── tiny tolerant XML scanning (namespace-prefix agnostic) ───────────

struct Block<'a> {
    /// From the opening tag through its content start.
    open: &'a str,
    /// Inner body (between open tag end and close tag), best-effort.
    body: &'a str,
}

/// Yield blocks for elements whose local name is `local` (any ns prefix).
fn split_blocks<'a>(xml: &'a str, local: &str) -> Vec<Block<'a>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = xml[at..].find('<') {
        let start = at + rel;
        let rest = &xml[start + 1..];
        at = start + 1;
        // Element name = up to whitespace/'>'/'/'.
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let matches = name == local
            || name
                .rsplit(':')
                .next()
                .map(|l| l == local)
                .unwrap_or(false);
        if !matches {
            continue;
        }
        let open = &xml[start..];
        // Body: from '>' to the matching close of this exact tag name.
        let Some(gt) = open.find('>') else { break };
        let body_start = gt + 1;
        let close_a = format!("</{name}>");
        let body = match open[body_start..].find(&close_a) {
            Some(e) => &open[body_start..body_start + e],
            // Self-closing or unclosed: best-effort to the next close tag.
            None => match open[body_start..].find("</") {
                Some(e) => &open[body_start..body_start + e],
                None => "",
            },
        };
        out.push(Block { open, body });
    }
    out
}

/// Value of `name="…"` in the text starting at an opening tag.
fn attr_of<'a>(open: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let tag_end = open.find('>').unwrap_or(open.len());
    let head = &open[..tag_end];
    let i = head.find(&pat)?;
    let rest = &head[i + pat.len()..];
    rest.split('"').next()
}

/// Element text content: chars after the first '>' up to the next '<'.
fn text_of(open: &str) -> &str {
    let Some(gt) = open.find('>') else { return "" };
    let rest = &open[gt + 1..];
    match rest.find('<') {
        Some(lt) => &rest[..lt],
        None => rest,
    }
}

/// First `<…local…>text<` inside `body`.
fn tag_text(body: &str, local: &str) -> Option<String> {
    for b in split_blocks(body, local) {
        let t = text_of(b.open).trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<xbrl>
      <xbrli:context id="c_seg_auto">
        <xbrli:entity><xbrli:segment>
          <xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">tsla:AutomotiveSegmentMember</xbrldi:explicitMember>
        </xbrli:segment></xbrli:entity>
        <xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period>
      </xbrli:context>
      <xbrli:context id="c_seg_energy">
        <xbrli:entity><xbrli:segment>
          <xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">tsla:EnergyGenerationAndStorageSegmentMember</xbrldi:explicitMember>
        </xbrli:segment></xbrli:entity>
        <xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period>
      </xbrli:context>
      <xbrli:context id="c_seg_elim">
        <xbrli:entity><xbrli:segment>
          <xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">us-gaap:IntersegmentEliminationMember</xbrldi:explicitMember>
        </xbrli:segment></xbrli:entity>
        <xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period>
      </xbrli:context>
      <xbrli:context id="c_double_tagged">
        <xbrli:entity><xbrli:segment>
          <xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">tsla:AutomotiveSegmentMember</xbrldi:explicitMember>
          <xbrldi:explicitMember dimension="srt:ProductsAndServicesAxis">tsla:AutomotiveSalesMember</xbrldi:explicitMember>
        </xbrli:segment></xbrli:entity>
        <xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period>
      </xbrli:context>
      <xbrli:context id="c_old_period">
        <xbrli:entity><xbrli:segment>
          <xbrldi:explicitMember dimension="us-gaap:StatementBusinessSegmentsAxis">tsla:AutomotiveSegmentMember</xbrldi:explicitMember>
        </xbrli:segment></xbrli:entity>
        <xbrli:period><xbrli:startDate>2024-01-01</xbrli:startDate><xbrli:endDate>2024-12-31</xbrli:endDate></xbrli:period>
      </xbrli:context>
      <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c_seg_auto" unitRef="usd" decimals="-6">82000000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
      <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c_seg_energy" unitRef="usd" decimals="-6">12000000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
      <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c_seg_elim" unitRef="usd" decimals="-6">-500000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
      <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c_double_tagged" unitRef="usd" decimals="-6">70000000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
      <us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax contextRef="c_old_period" unitRef="usd" decimals="-6">75000000000</us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax>
    </xbrl>"#;

    #[test]
    fn parses_single_dimension_segment_revenue_only() {
        let facts = parse_segment_revenue(FIXTURE);
        // Double-tagged (segment+product) and old-period facts are excluded.
        assert_eq!(facts.len(), 3, "{facts:?}");
        assert_eq!(facts[0].segment, "Automotive");
        assert_eq!(facts[0].value, 82_000_000_000.0);
        assert_eq!(facts[0].period_end, "2025-12-31");
        assert_eq!(facts[1].segment, "Energy Generation And Storage");
        // Eliminations kept, labeled, and last.
        assert!(facts[2].eliminations);
        assert_eq!(facts[2].value, -500_000_000.0);
        assert!(!facts[0].eliminations);
    }

    #[test]
    fn no_segment_contexts_means_empty_not_garbage() {
        assert!(parse_segment_revenue("<xbrl></xbrl>").is_empty());
        // Consolidated-only facts (no dimensions) never produce segments.
        let consolidated = r#"<xbrl>
          <xbrli:context id="c1"><xbrli:entity></xbrli:entity>
            <xbrli:period><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period></xbrli:context>
          <us-gaap:Revenues contextRef="c1">97000000000</us-gaap:Revenues>
        </xbrl>"#;
        assert!(parse_segment_revenue(consolidated).is_empty());
    }

    /// LIVE (network): real TSLA 10-K instance through the full path.
    /// Run: cargo test -p fm-fetch live_tsla_segments -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_tsla_segments() {
        let cik = crate::edgar::cik_from_ticker("TSLA").expect("cik");
        let facts = fetch_segment_revenue(&cik).expect("segments");
        for f in &facts {
            println!("  {} = {} ({}) elim={}", f.segment, f.value, f.period_end, f.eliminations);
        }
        assert!(facts.len() >= 2, "TSLA reports at least 2 segments");
        assert!(facts.iter().all(|f| f.value.abs() > 0.0));
    }

    #[test]
    fn member_names_humanize() {
        assert_eq!(humanize_member("tsla:AutomotiveSegmentMember"), "Automotive");
        assert_eq!(
            humanize_member("us-gaap:IntersegmentEliminationMember"),
            "Intersegment Elimination"
        );
        assert_eq!(humanize_member("NoPrefix"), "No Prefix");
    }
}
