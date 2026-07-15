//! 6.5 render gate. On this box soffice/pdftoppm are absent, so the gate
//! asserts the degraded error path (per the plan: "if soffice/pdftoppm absent,
//! the gate becomes an 'unavailable' unit test that asserts the error path").
//! When a backend IS present the render is exercised end-to-end instead.

mod common;

#[test]
fn render_deck_backend_probe() {
    let deck = format!("{}/deck.pptx", common::fixture_dir());
    let out = std::env::temp_dir().join(format!("fmpptx_render_{}", std::process::id()));
    let res = fm_pptx::render::render_deck(&deck, Some(out.to_str().unwrap()), 96);

    match fm_pptx::render::find_soffice() {
        None => {
            // Degraded environment: must return a clear, actionable error.
            let err = res.expect_err("expected 'no backend' error when soffice absent");
            assert!(
                err.contains("No render backend available"),
                "unexpected error message: {err}"
            );
        }
        Some(_) => {
            // Backend present: must produce at least a PDF.
            let paths = res.expect("render should succeed with soffice present");
            assert!(!paths.is_empty(), "render produced no output");
            assert!(paths[0].extension().and_then(|e| e.to_str()) == Some("pdf"));
            let _ = std::fs::remove_dir_all(&out);
        }
    }
}

#[test]
fn render_deck_missing_file_errors() {
    let res = fm_pptx::render::render_deck("C:/does/not/exist.pptx", None, 96);
    assert!(res.is_err(), "missing deck must error");
}
