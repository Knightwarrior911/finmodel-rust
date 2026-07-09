pub mod compare;
pub mod types;

use std::path::Path;

use types::*;

pub use compare::compare;

/// Score a `ModelOutput` against `GroundTruth`.
pub fn score(ground_truth: &GroundTruth, model: &ModelOutput) -> Score {
    compare::compare(ground_truth, model)
}

/// Parse and score two JSON strings.
pub fn score_from_json(gt_json: &str, model_json: &str) -> Result<Score, Error> {
    let gt: GroundTruth = serde_json::from_str(gt_json)?;
    let mo: ModelOutput = serde_json::from_str(model_json)?;
    Ok(compare::compare(&gt, &mo))
}

/// Load ground truth from a JSON file path.
pub fn load_ground_truth(path: &Path) -> Result<GroundTruth, Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

/// Load model output from a JSON file path.
pub fn load_model(path: &Path) -> Result<ModelOutput, Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}
