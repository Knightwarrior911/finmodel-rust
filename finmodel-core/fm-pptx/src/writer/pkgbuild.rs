//! Minimal-but-valid OOXML package assembly for the deck writer (6.4).
//!
//! python-pptx starts from a full default template; the finmodel deck writer
//! only needs a valid package whose *slides* carry the emitted shapes (the 6.4
//! gate inspects slides). We ship a compact 1-master / 1-blank-layout / 1-theme
//! template and wire the generated slide parts into it.

use crate::pkg::Package;

fn emu(inches: f64) -> i64 {
    (inches * 914400.0) as i64
}

/// Assemble the package from slide dimensions + per-slide shape XML fragments.
pub fn build_package(slide_w_in: f64, slide_h_in: f64, slides: &[Vec<String>]) -> Package {
    let mut pkg = Package::default();
    let n = slides.len();
    let cx = emu(slide_w_in);
    let cy = emu(slide_h_in);

    // [Content_Types].xml
    let mut overrides = String::new();
    overrides.push_str("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>");
    overrides.push_str("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>");
    overrides.push_str("<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>");
    overrides.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    for i in 1..=n {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    let ct = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
{overrides}</Types>"
    );
    pkg.set("[Content_Types].xml", ct.into_bytes());

    // _rels/.rels
    pkg.set(
        "_rels/.rels",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
</Relationships>"
            .to_string()
            .into_bytes(),
    );

    // ppt/presentation.xml
    let mut sld_ids = String::new();
    for i in 0..n {
        sld_ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 256 + i, i + 2));
    }
    let pres = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>\
<p:sldIdLst>{sld_ids}</p:sldIdLst>\
<p:sldSz cx=\"{cx}\" cy=\"{cy}\" type=\"screen16x9\"/>\
<p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>"
    );
    pkg.set("ppt/presentation.xml", pres.into_bytes());

    // ppt/_rels/presentation.xml.rels
    let mut pres_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>",
    );
    for i in 0..n {
        pres_rels.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{}.xml\"/>",
            i + 2,
            i + 1
        ));
    }
    pres_rels.push_str("</Relationships>");
    pkg.set("ppt/_rels/presentation.xml.rels", pres_rels.into_bytes());

    // theme
    pkg.set("ppt/theme/theme1.xml", THEME1.to_string().into_bytes());

    // master + rels
    pkg.set("ppt/slideMasters/slideMaster1.xml", master_xml().into_bytes());
    pkg.set(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/>\
</Relationships>"
            .to_string()
            .into_bytes(),
    );

    // layout + rels
    pkg.set("ppt/slideLayouts/slideLayout1.xml", layout_xml().into_bytes());
    pkg.set(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/>\
</Relationships>"
            .to_string()
            .into_bytes(),
    );

    // slides + rels
    for (i, shapes) in slides.iter().enumerate() {
        let idx = i + 1;
        let body = shapes.join("");
        let sld = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr/>{body}</p:spTree></p:cSld>\
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
        );
        pkg.set(&format!("ppt/slides/slide{idx}.xml"), sld.into_bytes());
        pkg.set(
            &format!("ppt/slides/_rels/slide{idx}.xml.rels"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
</Relationships>"
                .to_string()
                .into_bytes(),
        );
    }

    pkg
}

fn master_xml() -> String {
    // Blank master: empty spTree + clrMap + a single layout id ref.
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld name=\"\"><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg>\
<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr/></p:spTree></p:cSld>\
<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" \
accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>"
        .to_string()
}

fn layout_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" type=\"blank\" preserve=\"1\">\
<p:cSld name=\"Blank\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr/></p:spTree></p:cSld>\
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
        .to_string()
}

/// A compact but complete theme (clrScheme + fontScheme + fmtScheme).
const THEME1: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\">\
<a:themeElements>\
<a:clrScheme name=\"Office\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2>\
<a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1>\
<a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2>\
<a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3>\
<a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4>\
<a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5>\
<a:accent6><a:srgbClr val=\"F79646\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink>\
<a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink>\
</a:clrScheme>\
<a:fontScheme name=\"Office\">\
<a:majorFont><a:latin typeface=\"Calibri Light\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont>\
</a:fontScheme>\
<a:fmtScheme name=\"Office\">\
<a:fillStyleLst>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
</a:fillStyleLst>\
<a:lnStyleLst>\
<a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
<a:ln w=\"12700\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
<a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
</a:lnStyleLst>\
<a:effectStyleLst>\
<a:effectStyle><a:effectLst/></a:effectStyle>\
<a:effectStyle><a:effectLst/></a:effectStyle>\
<a:effectStyle><a:effectLst/></a:effectStyle>\
</a:effectStyleLst>\
<a:bgFillStyleLst>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
</a:bgFillStyleLst>\
</a:fmtScheme>\
</a:themeElements></a:theme>";
