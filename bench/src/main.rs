//! kelta-bench (L10): scenarios idle-3p10s, background, cold-start, switch, echo-latency,
//! ink-redraw, flood, idle-cpu; PSS / phys_footprint samplers; budgets + baseline comparison.
//!
//! SCAFFOLD STUB: prints "not implemented" and exits 1 (except `--help`).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: kelta-bench --scenario <name> [--dry-run]");
        return ExitCode::SUCCESS;
    }
    eprintln!("kelta-bench: not implemented");
    ExitCode::FAILURE
}
