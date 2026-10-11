//! In-app updates (ARCHITECTURE §2 "Bundling"): tauri-plugin-updater driven from Rust. The channel
//! comes from the running version. Checks run after `app_ready` and on window focus (6 h throttle),
//! never on a timer. An update found is installed right away and runs from the next start.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kelta_proto::api::SettingsSource;
use kelta_proto::ipc::{UpdateChannel, UpdateState, UpdateStatus};
use parking_lot::Mutex;
use semver::Version;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

const NIGHTLY_URL: &str = "https://github.com/InjustFr/kelta/releases/download/nightly/latest.json";
/// GitHub's "latest" skips prereleases, so it never serves a nightly.
const RELEASE_URL: &str = "https://github.com/InjustFr/kelta/releases/latest/download/latest.json";
/// Focus checks wait this long after the previous check.
const THROTTLE: Duration = Duration::from_secs(6 * 3600);

struct Inner {
    state: UpdateState,
    checked_at: Option<String>,
    /// Start of the last check (throttle; memory only).
    last: Option<Instant>,
}

static STATE: Mutex<Inner> =
    parking_lot::const_mutex(Inner { state: UpdateState::Idle, checked_at: None, last: None });
static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn channel(version: &str) -> UpdateChannel {
    if version.contains("-nightly.") { UpdateChannel::Nightly } else { UpdateChannel::Release }
}

pub fn endpoint(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Nightly => NIGHTLY_URL,
        UpdateChannel::Release => RELEASE_URL,
    }
}

/// The manifest's version is worth installing. Nightly: the manifest is the build to run, so any
/// version other than the running one (or the one already installed). Release: a higher one.
pub fn wanted(
    channel: UpdateChannel,
    current: &Version,
    installed: Option<&Version>,
    remote: &Version,
) -> bool {
    match channel {
        UpdateChannel::Nightly => remote != current && Some(remote) != installed,
        UpdateChannel::Release => remote > current && installed.is_none_or(|i| remote > i),
    }
}

/// An automatic check is due: `app.updates` on and none in the last 6 h.
pub fn due(updates: bool, last: Option<Instant>, now: Instant) -> bool {
    updates && last.is_none_or(|t| now.saturating_duration_since(t) >= THROTTLE)
}

/// The app runs from a place the updater can replace: not the dmg, not an App Translocation copy.
pub fn writable_bundle(exe: &Path) -> bool {
    let p = exe.to_string_lossy();
    !p.starts_with("/Volumes/") && !p.contains("/AppTranslocation/")
}

/// The updater plugin is registered: release overlay (`plugins.updater`), not a debug build.
pub fn enabled(config: &tauri::Config, debug: bool) -> bool {
    !debug && config.plugins.0.contains_key("updater")
}

/// shortcut: release.yml neither signs nor publishes `latest.json`, so the release channel has no
/// manifest to check; drop this once it does.
fn channel_reason(channel: UpdateChannel) -> Option<&'static str> {
    (channel == UpdateChannel::Release).then_some("Updates come from the releases page.")
}

fn unsupported(app: &AppHandle) -> Option<&'static str> {
    if !enabled(app.config(), cfg!(debug_assertions)) {
        Some("Development builds do not update.")
    } else if let Some(reason) = channel_reason(channel(&app.package_info().version.to_string())) {
        Some(reason)
    } else if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none() {
        Some("Updates come from your package manager.")
    } else if cfg!(target_os = "macos") && !std::env::current_exe().is_ok_and(|e| writable_bundle(&e)) {
        Some("Move Kelta to Applications to get updates.")
    } else {
        None
    }
}

/// Ready is sticky: once installed, only a newer install replaces it.
fn set(inner: &mut Inner, next: UpdateState) {
    if !matches!(inner.state, UpdateState::Ready { .. }) || matches!(next, UpdateState::Ready { .. }) {
        inner.state = next;
    }
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    let current = app.package_info().version.to_string();
    let inner = STATE.lock();
    let state = match unsupported(app) {
        Some(reason) => UpdateState::Unsupported { reason: reason.into() },
        None => inner.state.clone(),
    };
    UpdateStatus {
        channel: channel(&current),
        current,
        state,
        checked_at: inner.checked_at.clone(),
        restart_needs_confirm: crate::window::core(app).is_some_and(|c| c.quit_needs_confirm()),
    }
}

/// `app_ready` and window focus: check in the background when due.
pub fn auto_check(app: &AppHandle) {
    let updates = crate::window::core(app).is_some_and(|c| c.config().effective(None).app.updates);
    if !due(updates, STATE.lock().last, Instant::now()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check_and_install(&app).await });
}

