//! kelta-ctl (L7): tiny CLI used by Claude hooks, compositor keybindings and scripts (SPEC §8).
//!
//! Depends only on std + serde + serde_json + rustix. `hook` ALWAYS exits 0.
//!
//! SCAFFOLD STUB: prints "not implemented"; `hook` exits 0, other commands exit 1.

use std::process::ExitCode;

const USAGE: &str = "usage: kelta-ctl <toggle|palette|open <path>|focus-project <id>|start <ticket> [--project <id>]|\
new --template <id> [--cwd <dir>] [--project <id>]|emit <custom.event> --json <json>|trust <repo>|\
editor-open <file>[:line]|plugin install <src>|hook|version>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            eprintln!("kelta-ctl hook: not implemented");
            ExitCode::SUCCESS
        }
        Some("version") | Some("--version") => {
            println!("kelta-ctl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(cmd) => {
            eprintln!("kelta-ctl {cmd}: not implemented");
            ExitCode::FAILURE
        }
    }
}
