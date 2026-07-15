//! 6.5 — Render a `.pptx` to PDF + per-slide PNGs, a port of
//! `src/research/pptx_render.py`'s soffice backend (the PowerPoint-COM tier is
//! intentionally not ported — subprocess soffice covers verification).
//!
//! When LibreOffice (`soffice`) is present the deck is converted to PDF
//! (`--headless --convert-to pdf`) and, if `pdftoppm` (poppler) is present,
//! rasterized to per-slide PNGs. When neither is available a clear error is
//! returned (never a silent success).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate the `soffice` binary (PATH, then common Windows install paths).
pub fn find_soffice() -> Option<String> {
    if let Some(p) = which("soffice").or_else(|| which("soffice.exe")) {
        return Some(p);
    }
    for p in [
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
    ] {
        if Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

/// Locate `pdftoppm` (poppler).
pub fn find_pdftoppm() -> Option<String> {
    which("pdftoppm").or_else(|| which("pdftoppm.exe"))
}

fn which(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    None
}

/// Render `deck_path` to PDF + per-slide PNGs in `out_dir` (default:
/// `<deck>/preview`). Returns written paths (PDF first, PNGs in slide order).
///
/// Returns `Err` with an actionable message when no render backend is present —
/// exactly the degraded-environment path (soffice/pdftoppm absent).
pub fn render_deck(deck_path: &str, out_dir: Option<&str>, dpi: u32) -> Result<Vec<PathBuf>, String> {
    let deck = Path::new(deck_path);
    if !deck.exists() {
        return Err(format!("deck not found: {deck_path}"));
    }
    let out = match out_dir {
        Some(d) => PathBuf::from(d),
        None => deck.parent().unwrap_or_else(|| Path::new(".")).join("preview"),
    };

    let soffice = match find_soffice() {
        Some(s) => s,
        None => {
            return Err(
                "No render backend available. Install LibreOffice (soffice) and poppler \
                 (pdftoppm) to render decks to PNG. (PowerPoint-COM backend is not supported.)"
                    .to_string(),
            );
        }
    };

    render_soffice(deck, &out, dpi, &soffice)
}

fn render_soffice(deck: &Path, out_dir: &Path, dpi: u32, soffice: &str) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create out dir: {e}"))?;
    let stem = deck.file_stem().and_then(|s| s.to_str()).unwrap_or("deck");

    let status = Command::new(soffice)
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(out_dir)
        .arg(deck)
        .output()
        .map_err(|e| format!("soffice failed to launch: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "soffice failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    let pdf = out_dir.join(format!("{stem}.pdf"));
    if !pdf.exists() {
        return Err(format!("soffice produced no PDF at {}", pdf.display()));
    }

    let mut written = vec![pdf.clone()];
    if let Some(pdftoppm) = find_pdftoppm() {
        let prefix = out_dir.join(format!("{stem}_slide"));
        let png = Command::new(pdftoppm)
            .args(["-r", &dpi.to_string(), "-png"])
            .arg(&pdf)
            .arg(&prefix)
            .output()
            .map_err(|e| format!("pdftoppm failed to launch: {e}"))?;
        if png.status.success() {
            let mut pngs: Vec<PathBuf> = std::fs::read_dir(out_dir)
                .map_err(|e| format!("read out dir: {e}"))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&format!("{stem}_slide")) && n.ends_with(".png"))
                        .unwrap_or(false)
                })
                .collect();
            pngs.sort();
            written.extend(pngs);
        }
    }
    Ok(written)
}
