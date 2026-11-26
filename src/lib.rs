//! SigmaOS bi-analytics — business intelligence metrics and dashboards.
//!
//! Define metrics over JSON record datasets, compute aggregations
//! (count, sum, avg, min, max, group-by), and assemble dashboards.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Errors produced by this component.
#[derive(Debug, thiserror::Error)]
pub enum BiError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("query error: {0}")]
    Query(String),
}

/// Aggregation function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// A metric definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub field: String,
    pub agg: AggFn,
    /// Optional group-by field.
    #[serde(default)]
    pub group_by: Option<String>,
    /// Optional filter: field == value.
    #[serde(default)]
    pub filter_field: Option<String>,
    #[serde(default)]
    pub filter_value: Option<String>,
}

/// Metric result: scalar or grouped.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MetricResult {
    Scalar(f64),
    Grouped(BTreeMap<String, f64>),
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Compute a metric over records.
pub fn compute(records: &[Value], metric: &Metric) -> Result<MetricResult, BiError> {
    let filter_value_json = metric
        .filter_value
        .as_ref()
        .and_then(|v| serde_json::from_str::<Value>(v.as_str()).ok());
    let filtered: Vec<&Value> = records
        .iter()
        .filter(|r| match (&metric.filter_field, &metric.filter_value) {
            (Some(f), Some(v)) => r
                .get(f)
                .map(|x| {
                    x.as_str() == Some(v.as_str())
                        || filter_value_json.as_ref().map(|e| e == x).unwrap_or(false)
                })
                .unwrap_or(false),
            _ => true,
        })
        .collect();

    if let Some(gf) = &metric.group_by {
        let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for r in filtered {
            let key = r
                .get(gf)
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_else(|| "null".into());
            let val = r.get(&metric.field).and_then(num);
            groups.entry(key).or_default().push(val.unwrap_or(0.0));
        }
        let mut out = BTreeMap::new();
        for (k, vals) in groups {
            out.insert(k, agg_vals(&vals, metric.agg));
        }
        Ok(MetricResult::Grouped(out))
    } else {
        let vals: Vec<f64> = filtered
            .iter()
            .filter_map(|r| r.get(&metric.field).and_then(num))
            .collect();
        if metric.agg == AggFn::Count {
            return Ok(MetricResult::Scalar(filtered.len() as f64));
        }
        if vals.is_empty() {
            return Err(BiError::Query(format!(
                "no numeric values for field '{}'",
                metric.field
            )));
        }
        Ok(MetricResult::Scalar(agg_vals(&vals, metric.agg)))
    }
}

fn agg_vals(vals: &[f64], agg: AggFn) -> f64 {
    match agg {
        AggFn::Count => vals.len() as f64,
        AggFn::Sum => vals.iter().sum(),
        AggFn::Avg => vals.iter().sum::<f64>() / vals.len().max(1) as f64,
        AggFn::Min => vals.iter().cloned().fold(f64::INFINITY, f64::min),
        AggFn::Max => vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    }
}

/// A dashboard widget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub title: String,
    pub metric: Metric,
    /// chart type hint: "number" | "bar" | "table"
    #[serde(default = "default_chart")]
    pub chart: String,
}

fn default_chart() -> String {
    "number".into()
}

/// A dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub name: String,
    /// Dataset file (JSON array).
    pub dataset: PathBuf,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

/// Render a widget result to a display string.
pub fn render_widget(title: &str, result: &MetricResult) -> String {
    match result {
        MetricResult::Scalar(v) => format!("{title}: {v:.2}"),
        MetricResult::Grouped(map) => {
            let mut s = format!("{title}:\n");
            for (k, v) in map {
                s.push_str(&format!("  {k}: {v:.2}\n"));
            }
            s
        }
    }
}

