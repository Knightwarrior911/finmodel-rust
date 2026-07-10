//! Live end-to-end extraction test on the downloaded Sandvik annual report.
//! Run: cargo run --example extract_sandvik -p fm-extract
//!
//! Proves: PDF text extraction -> section finder -> LLM -> structured JSON.

fn main() {
    let pdf_path = "C:/Users/vinit/Documents/financial_model/tieout/filings/SAND.ST/annual_report.pdf";
    let periods = vec!["2023A".to_string(), "2024A".to_string()];

    eprintln!("Extracting from {pdf_path} ...");
    match fm_extract::extract_financials_from_pdf(pdf_path, &periods, "SAND.ST") {
        Ok(result) => {
            println!("=== EXTRACTION SUCCEEDED ===");
            println!("currency: {}", result.currency);
            println!("years_found: {:?}", result.years_found);
            println!("confidence: {}", result.confidence);
            println!("\n--- income_statement ---");
            let mut is_keys: Vec<_> = result.income_statement.keys().collect();
            is_keys.sort();
            for k in is_keys {
                println!("  {}: {:?}", k, result.income_statement[k]);
            }
            println!("\n--- balance_sheet ---");
            let mut bs_keys: Vec<_> = result.balance_sheet.keys().collect();
            bs_keys.sort();
            for k in bs_keys {
                println!("  {}: {:?}", k, result.balance_sheet[k]);
            }
            println!("\n--- cash_flow_statement ---");
            let mut cf_keys: Vec<_> = result.cash_flow_statement.keys().collect();
            cf_keys.sort();
            for k in cf_keys {
                println!("  {}: {:?}", k, result.cash_flow_statement[k]);
            }
            let json = serde_json::to_string_pretty(&result).unwrap();
            std::fs::write(
                "C:/Users/vinit/Documents/financial_model/SAND_ST_rust_extraction.json",
                &json,
            )
            .unwrap();
            println!("\nSaved to SAND_ST_rust_extraction.json");
        }
        Err(e) => {
            eprintln!("=== EXTRACTION FAILED ===");
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
