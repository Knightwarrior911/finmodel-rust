//! User attachments: staging (paste / picker / drag-drop) and text extraction.
//!
//! Security model mirrors artifacts.rs: the frontend never supplies raw
//! filesystem paths. Bytes arrive over IPC (base64), are written under the
//! app config dir, and are referenced by opaque `art-…` handles scoped to an
//! owner token (the conversation id, or a client staging token before a
//! conversation exists).
//!
//! Extraction is deterministic and engine-side: PPTX via fm-pptx's inspector,
//! XLSX via calamine, DOCX via a minimal OOXML text strip, and plain text
//! files read directly. PDFs are NOT extracted here — they flow through the
//! existing `analyze_pdf` tool. Images are not extracted; they ride the
//! provider request as vision parts.

use crate::commands::artifacts::{ArtifactKind, ArtifactRegistry};
use crate::error::{AppError, AppResult};
use base64::Engine as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{Manager, State};

/// What the composer may attach. One place, shared by staging AND drop grants.
pub const ALLOWED_EXTS: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "pptx", "xlsx", "docx", "csv", "txt", "md", "json",
];

/// Per-file byte caps: images stay small enough for a vision request; files
/// cap at 25 MB (claude.ai parity).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
/// Per-attachment extracted-text cap and whole-message budget (chars).
pub const MAX_EXTRACT_CHARS: usize = 12_000;
pub const MAX_TOTAL_EXTRACT_CHARS: usize = 30_000;
/// Vision caps per turn.
pub const MAX_IMAGES_PER_TURN: usize = 4;

/// Coarse attachment class the UI and message builder both speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentClass {
    Pdf,
    Image,
    Sheet,
    Deck,
    Doc,
    Text,
}

impl AttachmentClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            AttachmentClass::Pdf => "pdf",
            AttachmentClass::Image => "image",
            AttachmentClass::Sheet => "sheet",
            AttachmentClass::Deck => "deck",
            AttachmentClass::Doc => "doc",
            AttachmentClass::Text => "text",
        }
    }
}

/// Classify by extension. `None` = not attachable.
pub fn classify(name: &str) -> Option<AttachmentClass> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(AttachmentClass::Pdf),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(AttachmentClass::Image),
        "xlsx" => Some(AttachmentClass::Sheet),
        "pptx" => Some(AttachmentClass::Deck),
        "docx" => Some(AttachmentClass::Doc),
        "csv" | "txt" | "md" | "json" => Some(AttachmentClass::Text),
        _ => None,
    }
}

/// MIME for vision data URLs.
pub fn image_mime(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

/// Filesystem-safe stem for the staged copy.
fn safe_stem(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("attachment");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.chars().take(80).collect()
}

/// Stage user-supplied bytes as an attachment. `owner` is the conversation id
/// or a client staging token (`stg-…`); the same token must be echoed to
/// `agent_send`. Returns `{artifact_id, label, class, size}`.
#[tauri::command]
pub fn stage_attachment(
    app: tauri::AppHandle,
    registry: State<'_, ArtifactRegistry>,
    owner: String,
    name: String,
    bytes_b64: String,
) -> AppResult<String> {
    let owner = owner.trim().to_string();
    if owner.is_empty() {
        return Err(AppError::Config("attachment owner token required".into()));
    }
    let class = classify(&name).ok_or_else(|| {
        AppError::Config(format!(
            "that file type isn't supported — I can take {}",
            ALLOWED_EXTS.join(", ")
        ))
    })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(bytes_b64.as_bytes())
        .map_err(|e| AppError::Config(format!("bad attachment payload: {e}")))?;
    let cap = if class == AttachmentClass::Image {
        MAX_IMAGE_BYTES
    } else {
        MAX_FILE_BYTES
    };
    if bytes.is_empty() {
        return Err(AppError::Config("empty file".into()));
    }
    if bytes.len() > cap {
        return Err(AppError::Config(format!(
            "\u{201c}{name}\u{201d} is too large ({:.1} MB) — the limit is {} MB",
            bytes.len() as f64 / 1.0e6,
            cap / (1024 * 1024)
        )));
    }
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("no config dir: {e}")))?
        .join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Config(format!("attachments dir: {e}")))?;
    let file = dir.join(format!(
        "{}_{}",
        &crate::commands::chat::iso_now()[..10],
        safe_stem(&name)
    ));
    // Avoid collisions: suffix a counter when the day+name already exists.
    let file = unique_path(file);
    std::fs::write(&file, &bytes).map_err(|e| AppError::Config(format!("save attachment: {e}")))?;
    let kind = if class == AttachmentClass::Pdf {
        ArtifactKind::UserPdf
    } else {
        ArtifactKind::UserFile
    };
    let label = safe_stem(&name);
    let id = registry
        .register(file, kind, label.clone(), Some(owner))
        .map_err(AppError::Config)?;
    Ok(serde_json::json!({
        "artifact_id": id,
        "label": label,
        "class": class.as_str(),
        "size": bytes.len(),
    })
    .to_string())
}

fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a")
        .to_string();
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    for i in 1..1000 {
        let cand = p.with_file_name(if ext.is_empty() {
            format!("{stem}({i})")
        } else {
            format!("{stem}({i}).{ext}")
        });
        if !cand.exists() {
            return cand;
        }
    }
    p
}

/// Extract readable text from a staged attachment for the message context.
/// Returns `None` for classes that don't extract here (pdf, image).
pub fn extract_text(path: &Path, class: AttachmentClass) -> Option<Result<String, String>> {
    match class {
        AttachmentClass::Pdf | AttachmentClass::Image => None,
        AttachmentClass::Text => Some(
            std::fs::read_to_string(path)
                .map(|s| cap_chars(&s, MAX_EXTRACT_CHARS))
                .map_err(|e| e.to_string()),
        ),
        AttachmentClass::Deck => Some(extract_pptx(path)),
        AttachmentClass::Sheet => Some(extract_xlsx(path)),
        AttachmentClass::Doc => Some(extract_docx(path)),
    }
}

fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}\n[… truncated]")
}

/// PPTX → slide-ordered text runs via fm-pptx's inspector.
fn extract_pptx(path: &Path) -> Result<String, String> {
    let v = fm_pptx::inspect::inspect_pptx(&path.to_string_lossy())?;
    let mut out = String::new();
    if let Some(slides) = v.get("slides").and_then(|s| s.as_array()) {
        for (i, slide) in slides.iter().enumerate() {
            let mut slide_text = Vec::new();
            if let Some(els) = slide.get("elements").and_then(|e| e.as_array()) {
                for el in els {
                    if let Some(t) = el
                        .pointer("/text/text")
                        .and_then(|t| t.as_str())
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                    {
                        slide_text.push(t.to_string());
                    }
                }
            }
            if !slide_text.is_empty() {
                out.push_str(&format!(
                    "--- Slide {} ---\n{}\n",
                    i + 1,
                    slide_text.join("\n")
                ));
            }
        }
    }
    if out.trim().is_empty() {
        return Err("no readable text in the deck".into());
    }
    Ok(cap_chars(&out, MAX_EXTRACT_CHARS))
}

/// XLSX → per-sheet TSV (row-major, first 200 rows × 30 cols per sheet).
fn extract_xlsx(path: &Path) -> Result<String, String> {
    use calamine::{open_workbook_auto, Reader};
    let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let names: Vec<String> = wb.sheet_names().to_vec();
    let mut out = String::new();
    for name in names.iter().take(8) {
        let Ok(range) = wb.worksheet_range(name) else {
            continue;
        };
        out.push_str(&format!("--- Sheet {name} ---\n"));
        for row in range.rows().take(200) {
            let cells: Vec<String> = row
                .iter()
                .take(30)
                .map(|c| c.to_string().replace(['\t', '\n'], " "))
                .collect();
            let line = cells.join("\t");
            if !line.trim().is_empty() {
                out.push_str(&line);
                out.push('\n');
            }
            if out.chars().count() > MAX_EXTRACT_CHARS {
                break;
            }
        }
        if out.chars().count() > MAX_EXTRACT_CHARS {
            break;
        }
    }
    if out.trim().is_empty() {
        return Err("no readable cells in the workbook".into());
    }
    Ok(cap_chars(&out, MAX_EXTRACT_CHARS))
}

