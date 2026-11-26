//! CLI for sigmaos-bi-analytics.

use clap::{Parser, Subcommand};
use serde_json::Value;
use sigmaos_bi_analytics::{compute, load_dashboard, render_widget, run_dashboard, AggFn, Metric};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sigmaos-bi-analytics", about = "SigmaOS BI analytics")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compute a single metric on a JSON dataset.
    Metric {
        dataset: PathBuf,
        field: String,
        /// count|sum|avg|min|max
        #[arg(long, default_value = "count")]
        agg: String,
        #[arg(long)]
        group_by: Option<String>,
    },
    /// Run a dashboard JSON file.
    Dashboard { file: PathBuf },
    /// Show dataset summary.
    Inspect { dataset: PathBuf },
}

fn parse_agg(s: &str) -> Result<AggFn, Box<dyn std::error::Error>> {
    Ok(match s {
        "count" => AggFn::Count,
        "sum" => AggFn::Sum,
        "avg" => AggFn::Avg,
        "min" => AggFn::Min,
        "max" => AggFn::Max,
        other => return Err(format!("unknown agg: {other}").into()),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    match Cli::parse().command {
        Commands::Metric {
            dataset,
            field,
            agg,
            group_by,
        } => {
            let text = std::fs::read_to_string(&dataset)?;
            let v: Value = serde_json::from_str(&text)?;
            let records = match v {
                Value::Array(a) => a,
                Value::Object(o) => match o.get("records") {
                    Some(Value::Array(a)) => a.clone(),
                    _ => return Err("no records array".into()),
                },
                _ => return Err("invalid dataset".into()),
            };
            let m = Metric {
                name: "adhoc".into(),
                field,
                agg: parse_agg(&agg)?,
                group_by,
                filter_field: None,
                filter_value: None,
            };
            let r = compute(&records, &m)?;
            let label = format!("{}({:?})", m.field, m.agg);
            print!("{}", render_widget(&label, &r));
        }
        Commands::Dashboard { file } => {
            let d = load_dashboard(&file)?;
            for (title, result) in run_dashboard(&d)? {
                print!("{}", render_widget(&title, &result));
            }
        }
        Commands::Inspect { dataset } => {
            let text = std::fs::read_to_string(&dataset)?;
            let v: Value = serde_json::from_str(&text)?;
            let records = match v {
                Value::Array(a) => a,
                Value::Object(o) => match o.get("records") {
                    Some(Value::Array(a)) => a.clone(),
                    _ => return Err("no records array".into()),
                },
                _ => return Err("invalid dataset".into()),
            };
            let mut fields = std::collections::BTreeSet::new();
            for r in &records {
                if let Some(o) = r.as_object() {
                    for k in o.keys() {
                        fields.insert(k.clone());
                    }
                }
            }
            println!("records: {}", records.len());
            println!(
                "fields: {}",
                fields.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(())
}
