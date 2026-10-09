//! kelta-bench (L10): scenarios idle-3p10s, background, cold-start, switch, echo-latency,
//! ink-redraw, flood, idle-cpu; PSS / phys_footprint samplers; budgets + baseline comparison.
//!
//! Memory and CPU scenarios sample the process tree themselves. Scenarios that need in-app timing
//! (cold-start, switch, echo-latency, ink-redraw, flood) read the metrics the app writes to the
//! JSON file named by `KELTA_BENCH_MARKS` (see docs/contract-requests/L10.md).

mod budget;
mod run;
mod sample;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use budget::{Baseline, Budgets, Metrics};

pub const SCENARIOS: [&str; 8] =
    ["idle-3p10s", "background", "cold-start", "switch", "echo-latency", "ink-redraw", "flood", "idle-cpu"];

const USAGE: &str = "usage: kelta-bench --scenario <name> [--dry-run] [--app PATH] [--settle SECS] \
[--duration SECS] [--fixtures DIR] [--budgets FILE] [--baseline FILE] [--update-baseline]\n\
scenarios: idle-3p10s background cold-start switch echo-latency ink-redraw flood idle-cpu";

pub struct Opts {
    pub scenario: String,
    pub dry_run: bool,
    pub update_baseline: bool,
    pub app: PathBuf,
    pub settle_secs: u64,
    pub duration_secs: u64,
    pub fixtures: PathBuf,
    pub budgets: PathBuf,
    pub baseline: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Opts> {
    let bench = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut o = Opts {
        scenario: String::new(),
        dry_run: false,
        update_baseline: false,
        app: bench.join("../target/release/kelta"),
        settle_secs: 30,
        duration_secs: 120,
        fixtures: bench.join("fixtures"),
        budgets: bench.join("budgets.toml"),
        // macOS sums phys_footprint, Linux sums PSS: each OS regresses against its own numbers.
        baseline: bench.join(if cfg!(target_os = "macos") {
            "fixtures/baseline-macos.json"
        } else {
            "fixtures/baseline-linux.json"
        }),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--scenario" => o.scenario = val()?.clone(),
            "--dry-run" => o.dry_run = true,
            "--update-baseline" => o.update_baseline = true,
            "--app" => o.app = val()?.into(),
            "--settle" => o.settle_secs = val()?.parse()?,
            "--duration" => o.duration_secs = val()?.parse()?,
            "--fixtures" => o.fixtures = val()?.into(),
            "--budgets" => o.budgets = val()?.into(),
            "--baseline" => o.baseline = val()?.into(),
            other => bail!("unknown argument {other}"),
        }
    }
    if !SCENARIOS.contains(&o.scenario.as_str()) {
        bail!("unknown or missing --scenario");
    }
    Ok(o)
}

/// Dry run: the idle-3p10s sampler reads the fixture `/proc` tree, the others read canned metrics.
fn dry_metrics(o: &Opts) -> Result<Metrics> {
    use sample::Probe;
    if o.scenario == "idle-3p10s" {
        let m = sample::fixture_proc(&o.fixtures).memory()?;
        return Ok([("footprint_mb".to_owned(), m.total_mb), ("core_mb".to_owned(), m.core_mb)].into());
    }
    let path = o.fixtures.join("dry").join(format!("{}.json", o.scenario));
    serde_json::from_str(&std::fs::read_to_string(&path).with_context(|| path.display().to_string())?)
        .with_context(|| path.display().to_string())
}

async fn run(o: &Opts) -> Result<bool> {
    let budgets: Budgets = toml::from_str(&std::fs::read_to_string(&o.budgets).context("budgets.toml")?)?;
    let mut baseline: Baseline = match std::fs::read_to_string(&o.baseline) {
        Ok(text) => serde_json::from_str(&text).with_context(|| o.baseline.display().to_string())?,
        Err(_) => Baseline::new(),
    };
    let metrics = if o.dry_run { dry_metrics(o)? } else { run::measure(o).await? };
    let rows = budget::evaluate(&o.scenario, &metrics, &budgets, &baseline, cfg!(target_os = "macos"));
    println!("scenario {}{}", o.scenario, if o.dry_run { " (dry run)" } else { "" });
    for r in &rows {
        let limit = r.limit.map_or("-".into(), |l| l.to_string());
        let base = r.baseline.map_or("-".into(), |b| b.to_string());
        let verdict = r.failure.as_deref().map_or("ok".to_owned(), |f| format!("FAIL: {f}"));
        println!(
            "  {:<24} {:>10.2}  budget {:<8} baseline {:<8} {}",
            r.metric, r.value, limit, base, verdict
        );
    }
    if o.update_baseline && !o.dry_run {
        baseline.insert(o.scenario.clone(), metrics);
        std::fs::write(&o.baseline, serde_json::to_string_pretty(&baseline)? + "\n")?;
    }
    Ok(rows.iter().all(|r| r.failure.is_none()))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("kelta-bench: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&opts).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("kelta-bench: {e:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_baseline_is_per_os() {
        let o = super::parse_args(&["--scenario".into(), "flood".into()]).unwrap();
        let name = if cfg!(target_os = "macos") { "baseline-macos.json" } else { "baseline-linux.json" };
        assert!(o.baseline.ends_with(format!("fixtures/{name}")));
    }
}