/// DOCX → paragraph text: read word/document.xml from the OOXML zip and keep
/// `<w:t>` runs, inserting newlines at paragraph ends. Deliberately minimal —
/// prose fidelity, not layout.
fn extract_docx(path: &Path) -> Result<String, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut doc = zip
        .by_name("word/document.xml")
        .map_err(|_| "not a Word document (missing document.xml)".to_string())?;
    let mut xml = String::new();
    doc.read_to_string(&mut xml).map_err(|e| e.to_string())?;
    let text = docx_xml_to_text(&xml);
    if text.trim().is_empty() {
        return Err("no readable text in the document".into());
    }
    Ok(cap_chars(&text, MAX_EXTRACT_CHARS))
}

/// Pull `<w:t>` contents out of OOXML, paragraph-aware. Pure for tests.
pub(crate) fn docx_xml_to_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    loop {
        // Paragraph end → newline.
        let next_t = rest.find("<w:t");
        let next_p_end = rest.find("</w:p>");
        match (next_t, next_p_end) {
            (None, None) => break,
            (Some(t), p) if p.map(|p| t < p).unwrap_or(true) => {
                let after = &rest[t..];
                let Some(gt) = after.find('>') else { break };
                // Self-closing <w:t/> carries no text.
                if after[..gt].ends_with('/') {
                    rest = &after[gt + 1..];
                    continue;
                }
                let body = &after[gt + 1..];
                let Some(end) = body.find("</w:t>") else {
                    break;
                };
                out.push_str(&unescape_xml(&body[..end]));
                rest = &body[end + 6..];
            }
            (_, Some(p)) => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                rest = &rest[p + 6..];
            }
            (Some(_), None) => break,
        }
    }
    out
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Whether any staged attachment classifies as an image — a READ-ONLY
/// pre-flight probe. The vision router must decide (and possibly refuse the
/// send) BEFORE [`build_attachment_context`] consumes the staging handles,
/// so a refused send never eats the user's attachments.
pub fn has_image_attachments(
    registry: &ArtifactRegistry,
    attachments: &[(String, String)],
) -> bool {
    attachments.iter().any(|(id, scope)| {
        registry
            .resolve(id, Some(scope))
            .ok()
            .and_then(|(path, _, _)| classify(&path.to_string_lossy()))
            .map(|c| matches!(c, AttachmentClass::Image))
            .unwrap_or(false)
    })
}
/// Build the message-side view of a turn's attachments:
/// - text blocks appended to the user message (extractions + tool pointers)
/// - image data URLs for the vision request
///
/// PDF artifacts are re-registered under the real conversation id so the
/// `analyze_pdf` tool can resolve them later in the turn.
pub fn build_attachment_context(
    registry: &ArtifactRegistry,
    conversation_id: &str,
    attachments: &[(String, String)], // (artifact_id, owner_scope)
) -> (Vec<String>, Vec<String>) {
    let mut blocks: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    let mut total_chars = 0usize;
    for (artifact_id, scope) in attachments {
        let Ok((path, _kind, label)) = registry.resolve(artifact_id, Some(scope)) else {
            blocks.push(format!(
                "[Attachment \u{201c}{artifact_id}\u{201d} expired before send — ask the user to re-attach it.]"
            ));
            continue;
        };
        let Some(class) = classify(&path.to_string_lossy()) else {
            continue;
        };
        match class {
            AttachmentClass::Image => {
                if images.len() >= MAX_IMAGES_PER_TURN {
                    blocks.push(format!(
                        "[Image \u{201c}{label}\u{201d} skipped — at most {MAX_IMAGES_PER_TURN} images per message.]"
                    ));
                    continue;
                }
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let mime = image_mime(&path.to_string_lossy());
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        images.push(format!("data:{mime};base64,{b64}"));
                        blocks.push(format!(
                            "[Attached image \u{201c}{label}\u{201d} — shown above.]"
                        ));
                    }
                    Err(e) => {
                        blocks.push(format!("[Image \u{201c}{label}\u{201d} unreadable: {e}]"))
                    }
                }
            }
            AttachmentClass::Pdf => {
                // Move ownership to the live conversation for analyze_pdf.
                let new_id = registry
                    .register(
                        path.clone(),
                        ArtifactKind::UserPdf,
                        label.clone(),
                        Some(conversation_id.to_string()),
                    )
                    .unwrap_or_else(|_| artifact_id.clone());
                blocks.push(format!(
                    "[Attached PDF \u{201c}{label}\u{201d} — artifact_id {new_id}. Use the analyze_pdf tool to read it.]"
                ));
            }
            _ => match extract_text(&path, class) {
                Some(Ok(text)) => {
                    let remaining = MAX_TOTAL_EXTRACT_CHARS.saturating_sub(total_chars);
                    let capped = cap_chars(&text, remaining.min(MAX_EXTRACT_CHARS));
                    total_chars += capped.chars().count();
                    blocks.push(format!(
                        "[Attached {} \u{201c}{label}\u{201d} — extracted content:]\n{capped}",
                        class.as_str()
                    ));
                }
                Some(Err(e)) => blocks.push(format!(
                    "[Attached {} \u{201c}{label}\u{201d} could not be read: {e}]",
                    class.as_str()
                )),
                None => {}
            },
        }
    }
    (blocks, images)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_supported_and_rejects_unknown() {
        assert_eq!(classify("deck.PPTX"), Some(AttachmentClass::Deck));
        assert_eq!(classify("shot.png"), Some(AttachmentClass::Image));
        assert_eq!(classify("10k.pdf"), Some(AttachmentClass::Pdf));
        assert_eq!(classify("model.xlsx"), Some(AttachmentClass::Sheet));
        assert_eq!(classify("memo.docx"), Some(AttachmentClass::Doc));
        assert_eq!(classify("notes.md"), Some(AttachmentClass::Text));
        assert_eq!(classify("virus.exe"), None);
        assert_eq!(classify("noext"), None);
    }

    #[test]
    fn docx_text_strip_keeps_runs_and_paragraphs() {
        let xml = r#"<w:document><w:p><w:r><w:t>Revenue grew</w:t></w:r><w:r><w:t xml:space="preserve"> 12%</w:t></w:r></w:p><w:p><w:r><w:t>Margins held &amp; expanded</w:t></w:r></w:p></w:document>"#;
        let t = docx_xml_to_text(xml);
        assert_eq!(t, "Revenue grew 12%\nMargins held & expanded\n");
    }

    #[test]
    fn docx_self_closing_t_is_skipped() {
        let xml = r#"<w:p><w:r><w:t/></w:r><w:r><w:t>ok</w:t></w:r></w:p>"#;
        assert_eq!(docx_xml_to_text(xml), "ok\n");
    }

    #[test]
    fn cap_chars_truncates_honestly() {
        let s = "abcdef";
        assert_eq!(cap_chars(s, 10), "abcdef");
        assert!(cap_chars(s, 3).starts_with("abc"));
        assert!(cap_chars(s, 3).contains("truncated"));
    }

    #[test]
    fn safe_stem_sanitizes() {
        assert_eq!(safe_stem("../..\\evil name?.pdf"), "evil_name_.pdf");
    }

    #[test]
    fn image_mime_maps() {
        assert_eq!(image_mime("a.jpg"), "image/jpeg");
        assert_eq!(image_mime("a.webp"), "image/webp");
        assert_eq!(image_mime("a.png"), "image/png");
    }

    #[test]
    fn attachment_context_moves_pdf_ownership_and_reads_text() {
        let reg = ArtifactRegistry::default();
        let dir = std::env::temp_dir().join(format!("fm_att_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, "Q2 revenue was 25,500M.").unwrap();
        let pdf = dir.join("deck.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 fake").unwrap();
        let t_id = reg
            .register(
                txt,
                ArtifactKind::UserFile,
                "notes.txt",
                Some("stg-1".into()),
            )
            .unwrap();
        let p_id = reg
            .register(pdf, ArtifactKind::UserPdf, "deck.pdf", Some("stg-1".into()))
            .unwrap();
        let (blocks, images) = build_attachment_context(
            &reg,
            "conv-9",
            &[(t_id, "stg-1".into()), (p_id.clone(), "stg-1".into())],
        );
        assert!(images.is_empty());
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("25,500M"));
        // PDF block references a NEW id resolvable under the real conversation.
        let new_id = blocks[1]
            .split("artifact_id ")
            .nth(1)
            .and_then(|s| s.split('.').next())
            .unwrap()
            .trim()
            .to_string();
        assert_ne!(new_id, p_id);
        assert!(reg.resolve(&new_id, Some("conv-9")).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_attachment_degrades_honestly() {
        let reg = ArtifactRegistry::default();
        let (blocks, images) =
            build_attachment_context(&reg, "conv-1", &[("art-nope".into(), "stg-x".into())]);
        assert!(images.is_empty());
        assert!(blocks[0].contains("expired"));
    }
}
