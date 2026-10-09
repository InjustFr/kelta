//! Notification rules (SPEC §7, SETTINGS [notifications]): per-kind toggles, focus/visibility,
//! quiet hours (local time).

use kelta_proto::ipc::WindowState;
use kelta_proto::settings::NotificationSettings;

use crate::status::NotifyKind;

pub fn kind_enabled(kind: NotifyKind, n: &NotificationSettings) -> bool {
    match kind {
        NotifyKind::ClaudeNeedsInput => n.claude_needs_input,
        NotifyKind::ClaudeDone => n.claude_done,
        NotifyKind::BellBackground => n.bell_background,
        NotifyKind::ReviewRequested => n.review_requested,
        NotifyKind::CiFailedMine => n.ci_failed_mine,
        NotifyKind::PrApproved => n.pr_approved,
        NotifyKind::PrChangesRequested => n.pr_changes_requested,
        NotifyKind::Program => true,
    }
}

fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// `"HH:MM-HH:MM"` (may wrap over midnight); empty or invalid = never quiet.
pub fn in_quiet_hours(spec: &str, minute_of_day: u32) -> bool {
    let Some((a, b)) = spec.split_once('-') else { return false };
    let (Some(start), Some(end)) = (parse_hm(a), parse_hm(b)) else { return false };
    if start == end {
        return false;
    }
    if start < end {
        (start..end).contains(&minute_of_day)
    } else {
        minute_of_day >= start || minute_of_day < end
    }
}

/// The notification rule: enabled, kind toggle, outside quiet hours, and (when
/// `only_when_unfocused`) the window is unfocused/hidden or the related pane is not visible.
pub fn should_notify(
    kind: NotifyKind,
    n: &NotificationSettings,
    window: WindowState,
    pane_visible: bool,
    minute_of_day: u32,
) -> bool {
    if !n.enabled || !kind_enabled(kind, n) || in_quiet_hours(&n.quiet_hours, minute_of_day) {
        return false;
    }
    if !n.only_when_unfocused {
        return true;
    }
    !window.exists || !window.visible || !window.focused || !pane_visible
}

/// Minutes since local midnight.
pub fn local_minute_of_day() -> u32 {
    let mut t: libc::time_t = 0;
    // SAFETY: `time` writes the current time into `t`; `localtime_r` fills `tm` (a plain C struct,
    // zero-initialised) and does not retain the pointers.
    let tm = unsafe {
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm
    };
    (tm.tm_hour.clamp(0, 23) as u32) * 60 + tm.tm_min.clamp(0, 59) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_wrap() {
        assert!(in_quiet_hours("22:00-07:00", 23 * 60));
        assert!(in_quiet_hours("22:00-07:00", 6 * 60 + 59));
        assert!(!in_quiet_hours("22:00-07:00", 7 * 60));
        assert!(in_quiet_hours("12:00-13:00", 12 * 60 + 30));
        assert!(!in_quiet_hours("", 0));
        assert!(!in_quiet_hours("garbage", 0));
        assert!(local_minute_of_day() < 24 * 60);
    }
}
