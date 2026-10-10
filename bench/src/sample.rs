//! Memory and CPU samplers. The `/proc` parsers are pure and run against fixture trees in
//! `--dry-run` (on any OS); the macOS sampler uses `proc_pid_rusage`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mem {
    /// The kelta process alone.
    pub core_mb: f64,
    /// kelta + its WebKit helpers (children such as claude or nvim are excluded).
    pub total_mb: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cpu {
    pub cpu_secs: f64,
    /// Context switches (Linux) / package-idle + interrupt wakeups (macOS).
    pub wakeups: f64,
}

pub trait Probe {
    fn memory(&self) -> Result<Mem>;
    fn cpu(&self) -> Result<Cpu>;
}

/// `Pss: 1234 kB` from `/proc/<pid>/smaps_rollup`.
pub fn parse_pss_kb(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("Pss:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Voluntary + involuntary context switches from `/proc/<pid>/task/<tid>/status`.
pub fn parse_ctxt(text: &str) -> u64 {
    text.lines()
        .filter(|l| l.starts_with("voluntary_ctxt_switches:") || l.starts_with("nonvoluntary_ctxt_switches:"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .sum()
}

#[derive(Debug, PartialEq)]
pub struct Stat {
    pub comm: String,
    pub ppid: u32,
    /// utime + stime in clock ticks.
    pub ticks: u64,
}

/// `/proc/<pid>/stat`; `comm` may contain spaces and parentheses, so split on the last `)`.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let (open, close) = (text.find('(')?, text.rfind(')')?);
    let comm = text.get(open + 1..close)?.to_owned();
    let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // rest[0] = state, [1] = ppid, [11] = utime, [12] = stime
    Some(Stat {
        comm,
        ppid: rest.get(1)?.parse().ok()?,
        ticks: rest.get(11)?.parse::<u64>().ok()? + rest.get(12)?.parse::<u64>().ok()?,
    })
}

pub fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    match values.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => values[n / 2],
        n => (values[n / 2 - 1] + values[n / 2]) / 2.0,
    }
}

pub struct ProcFs {
    pub root: PathBuf,
    pub pid: u32,
    /// Clock ticks per second (`sysconf(_SC_CLK_TCK)`, 100 in the fixtures).
    pub hz: f64,
}

impl ProcFs {
    fn read(&self, rel: &str) -> Result<String> {
        let p = self.root.join(rel);
        std::fs::read_to_string(&p).with_context(|| p.display().to_string())
    }

    fn pss_mb(&self, pid: u32) -> Result<f64> {
        let text = self.read(&format!("{pid}/smaps_rollup"))?;
        Ok(parse_pss_kb(&text).with_context(|| format!("no Pss in {pid}/smaps_rollup"))? as f64 / 1024.0)
    }

    fn webkit_children(&self) -> Vec<u32> {
        let Ok(rd) = std::fs::read_dir(&self.root) else { return Vec::new() };
        rd.filter_map(|e| {
            let pid: u32 = e.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = parse_stat(&self.read(&format!("{pid}/stat")).ok()?)?;
            (stat.ppid == self.pid && stat.comm.starts_with("WebKit")).then_some(pid)
        })
        .collect()
    }
}

impl Probe for ProcFs {
    fn memory(&self) -> Result<Mem> {
        let core_mb = self.pss_mb(self.pid)?;
        let mut total_mb = core_mb;
        for pid in self.webkit_children() {
            total_mb += self.pss_mb(pid)?;
        }
        Ok(Mem { core_mb, total_mb })
    }

    fn cpu(&self) -> Result<Cpu> {
        let stat = parse_stat(&self.read(&format!("{}/stat", self.pid))?).context("unparsable stat")?;
        let mut wakeups = 0;
        let tasks = self.root.join(self.pid.to_string()).join("task");
        for t in std::fs::read_dir(&tasks).with_context(|| tasks.display().to_string())?.flatten() {
            // A thread may exit between listing and reading.
            if let Ok(text) = std::fs::read_to_string(t.path().join("status")) {
                wakeups += parse_ctxt(&text);
            }
        }
        Ok(Cpu { cpu_secs: stat.ticks as f64 / self.hz, wakeups: wakeups as f64 })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn clock_ticks() -> f64 {
    // SAFETY: sysconf has no preconditions.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz > 0 { hz as f64 } else { 100.0 }
}

#[cfg(target_os = "macos")]
pub use mac::Mach;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;

    unsafe extern "C" {
        // libsystem (what Activity Monitor uses): WebKit XPC helpers are parented to launchd but
        // "responsible" to the app that started them.
        fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
    }

    /// `phys_footprint` of kelta + the `com.apple.WebKit.*` processes it is responsible for.
    pub struct Mach {
        pub pid: u32,
    }

    fn usage(pid: i32) -> Result<libc::rusage_info_v4> {
        // SAFETY: zeroed POD out-parameter, filled by the kernel for the V4 flavor.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let rc =
            unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, std::ptr::from_mut(&mut ri).cast()) };
        if rc != 0 {
            anyhow::bail!("proc_pid_rusage({pid}) failed");
        }
        Ok(ri)
    }

    fn name(pid: i32) -> String {
        let mut buf = [0u8; 256];
        // SAFETY: buffer and length match.
        let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned()
    }

    fn all_pids() -> Vec<i32> {
        // SAFETY: a null buffer asks for the byte count; the second call fills our buffer.
        let bytes = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let mut pids = vec![0i32; usize::try_from(bytes).unwrap_or(0) / 4 + 64];
        let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
        pids.truncate(usize::try_from(got).unwrap_or(0));
        pids
    }

    /// WebKit helpers of `app` (other WebKit apps, the user's own Kelta included, are excluded).
    fn webkit_pids(app: i32) -> Vec<i32> {
        all_pids()
            .into_iter()
            // SAFETY: plain pid query, no pointers.
            .filter(|p| unsafe { responsibility_get_pid_responsible_for_pid(*p) } == app)
            .filter(|p| name(*p).contains("WebKit"))
            .collect()
    }

    impl Probe for Mach {
        fn memory(&self) -> Result<Mem> {
            let mb = |ri: libc::rusage_info_v4| ri.ri_phys_footprint as f64 / 1_048_576.0;
            let pid = i32::try_from(self.pid)?;
            let core_mb = mb(usage(pid)?);
            let helpers: f64 = webkit_pids(pid).into_iter().filter_map(|p| usage(p).ok()).map(mb).sum();
            Ok(Mem { core_mb, total_mb: core_mb + helpers })
        }

        // shortcut: libc marks mach_timebase_info deprecated (mach2 is not a dependency); upgrade if it is removed.
        #[allow(deprecated)]
        fn cpu(&self) -> Result<Cpu> {
            let ri = usage(i32::try_from(self.pid)?)?;
            let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: out-parameter is a valid struct.
            unsafe { libc::mach_timebase_info(&mut tb) };
            let ns = (ri.ri_user_time + ri.ri_system_time) as f64 * f64::from(tb.numer)
                / f64::from(tb.denom.max(1));
            Ok(Cpu { cpu_secs: ns / 1e9, wakeups: (ri.ri_pkg_idle_wkups + ri.ri_interrupt_wkups) as f64 })
        }
    }
}

