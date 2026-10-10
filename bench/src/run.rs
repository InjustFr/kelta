//! Real runs: launch the app against the reference fixture and measure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use kelta_proto::dirs::{Dirs, DirsOverrides};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::Opts;
use crate::budget::Metrics;
use crate::sample::{Probe, median};

/// How long the app gets to write its first marks.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long background mode waits for the WebKit helpers to exit before sampling anyway.
const HELPERS_EXIT: Duration = Duration::from_secs(180);

struct App {
    pid: i32,
    tmp: PathBuf,
    marks: PathBuf,
    ctl: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        // SAFETY: our own child; waitpid reaps it so the next launch does not forward to it.
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            libc::waitpid(self.pid, std::ptr::null_mut(), 0);
        }
        // Session children that outlive a killed app (`nvim --embed --listen <tmp>/…` ignores SIGTERM).
        // shortcut: matches on the temp path in argv only; upgrade to a process group if one escapes.
        let _ = std::process::Command::new("pkill").args(["-9", "-f"]).arg(&self.tmp).status();
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
    // Kelta opens git repositories only: copy the fixture projects out of the source tree and init each.
    for p in ["a", "b", "c"] {
        let dir = tmp.join("projects").join(p);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(o.fixtures.join("3p10s/projects").join(p).join("README.md"), dir.join("README.md"))?;
        let git = |args: &[&str]| std::process::Command::new("git").current_dir(&dir).args(args).output();
        git(&["init", "-q"])?;
        git(&["add", "."])?;
        git(&["-c", "user.name=bench", "-c", "user.email=bench@localhost", "commit", "-qm", "fixture"])?;
    }
    let marks = tmp.join("marks.json");
    let home = tmp.join("home");
    std::fs::create_dir_all(&home)?;

    let env: BTreeMap<String, String> = [
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        // macOS: CF/WebKit read the home from CFFIXED_USER_HOME, not HOME.
        ("CFFIXED_USER_HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("KELTA_RUNTIME_DIR".to_owned(), tmp.join("run").to_string_lossy().into_owned()),
    ]
    .into();
    std::fs::create_dir_all(tmp.join("run"))?;
    let dirs = Dirs::resolve(&env, &DirsOverrides { config: Some(config.clone()), ..Default::default() })?;

    // tui-sim sits next to the bench binary.
    let bin_dir = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
    let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());

    // Drop inherited KELTA_*/XDG_* (the bench may run inside a Kelta session): they point at the user's instance.
    let mut full: BTreeMap<String, String> =
        std::env::vars().filter(|(k, _)| !k.starts_with("KELTA_") && !k.starts_with("XDG_")).collect();
    full.extend(env);
    full.insert("PATH".into(), path);
    full.insert("KELTA_BENCH".into(), "1".into());
    full.insert("KELTA_BENCH_MARKS".into(), marks.to_string_lossy().into_owned());
    let argv =
        [o.app.to_string_lossy().into_owned(), "--config-dir".into(), config.to_string_lossy().into_owned()];
    let pid = spawn(&argv, &full).with_context(|| format!("cannot start {}", o.app.display()))?;
    Ok(App { pid, tmp, marks, ctl: dirs.ctl_socket() })
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    // libsystem, private but stable (Terminal and Xcode use it).
    fn responsibility_spawnattrs_setdisclaim(
        attr: *mut libc::posix_spawnattr_t,
        disclaim: libc::c_int,
    ) -> libc::c_int;
}

/// posix_spawn with stdio on /dev/null. On macOS the app disclaims responsibility, so the WebKit
/// helpers it starts are attributed to it (the sampler's filter) and not to whatever app the bench runs in.
fn spawn(argv: &[String], env: &BTreeMap<String, String>) -> Result<i32> {
    use std::ffi::CString;
    let cs = |s: String| CString::new(s).context("NUL in argv/env");
    let argv: Vec<CString> = argv.iter().cloned().map(cs).collect::<Result<_>>()?;
    let envp: Vec<CString> = env.iter().map(|(k, v)| cs(format!("{k}={v}"))).collect::<Result<_>>()?;
    let ptrs = |v: &[CString]| {
        v.iter().map(|c| c.as_ptr().cast_mut()).chain([std::ptr::null_mut()]).collect::<Vec<_>>()
    };
    let (argp, envpp) = (ptrs(&argv), ptrs(&envp));
    let null = CString::new("/dev/null")?;
    let mut pid = 0;
    // SAFETY: attr/actions are initialised before use and destroyed after; argv/envp are NULL-terminated
    // arrays of live CStrings.
    let rc = unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        let mut fa: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawn_file_actions_init(&mut fa);
        for fd in 0..3 {
            let flags = if fd == 0 { libc::O_RDONLY } else { libc::O_WRONLY };
            libc::posix_spawn_file_actions_addopen(&mut fa, fd, null.as_ptr(), flags, 0);
        }
        #[cfg(target_os = "macos")]
        responsibility_spawnattrs_setdisclaim(&mut attr, 1);
        let rc = libc::posix_spawn(&mut pid, argv[0].as_ptr(), &fa, &attr, argp.as_ptr(), envpp.as_ptr());
        libc::posix_spawn_file_actions_destroy(&mut fa);
        libc::posix_spawnattr_destroy(&mut attr);
        rc
    };
    if rc != 0 {
        return Err(std::io::Error::from_raw_os_error(rc).into());
    }
    Ok(pid)
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

/// Connects to the app's control socket, waiting until core has bound it.
async fn connect_ctl(app: &App) -> Result<UnixStream> {
    let start = Instant::now();
    loop {
        match UnixStream::connect(&app.ctl).await {
            Ok(s) => return Ok(s),
            Err(e) if start.elapsed() > READY_TIMEOUT => {
                return Err(e).with_context(|| app.ctl.display().to_string());
            }
            // one-shot: bench harness poll, outside the app
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

/// Sends the fixture setup (3 projects, 10 sessions) over the control socket.
async fn setup(app: &App, o: &Opts) -> Result<()> {
    let text = std::fs::read_to_string(o.fixtures.join("3p10s/setup.jsonl"))?;
    let lines =
        fill(&text.replace("@FIXTURES@/3p10s/projects", &app.tmp.join("projects").to_string_lossy()), o);
    let stream = connect_ctl(app).await?;
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
    let pid = u32::try_from(app.pid)?;
    #[cfg(target_os = "macos")]
    return Ok(Box::new(crate::sample::Mach { pid }));
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
            setup(&app, o).await?;
            settle(o).await;
            let m = probe(&app)?.memory()?;
            Ok([("footprint_mb".to_owned(), m.total_mb), ("core_mb".to_owned(), m.core_mb)].into())
        }
        "background" => {
            let app = launch(o).await?;
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
            // ARCH §13: background mode is measured once the webview's WebKit processes are gone; the
            // GPU process idles out a minute or two after its last page.
            let (p, start) = (probe(&app)?, Instant::now());
            let mut m = p.memory()?;
            while m.total_mb > m.core_mb && start.elapsed() < HELPERS_EXIT {
                // one-shot: bench harness poll, outside the app
                tokio::time::sleep(Duration::from_secs(1)).await;
                m = p.memory()?;
            }
            Ok([("background_mb".to_owned(), m.total_mb)].into())
        }
        "idle-cpu" => {
            let app = launch(o).await?;
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
                let app = launch(o).await?;
                for (k, v) in wait_mark(&app, "app_ready_ms").await? {
                    runs.entry(k).or_default().push(v);
                }
                // Wait for the exit: the next launch would otherwise forward to this instance.
                drop(app);
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
