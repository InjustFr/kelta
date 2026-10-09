//! Real runs: launch the app against the reference fixture and measure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use kelta_proto::dirs::{Dirs, DirsOverrides};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

use crate::Opts;
use crate::budget::Metrics;
use crate::sample::{Probe, median};

/// How long the app gets to write its first marks.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

struct App {
    child: Child,
    tmp: PathBuf,
    marks: PathBuf,
    ctl: PathBuf,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // only the macOS sampler reads it
    before: std::collections::HashSet<i32>,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

fn root(o: &Opts) -> PathBuf {
    o.fixtures.join("../..")
}

/// Substitutes `@FIXTURES@` / `@ROOT@` in a fixture text file.
fn fill(text: &str, o: &Opts) -> String {
    text.replace("@FIXTURES@", &o.fixtures.to_string_lossy()).replace("@ROOT@", &root(o).to_string_lossy())
}

async fn launch(o: &Opts) -> Result<App> {
    // Short path: unix socket paths are limited to ~100 bytes.
    let tmp = PathBuf::from(format!("/tmp/kb-{}", std::process::id()));
    let config = tmp.join("config");
    std::fs::create_dir_all(&config)?;
    let cfg = std::fs::read_to_string(o.fixtures.join("3p10s/config/config.toml"))?;
    std::fs::write(config.join("config.toml"), fill(&cfg, o))?;
    let marks = tmp.join("marks.json");
    let home = tmp.join("home");
    std::fs::create_dir_all(&home)?;

    let env: BTreeMap<String, String> = [
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("XDG_RUNTIME_DIR".to_owned(), tmp.join("run").to_string_lossy().into_owned()),
    ]
    .into();
    std::fs::create_dir_all(tmp.join("run"))?;
    let dirs = Dirs::resolve(&env, &DirsOverrides { config: Some(config.clone()), ..Default::default() })?;

    // tui-sim sits next to the bench binary.
    let bin_dir = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
    let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());
    #[cfg(target_os = "macos")]
    let before = crate::sample::webkit_pids();
    #[cfg(not(target_os = "macos"))]
    let before = Default::default();

    let child = Command::new(&o.app)
        .arg("--config-dir")
        .arg(&config)
        .envs(&env)
        .env("PATH", path)
        .env("KELTA_BENCH", "1")
        .env("KELTA_BENCH_MARKS", &marks)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("cannot start {}", o.app.display()))?;
    Ok(App { child, tmp, marks, ctl: dirs.ctl_socket(), before })
}

fn read_marks(path: &Path) -> Metrics {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Waits until the marks file holds `key`.
async fn wait_mark(app: &App, key: &str) -> Result<Metrics> {
    let start = Instant::now();
    loop {
        let marks = read_marks(&app.marks);
        if marks.contains_key(key) {
            return Ok(marks);
        }
        if start.elapsed() > READY_TIMEOUT {
            bail!("the app did not report `{key}` within {READY_TIMEOUT:?}");
        }
        // one-shot: bench harness poll, outside the app
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Sends the fixture setup (3 projects, 10 sessions) over the control socket.
async fn setup(app: &App, o: &Opts) -> Result<()> {
    let lines = fill(&std::fs::read_to_string(o.fixtures.join("3p10s/setup.jsonl"))?, o);
    let stream = UnixStream::connect(&app.ctl).await.with_context(|| app.ctl.display().to_string())?;
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd).lines();
    for line in lines.lines().filter(|l| !l.trim().is_empty()) {
        let req = format!("{{\"v\":1,{}\n", line.trim_start().trim_start_matches('{'));
        wr.write_all(req.as_bytes()).await?;
        let reply = rd.next_line().await?.context("ctl socket closed")?;
        if reply.contains("\"ok\":false") {
            bail!("setup command failed: {line}: {reply}");
        }
    }
    Ok(())
}

fn probe(app: &App) -> Result<Box<dyn Probe>> {
    let pid = app.child.id().context("app already exited")?;
    #[cfg(target_os = "macos")]
    return Ok(Box::new(crate::sample::Mach { pid, before: app.before.clone() }));
    #[cfg(not(target_os = "macos"))]
    Ok(Box::new(crate::sample::ProcFs { root: "/proc".into(), pid, hz: crate::sample::clock_ticks() }))
}

async fn settle(o: &Opts) {
    // one-shot: let the reference workload go idle before sampling
    tokio::time::sleep(Duration::from_secs(o.settle_secs)).await;
}

pub async fn measure(o: &Opts) -> Result<Metrics> {
    match o.scenario.as_str() {
        "idle-3p10s" => {
            let app = launch(o).await?;
            wait_mark(&app, "app_ready_ms").await?;
            setup(&app, o).await?;
            settle(o).await;
            let m = probe(&app)?.memory()?;
            Ok([("footprint_mb".to_owned(), m.total_mb), ("core_mb".to_owned(), m.core_mb)].into())
        }
        "background" => {
            let app = launch(o).await?;
            wait_mark(&app, "app_ready_ms").await?;
            setup(&app, o).await?;
            settle(o).await;
            // Bench hook (window::bridge): closes the window as the user would; sessions stay alive.
            let stream = UnixStream::connect(&app.ctl).await?;
            let (_, mut wr) = stream.into_split();
            wr.write_all(
                b"{\"v\":1,\"cmd\":\"emit\",\"name\":\"custom.bench.close_window\",\"payload\":null}\n",
            )
            .await?;
            wait_mark(&app, "window_closed").await?;
            settle(o).await;
            let m = probe(&app)?.memory()?;
            Ok([("background_mb".to_owned(), m.total_mb)].into())
        }
        "idle-cpu" => {
            let app = launch(o).await?;
            wait_mark(&app, "app_ready_ms").await?;
            setup(&app, o).await?;
            settle(o).await;
            let p = probe(&app)?;
            let (a, t0) = (p.cpu()?, Instant::now());
            // one-shot: the measurement window itself
            tokio::time::sleep(Duration::from_secs(o.duration_secs)).await;
            let (b, secs) = (p.cpu()?, t0.elapsed().as_secs_f64());
            Ok([
                ("cpu_pct".to_owned(), (b.cpu_secs - a.cpu_secs) / secs * 100.0),
                ("wakeups_per_s".to_owned(), (b.wakeups - a.wakeups) / secs),
            ]
            .into())
        }
        "cold-start" => {
            let mut runs: BTreeMap<String, Vec<f64>> = BTreeMap::new();
            for _ in 0..5 {
                let mut app = launch(o).await?;
                for (k, v) in wait_mark(&app, "app_ready_ms").await? {
                    runs.entry(k).or_default().push(v);
                }
                // Wait for the exit: the next launch would otherwise forward to this instance.
                app.child.kill().await?;
            }
            let ready = runs.remove("app_ready_ms").unwrap_or_default();
            Ok([("cold_start_ms".to_owned(), median(&mut ready.clone()))].into())
        }
        // The app measures these itself (UI timestamps vs Rust timestamps) and reports them when
        // the scenario's last metric appears.
        name @ ("switch" | "echo-latency" | "ink-redraw" | "flood") => {
            let app = launch(o).await?;
            wait_mark(&app, "app_ready_ms").await?;
            setup(&app, o).await?;
            let done = match name {
                "switch" => "switch_cold_ms",
                "echo-latency" => "keydown_to_pty_p99_ms",
                "ink-redraw" => "frame_p95_ms",
                _ => "inflight_kib",
            };
            let mut marks = wait_mark(&app, done).await?;
            marks.remove("app_ready_ms");
            Ok(marks)
        }
        other => bail!("unknown scenario {other}"),
    }
}
