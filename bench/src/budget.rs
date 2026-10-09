//! Budgets (`budgets.toml`) and the baseline regression rule.

use std::collections::BTreeMap;

use serde::Deserialize;

/// A metric may not exceed its baseline by more than this factor.
pub const REGRESSION: f64 = 1.10;

pub type Metrics = BTreeMap<String, f64>;

#[derive(Debug, Deserialize)]
pub struct Limit {
    pub max: f64,
    pub max_macos: Option<f64>,
}

/// `scenario -> metric -> limit`.
pub type Budgets = BTreeMap<String, BTreeMap<String, Limit>>;
/// `scenario -> metric -> value` (baseline.json).
pub type Baseline = BTreeMap<String, Metrics>;

#[derive(Debug, PartialEq)]
pub struct Row {
    pub metric: String,
    pub value: f64,
    pub limit: Option<f64>,
    pub baseline: Option<f64>,
    pub failure: Option<String>,
}

/// Compares measured metrics with the budget and the baseline. A budgeted metric that was not
/// measured is a failure; a measured metric without a budget is reported but never fails.
pub fn evaluate(
    scenario: &str,
    metrics: &Metrics,
    budgets: &Budgets,
    baseline: &Baseline,
    macos: bool,
) -> Vec<Row> {
    let limits = budgets.get(scenario);
    let base = baseline.get(scenario);
    let mut rows: Vec<Row> = metrics
        .iter()
        .map(|(name, &value)| {
            let limit = limits
                .and_then(|l| l.get(name))
                .map(|l| if macos { l.max_macos.unwrap_or(l.max) } else { l.max });
            let baseline = base.and_then(|b| b.get(name)).copied();
            let failure = match (limit, baseline) {
                (Some(max), _) if value > max => Some(format!("{value} exceeds budget {max}")),
                (_, Some(b)) if b > 0.0 && value > b * REGRESSION => {
                    Some(format!("{value} regressed more than 10% over baseline {b}"))
                }
                _ => None,
            };
            Row { metric: name.clone(), value, limit, baseline, failure }
        })
        .collect();
    for name in limits.into_iter().flat_map(BTreeMap::keys).filter(|k| !metrics.contains_key(*k)) {
        rows.push(Row {
            metric: name.clone(),
            value: f64::NAN,
            limit: None,
            baseline: None,
            failure: Some("budgeted metric was not measured".into()),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budgets() -> Budgets {
        toml::from_str("[s.a]\nmax = 10.0\nmax_macos = 5.0\n[s.b]\nmax = 1.0\n").unwrap()
    }

    fn m(pairs: &[(&str, f64)]) -> Metrics {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    fn failures(rows: &[Row]) -> usize {
        rows.iter().filter(|r| r.failure.is_some()).count()
    }

    #[test]
    fn budget_is_per_platform() {
        let metrics = m(&[("a", 7.0), ("b", 0.5)]);
        assert_eq!(failures(&evaluate("s", &metrics, &budgets(), &Baseline::new(), false)), 0);
        assert_eq!(failures(&evaluate("s", &metrics, &budgets(), &Baseline::new(), true)), 1);
    }

    #[test]
    fn baseline_allows_ten_percent() {
        let base: Baseline = [("s".to_owned(), m(&[("a", 4.0)]))].into();
        let ok = evaluate("s", &m(&[("a", 4.4), ("b", 0.5)]), &budgets(), &base, false);
        let bad = evaluate("s", &m(&[("a", 4.5), ("b", 0.5)]), &budgets(), &base, false);
        assert_eq!((failures(&ok), failures(&bad)), (0, 1));
    }

    #[test]
    fn missing_budgeted_metric_fails() {
        assert_eq!(failures(&evaluate("s", &m(&[("a", 1.0)]), &budgets(), &Baseline::new(), false)), 1);
    }
}
