//! Kelta desktop shell: Tauri v2 over `kelta-core` (ARCHITECTURE §2, §3).
//!
//! Scaffold-owned composition (`run`, plugin registration, URI scheme, invoke handler).
//! `commands/<domain>.rs`, `platform/` and `window/` are owned by lanes.

pub mod commands;
pub mod platform;
pub mod window;

use std::process::ExitCode;

use kelta_core::{Core, CoreDeps};
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

/// Past this size at launch, `kelta.log` becomes `kelta.log.1` (ARCHITECTURE §2.1: 5 MB × 2).
const LOG_CAP: u64 = 5 * 1024 * 1024;

/// `<logs>/kelta.log` beside `keltad.log` (macOS `~/Library/Logs/Kelta`), `info` by default
/// (`KELTA_LOG`); debug builds also log to stderr.
fn init_tracing(logs: &std::path::Path) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let filter = tracing_subscriber::EnvFilter::try_from_env("KELTA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let path = logs.join("kelta.log");
    // shortcut: rotated at launch only; rotate while running if one run can outgrow LOG_CAP.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_CAP) {
        let _ = std::fs::rename(&path, logs.join("kelta.log.1"));
    }
    let file = std::fs::create_dir_all(logs)
        .and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(&path));
    let fmt = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    let _ = match (file, cfg!(debug_assertions)) {
        (Ok(f), true) => fmt.with_writer(std::io::stderr.and(std::sync::Mutex::new(f))).try_init(),
        (Ok(f), false) => fmt.with_writer(std::sync::Mutex::new(f)).try_init(),
        (Err(e), _) => {
            eprintln!("kelta: cannot open {}: {e}", path.display());
            fmt.with_writer(std::io::stderr).try_init()
        }
    };
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
    init_tracing(&dirs.logs);

    let runtime = match build_runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("kelta: cannot start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    tauri::async_runtime::set(runtime.handle().clone());

    let mut builder = tauri::Builder::default();
    // An isolated instance (own runtime dir) must not forward to, or receive from, the user's Kelta.
    if std::env::var_os("KELTA_RUNTIME_DIR").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            window::on_second_instance(app, argv, cwd);
        }));
    }
    let ctx = context();
    // Only the release overlay configures `plugins.updater`: dev and debug builds never update.
    if platform::update::enabled(ctx.config(), cfg!(debug_assertions)) {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    let app = builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("kelta-plugin", |ctx, request, responder| {
            let Some(core) = window::core(ctx.app_handle()) else {
                let mut r = tauri::http::Response::new(b"core not ready".to_vec());
                *r.status_mut() = tauri::http::StatusCode::SERVICE_UNAVAILABLE;
                return responder.respond(r);
            };
            // Called on the webview main thread: the file reads must not block it.
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(kelta_plugins::uri::handle(core.plugins(), request));
            });
        })
        .setup(move |app| {
            let bridge = TauriBridge::new(app.handle().clone());
            let deps = CoreDeps::new(dirs, cli, bridge.clone());
            #[cfg(target_os = "linux")]
            let deps = CoreDeps {
                clipboard: Some(std::sync::Arc::new(platform::clipboard::GtkClipboard::new(
                    app.handle().clone(),
                ))),
                ..deps
            };
            let core = Core::start_with(deps)?;
            app.manage(bridge);
            app.manage(core.clone());
            window::setup(app, core)?;
            Ok(())
        })
        .on_window_event(window::on_window_event)
        .invoke_handler(commands::handler())
        .build(ctx);

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

#[cfg(test)]
mod tests {
    #[test]
    fn launch_writes_the_app_log() {
        let tmp = tempfile::tempdir().unwrap();
        let logs = tmp.path().join("Logs/Kelta");
        super::init_tracing(&logs);
        tracing::info!("kelta started");
        let text = std::fs::read_to_string(logs.join("kelta.log")).unwrap();
        assert!(text.contains("kelta started"), "{text}");
    }
}
