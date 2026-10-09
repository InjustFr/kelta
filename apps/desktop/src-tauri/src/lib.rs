//! Kelta desktop shell: Tauri v2 over `kelta-core` (ARCHITECTURE §2, §3).
//!
//! Scaffold-owned composition (`run`, plugin registration, URI scheme, invoke handler).
//! `commands/<domain>.rs`, `platform/` and `window/` are owned by lanes.

pub mod commands;
pub mod platform;
pub mod window;

use std::process::ExitCode;
use std::sync::Arc;

use kelta_core::{Core, CoreConfig};
use kelta_proto::dirs::{CliArgs, Dirs, DirsOverrides};
use tauri::Manager;

use crate::window::bridge::TauriBridge;

/// Tokio runtime per ARCHITECTURE §2: 2 workers, ≤ 8 blocking threads, 1 MiB stacks.
fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .thread_stack_size(1024 * 1024)
        .thread_name("kelta-rt")
        .enable_all()
        .build()
}

fn init_tracing() {
    // File logging (size-capped, `info` default, no stdout in release) is wired by L10/L3;
    // debug builds log to stderr.
    if cfg!(debug_assertions) {
        let filter = tracing_subscriber::EnvFilter::try_from_env("KELTA_LOG")
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
    }
}

// allowlisted: `tauri::generate_context!` expands to code calling `std::process::exit`.
#[allow(clippy::disallowed_methods)]
fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

/// Entry point of the `kelta` binary.
pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pre = platform::pre_init(&args);
    init_tracing();

    let mut cli = CliArgs::parse(&args);
    cli.safe_graphics |= pre.safe_graphics;
    let dirs =
        match Dirs::from_process_env(&DirsOverrides { config: cli.config_dir.clone(), ..Default::default() })
        {
            Ok(d) => d,
            Err(e) => {
                eprintln!("kelta: cannot resolve directories: {e}");
                return ExitCode::FAILURE;
            }
        };

    let runtime = match build_runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("kelta: cannot start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    tauri::async_runtime::set(runtime.handle().clone());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            window::on_second_instance(app, argv, cwd);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("kelta-plugin", |ctx, request, responder| {
            let response = match ctx.app_handle().try_state::<Arc<Core>>() {
                Some(core) => kelta_plugins::uri::handle(core.plugins(), request),
                None => {
                    let mut r = tauri::http::Response::new(b"core not ready".to_vec());
                    *r.status_mut() = tauri::http::StatusCode::SERVICE_UNAVAILABLE;
                    r
                }
            };
            responder.respond(response);
        })
        .setup(move |app| {
            let bridge = TauriBridge::new(app.handle().clone());
            let core = Core::start(CoreConfig { dirs, cli, bridge: bridge.clone() })?;
            app.manage(bridge);
            app.manage(core.clone());
            window::setup(app, core)?;
            Ok(())
        })
        .on_window_event(window::on_window_event)
        .invoke_handler(commands::handler())
        .build(context());

    match app {
        Ok(app) => {
            app.run(|handle, event| window::on_run_event(handle, &event));
            drop(runtime);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kelta: {e}");
            ExitCode::FAILURE
        }
    }
}