/// Fixture `/proc` for dry runs.
pub fn fixture_proc(fixtures: &Path) -> ProcFs {
    ProcFs { root: fixtures.join("proc"), pid: 4000, hz: 100.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn parses_proc_files() {
        assert_eq!(parse_pss_kb("Rss: 9 kB\nPss:   2048 kB\n"), Some(2048));
        assert_eq!(parse_pss_kb("nothing"), None);
        assert_eq!(parse_ctxt("voluntary_ctxt_switches:\t5\nnonvoluntary_ctxt_switches:\t2\n"), 7);
        let s = parse_stat("12 (we (ird) x) S 7 12 12 0 -1 0 0 0 0 0 30 12 0 0").unwrap();
        assert_eq!(s, Stat { comm: "we (ird) x".into(), ppid: 7, ticks: 42 });
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn fixture_tree_counts_kelta_and_webkit_only() {
        let m = fixture_proc(&fixtures()).memory().unwrap();
        assert!((m.core_mb - 41000.0 / 1024.0).abs() < 1e-9);
        // 41000 + 110000 + 18000 kB; the nvim child (25000 kB) is excluded.
        assert!((m.total_mb - 169000.0 / 1024.0).abs() < 1e-9);
        let c = fixture_proc(&fixtures()).cpu().unwrap();
        assert_eq!((c.cpu_secs, c.wakeups), (7.0, 124.0));
    }

    #[test]
    fn median_of_odd_and_even() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    }
}
