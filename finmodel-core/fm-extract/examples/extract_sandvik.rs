//! Live end-to-end validation on the downloaded Sandvik annual report.
//! Run: cargo run --example extract_sandvik -p fm-extract
//!
//! Validates the native (no-Python) pipeline: pdf-extract page text ->
//! sector detection -> financial section finder -> (LLM if key present).

fn main() {
    let pdf_path = "C:/Users/vinit/Documents/financial_model/tieout/filings/SAND.ST/annual_report.pdf";

    // Native PDF page extraction (pure Rust)
    let pages = match pdf_extract::extract_text_by_pages(pdf_path) {
        Ok(p) => p,
        Err(e) => { eprintln!("PDF extract failed: {e}"); std::process::exit(1); }
    };
    println!("native pages: {}", pages.len());

    // Sector detection
    let sector = fm_extract::detect_sector(&pages);
    println!("sector: {sector}");

    // Financial section finder
    let section = fm_extract::extract_financial_section(&pages, 30);
    println!("section chars: {}", section.len());

    // Validate the known fixture figures land in the selected section
    for fig in ["126,503", "122,878", "52,046", "49,136"] {
        let in_section = section.contains(fig);
        println!("  {fig:?} in section: {in_section}");
    }
    // Swedish chars survive?
    let swedish = section.contains('ä') || section.contains('ö') || section.contains('å');
    println!("  swedish chars present: {swedish}");

    // Full extraction (needs an LLM key; will fail gracefully without one)
    println!("\nAttempting full extraction (needs OPENROUTER_API_KEY)...");
    let periods = vec!["2023A".to_string(), "2024A".to_string()];
    match fm_extract::extract_financials_from_pdf(pdf_path, &periods, "SAND.ST", None) {
        Ok(result) => {
            println!("=== EXTRACTION SUCCEEDED ===");
            println!("currency: {}  years: {:?}", result.currency, result.years_found);
            if let Some(rev) = result.income_statement.get("revenue") {
                println!("revenue: {rev:?}");
            }
            let json = serde_json::to_string_pretty(&result).unwrap();
            std::fs::write("C:/Users/vinit/Documents/financial_model/SAND_ST_rust_extraction.json", &json).unwrap();
            println!("saved SAND_ST_rust_extraction.json");
        }
        Err(e) => {
            println!("extraction blocked (expected without key): {e}");
        }
    }
}