/// Run all widgets in a dashboard against its dataset.
pub fn run_dashboard(dash: &Dashboard) -> Result<Vec<(String, MetricResult)>, BiError> {
    let text = std::fs::read_to_string(&dash.dataset)
        .map_err(|_| BiError::NotFound(dash.dataset.display().to_string()))?;
    let v: Value = serde_json::from_str(&text)?;
    let records = match v {
        Value::Array(a) => a,
        Value::Object(o) => match o.get("records") {
            Some(Value::Array(a)) => a.clone(),
            _ => return Err(BiError::Query("no records array".into())),
        },
        _ => return Err(BiError::Query("dataset must be array".into())),
    };
    let mut out = Vec::new();
    for w in &dash.widgets {
        out.push((w.title.clone(), compute(&records, &w.metric)?));
    }
    Ok(out)
}

/// Save/load dashboard JSON.
pub fn save_dashboard(d: &Dashboard, path: &Path) -> Result<(), BiError> {
    std::fs::write(path, serde_json::to_string_pretty(d)?)?;
    Ok(())
}

pub fn load_dashboard(path: &Path) -> Result<Dashboard, BiError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| BiError::NotFound(path.display().to_string()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Default dashboards directory.
pub fn default_dashboards_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "sigmaos", "sigmaos")
        .map(|p| p.config_dir().join("bi-analytics").join("dashboards"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn records() -> Vec<Value> {
        vec![
            json!({"region": "us", "sales": 10, "qty": 1}),
            json!({"region": "us", "sales": 20, "qty": 2}),
            json!({"region": "eu", "sales": 5, "qty": 3}),
            json!({"region": "eu", "sales": "7", "qty": 4}),
        ]
    }

    fn metric(field: &str, agg: AggFn) -> Metric {
        Metric {
            name: "m".into(),
            field: field.into(),
            agg,
            group_by: None,
            filter_field: None,
            filter_value: None,
        }
    }

    #[test]
    fn scalar_aggs() {
        let r = records();
        assert_eq!(
            compute(&r, &metric("sales", AggFn::Sum)).unwrap(),
            MetricResult::Scalar(42.0)
        );
        assert_eq!(
            compute(&r, &metric("sales", AggFn::Avg)).unwrap(),
            MetricResult::Scalar(10.5)
        );
        assert_eq!(
            compute(&r, &metric("sales", AggFn::Min)).unwrap(),
            MetricResult::Scalar(5.0)
        );
        assert_eq!(
            compute(&r, &metric("sales", AggFn::Max)).unwrap(),
            MetricResult::Scalar(20.0)
        );
        // numeric strings parse too
        assert_eq!(
            compute(&r, &metric("sales", AggFn::Count)).unwrap(),
            MetricResult::Scalar(4.0)
        );
    }

    #[test]
    fn group_by_sorts_and_aggregates() {
        let mut m = metric("sales", AggFn::Sum);
        m.group_by = Some("region".into());
        let out = compute(&records(), &m).unwrap();
        let MetricResult::Grouped(map) = out else {
            panic!("expected grouped")
        };
        assert_eq!(map["eu"], 12.0);
        assert_eq!(map["us"], 30.0);
    }

    #[test]
    fn filter_limits_rows() {
        let mut m = metric("sales", AggFn::Sum);
        m.filter_field = Some("region".into());
        m.filter_value = Some("us".into());
        assert_eq!(compute(&records(), &m).unwrap(), MetricResult::Scalar(30.0));
    }

    #[test]
    fn empty_numeric_is_error() {
        let r = vec![json!({"name": "x"})];
        assert!(compute(&r, &metric("missing", AggFn::Sum)).is_err());
    }

    #[test]
    fn dashboard_roundtrip_and_render() {
        let dir = std::env::temp_dir();
        let path = dir.join("sigmaos-bi-test-dash.json");
        let d = Dashboard {
            name: "t".into(),
            dataset: PathBuf::from("data.json"),
            widgets: vec![Widget {
                title: "Sales".into(),
                metric: metric("sales", AggFn::Sum),
                chart: "number".into(),
            }],
        };
        save_dashboard(&d, &path).unwrap();
        let back = load_dashboard(&path).unwrap();
        assert_eq!(back.name, "t");
        assert_eq!(back.widgets.len(), 1);
        let _ = std::fs::remove_file(&path);
        assert!(render_widget("Sales", &MetricResult::Scalar(42.0)).contains("42.00"));
    }
}
