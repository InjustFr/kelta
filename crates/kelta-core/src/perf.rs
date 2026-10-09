//! `perf_snapshot` (ARCHITECTURE §13): own process + WebKit helpers (Linux: `WebKit*` children;
//! macOS: bridge-provided pids), per-session model memory, live views, armed timers, HTTP server.

use kelta_proto::ipc::{PerfSnapshot, ProcMem, ProcRole, SessionMem};

use crate::Core;

fn role_for(name: &str) -> ProcRole {
    let n = name.to_ascii_lowercase();
    if n.contains("network") {
        ProcRole::Network
    } else if n.contains("gpu") {
        ProcRole::Gpu
    } else if n.contains("webkit") || n.contains("webcontent") {
        ProcRole::WebContent
    } else {
        ProcRole::Child
    }
}

#[cfg(target_os = "macos")]
mod os {
    /// `ri_phys_footprint` in KiB.
    pub fn footprint_kb(pid: u32) -> u64 {
        // SAFETY: `info` is a plain C struct sized for RUSAGE_INFO_V4; the kernel fills it.
        unsafe {
            let mut info: libc::rusage_info_v4 = std::mem::zeroed();
            let r = libc::proc_pid_rusage(
                pid as libc::c_int,
                libc::RUSAGE_INFO_V4,
                (&mut info as *mut libc::rusage_info_v4).cast::<libc::rusage_info_t>(),
            );
            if r == 0 { info.ri_phys_footprint / 1024 } else { 0 }
        }
    }

    pub fn name(pid: u32) -> String {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is valid for `buf.len()` bytes; proc_name writes at most that many.
        let n = unsafe { libc::proc_name(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n <= 0 { String::new() } else { String::from_utf8_lossy(&buf[..n as usize]).into_owned() }
    }

    pub fn webkit_children(_me: u32) -> Vec<u32> {
        Vec::new()
    }
}

#[cfg(not(target_os = "macos"))]
mod os {
    /// `Pss` of `/proc/<pid>/smaps_rollup` in KiB.
    pub fn footprint_kb(pid: u32) -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .ok()
            .and_then(|s| {
                s.lines().find_map(|l| {
                    l.strip_prefix("Pss:")
                        .and_then(|v| v.split_whitespace().next())
                        .and_then(|v| v.parse().ok())
                })
            })
            .unwrap_or(0)
    }

    pub fn name(pid: u32) -> String {
        std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|s| s.trim().to_owned()).unwrap_or_default()
    }

    /// Children of `me` whose comm starts with `WebKit`.
    pub fn webkit_children(me: u32) -> Vec<u32> {
        let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
        rd.flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
            .filter(|pid| {
                let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
                let Some(close) = stat.rfind(')') else { return false };
                let comm = stat.get(stat.find('(').map_or(0, |i| i + 1)..close).unwrap_or("");
                let ppid = stat[close + 1..].split_whitespace().nth(1).and_then(|p| p.parse::<u32>().ok());
                ppid == Some(me) && comm.starts_with("WebKit")
            })
            .collect()
    }
}

impl Core {
    /// `perf_snapshot`.
    pub fn perf_snapshot(&self) -> PerfSnapshot {
        let me = std::process::id();
        let mut processes = vec![ProcMem {
            pid: me,
            name: "kelta".into(),
            role: ProcRole::Core,
            pss_or_footprint_kb: os::footprint_kb(me),
        }];
        let mut helpers = self.bridge.webview_pids();
        helpers.extend(os::webkit_children(me));
        helpers.sort_unstable();
        helpers.dedup();
        for pid in helpers {
            let name = os::name(pid);
            processes.push(ProcMem {
                pid,
                role: role_for(&name),
                name,
                pss_or_footprint_kb: os::footprint_kb(pid),
            });
        }
        let stats = self.terminal.stats();
        let names: std::collections::HashMap<_, _> =
            self.sessions.lock().values().map(|e| (e.info.id.clone(), e.info.name.clone())).collect();
        let live_views = stats.sessions.iter().filter(|s| s.attached).count() as u32;
        let sessions = stats
            .sessions
            .into_iter()
            .map(|s| SessionMem {
                name: names.get(&s.id).cloned().unwrap_or_default(),
                id: s.id,
                history_lines: s.history_lines,
                model_bytes: s.memory_bytes,
                child_kb: None,
            })
            .collect();
        PerfSnapshot {
            processes,
            sessions,
            live_views,
            timers_armed: self.rt.timers.get(),
            http_server: self.http_refs.load(std::sync::atomic::Ordering::SeqCst) > 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_footprint_is_measured() {
        let me = std::process::id();
        assert!(os::footprint_kb(me) > 0 || !std::path::Path::new("/proc/self/smaps_rollup").exists());
        assert_eq!(role_for("WebKitNetworkProcess"), ProcRole::Network);
        assert_eq!(role_for("com.apple.WebKit.WebContent"), ProcRole::WebContent);
        assert_eq!(role_for("com.apple.WebKit.GPU"), ProcRole::Gpu);
    }
}
