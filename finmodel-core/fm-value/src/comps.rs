use serde::{Deserialize, Serialize};

/// A single comparable company entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompsEntry {
    pub ticker: String,
    pub ev_ebitda: f64,
    pub pe: f64,
    pub ev_revenue: f64,
}

/// Result of comparable company analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompsResult {
    pub median_ev_ebitda: f64,
    pub mean_ev_ebitda: f64,
    pub entries: Vec<CompsEntry>,
}

/// Analyze a list of comparable companies.
///
/// Computes the mean and median EV/EBITDA across the peer group.
pub fn analyze(entries: &[CompsEntry]) -> CompsResult {
    let mean_ev_ebitda = if entries.is_empty() {
        0.0
    } else {
        entries.iter().map(|e| e.ev_ebitda).sum::<f64>() / entries.len() as f64
    };

    let mut sorted: Vec<f64> = entries.iter().map(|e| e.ev_ebitda).collect();
    sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_ev_ebitda = if sorted.is_empty() {
        0.0
    } else if sorted.len() % 2 == 0 {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };

    CompsResult {
        median_ev_ebitda,
        mean_ev_ebitda,
        entries: entries.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comps_median_odd() {
        let entries = vec![
            CompsEntry {
                ticker: "A".to_string(),
                ev_ebitda: 10.0,
                pe: 15.0,
                ev_revenue: 2.0,
            },
            CompsEntry {
                ticker: "B".to_string(),
                ev_ebitda: 12.0,
                pe: 18.0,
                ev_revenue: 2.5,
            },
            CompsEntry {
                ticker: "C".to_string(),
                ev_ebitda: 8.0,
                pe: 14.0,
                ev_revenue: 1.8,
            },
        ];
        let result = analyze(&entries);
        // Sorted: 8, 10, 12 -> median = 10, mean = 30/3 = 10
        assert!((result.mean_ev_ebitda - 10.0).abs() < 1e-10);
        assert!((result.median_ev_ebitda - 10.0).abs() < 1e-10);
        assert_eq!(result.entries.len(), 3);
    }

    #[test]
    fn test_comps_median_even() {
        let entries = vec![
            CompsEntry {
                ticker: "A".to_string(),
                ev_ebitda: 10.0,
                pe: 15.0,
                ev_revenue: 2.0,
            },
            CompsEntry {
                ticker: "B".to_string(),
                ev_ebitda: 12.0,
                pe: 18.0,
                ev_revenue: 2.5,
            },
            CompsEntry {
                ticker: "C".to_string(),
                ev_ebitda: 8.0,
                pe: 14.0,
                ev_revenue: 1.8,
            },
            CompsEntry {
                ticker: "D".to_string(),
                ev_ebitda: 14.0,
                pe: 20.0,
                ev_revenue: 3.0,
            },
        ];
        let result = analyze(&entries);
        // Sorted: 8, 10, 12, 14 -> median = (10+12)/2 = 11, mean = 44/4 = 11
        assert!((result.mean_ev_ebitda - 11.0).abs() < 1e-10);
        assert!((result.median_ev_ebitda - 11.0).abs() < 1e-10);
    }

    #[test]
    fn test_comps_empty() {
        let result = analyze(&[]);
        assert!((result.mean_ev_ebitda - 0.0).abs() < 1e-10);
        assert!((result.median_ev_ebitda - 0.0).abs() < 1e-10);
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_comps_single_entry() {
        let entries = vec![CompsEntry {
            ticker: "X".to_string(),
            ev_ebitda: 15.5,
            pe: 22.0,
            ev_revenue: 3.0,
        }];
        let result = analyze(&entries);
        assert!((result.mean_ev_ebitda - 15.5).abs() < 1e-10);
        assert!((result.median_ev_ebitda - 15.5).abs() < 1e-10);
    }
}