/// Check, then download, verify and install. Returns at once when a check is already running.
pub async fn check_and_install(app: &AppHandle) {
    if unsupported(app).is_some() || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    {
        let mut inner = STATE.lock();
        inner.last = Some(Instant::now());
        set(&mut inner, UpdateState::Checking);
    }
    let next = match install(app).await {
        Ok(Some(version)) => UpdateState::Ready { version },
        Ok(None) => UpdateState::UpToDate,
        Err(e) => {
            tracing::warn!(error = %e, "update check failed");
            UpdateState::Failed { message: e.to_string() }
        }
    };
    let mut inner = STATE.lock();
    set(&mut inner, next);
    inner.checked_at =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).ok();
    drop(inner);
    RUNNING.store(false, Ordering::SeqCst);
    // The banner reads the status when told: a background check ends after its first read.
    let _ = app.emit("kelta://update", ());
}

async fn install(app: &AppHandle) -> tauri_plugin_updater::Result<Option<String>> {
    let current = app.package_info().version.clone();
    let ch = channel(&current.to_string());
    let installed = match &STATE.lock().state {
        UpdateState::Ready { version } => Version::parse(version).ok(),
        _ => None,
    };
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint(ch).parse()?])?
        .version_comparator(move |cur, remote| wanted(ch, &cur, installed.as_ref(), &remote.version))
        .build()?;
    let Some(update) = updater.check().await? else { return Ok(None) };
    set(&mut STATE.lock(), UpdateState::Downloading);
    update.download_and_install(|_, _| {}, || {}).await?;
    tracing::info!(version = %update.version, "update installed; it runs from the next start");
    Ok(Some(update.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn channel_comes_from_the_version() {
        assert_eq!(channel("0.1.0-nightly.202610101230"), UpdateChannel::Nightly);
        assert_eq!(channel("0.1.0"), UpdateChannel::Release);
        assert_eq!(channel("0.2.0-beta.1"), UpdateChannel::Release);
        assert_eq!(endpoint(UpdateChannel::Nightly), NIGHTLY_URL);
        assert_eq!(endpoint(UpdateChannel::Release), RELEASE_URL);
    }

    #[test]
    fn nightly_takes_any_other_build_release_only_a_higher_one() {
        let (n1, n2) = (v("0.1.0-nightly.202610101230"), v("0.1.0-nightly.202610111230"));
        let n = UpdateChannel::Nightly;
        assert!(wanted(n, &n1, None, &n2));
        assert!(wanted(n, &n2, None, &n1), "an older manifest is still the build to run");
        assert!(!wanted(n, &n1, None, &n1));
        assert!(!wanted(n, &n1, Some(&n2), &n2), "already installed");

        let r = UpdateChannel::Release;
        assert!(wanted(r, &v("0.1.0"), None, &v("0.1.1")));
        assert!(!wanted(r, &v("0.1.1"), None, &v("0.1.0")));
        assert!(!wanted(r, &v("0.1.0"), None, &v("0.1.0")));
        assert!(!wanted(r, &v("0.1.0"), Some(&v("0.1.1")), &v("0.1.1")));
        assert!(wanted(r, &v("0.1.0"), Some(&v("0.1.1")), &v("0.1.2")));
    }

    #[test]
    fn automatic_checks_wait_six_hours_and_follow_the_setting() {
        let now = Instant::now();
        let t0 = now.checked_sub(THROTTLE).unwrap();
        assert!(due(true, None, now));
        assert!(due(true, Some(t0), now));
        assert!(!due(true, Some(now - Duration::from_secs(5 * 3600)), now));
        assert!(!due(false, None, now), "app.updates = false");
        assert!(!due(false, Some(t0), now));
    }

    #[test]
    fn bundle_in_the_dmg_or_translocated_is_not_writable() {
        assert!(writable_bundle(Path::new("/Applications/Kelta.app/Contents/MacOS/kelta")));
        assert!(!writable_bundle(Path::new("/Volumes/Kelta/Kelta.app/Contents/MacOS/kelta")));
        assert!(!writable_bundle(Path::new(
            "/private/var/folders/x/AppTranslocation/1234/d/Kelta.app/Contents/MacOS/kelta"
        )));
    }

    #[test]
    fn dev_and_debug_builds_register_no_updater() {
        let mut config = tauri::Config::default();
        assert!(!enabled(&config, false), "no plugins.updater (dev config)");
        config.plugins.0.insert("updater".into(), serde_json::json!({ "pubkey": "k" }));
        assert!(enabled(&config, false), "release overlay");
        assert!(!enabled(&config, true), "debug build");
    }

    #[test]
    fn release_channel_has_no_manifest_yet() {
        assert_eq!(channel_reason(UpdateChannel::Release), Some("Updates come from the releases page."));
        assert_eq!(channel_reason(UpdateChannel::Nightly), None);
    }

    #[test]
    fn ready_is_sticky() {
        let mut inner =
            Inner { state: UpdateState::Ready { version: "1".into() }, checked_at: None, last: None };
        set(&mut inner, UpdateState::Failed { message: "offline".into() });
        assert_eq!(inner.state, UpdateState::Ready { version: "1".into() });
        set(&mut inner, UpdateState::Ready { version: "2".into() });
        assert_eq!(inner.state, UpdateState::Ready { version: "2".into() });
    }
}
