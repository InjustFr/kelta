//! Pure session status / attention decisions (ARCHITECTURE §7.5, §7.6).
//!
//! Hooks deliver a [`StatusChange`] (from `kelta_server::hooks::map`); the hooks-inactive fallback
//! derives changes from terminal output (heuristic). Both are folded into the session state by the
//! functions below, which are table-tested.

use kelta_proto::model::{Attention, SessionStatus, StatusChange};

/// Notification candidates; whether they fire is decided by [`crate::notifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    ClaudeNeedsInput,
    ClaudeDone,
    BellBackground,
    ReviewRequested,
    CiFailedMine,
    PrApproved,
    PrChangesRequested,
    /// OSC 9/777 desktop-notification request from a program.
    Program,
}

/// The parts of a session the decisions depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessState {
    pub status: SessionStatus,
    pub attention: Attention,
    pub seen: bool,
    /// Pane on screen (active project, active tab, window visible).
    pub visible: bool,
}

/// Result of folding one change into a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub status: SessionStatus,
    pub attention: Attention,
    pub seen: bool,
    /// `Some(true)` on SessionStart.
    pub hooks_active: Option<bool>,
    pub notify: Option<NotifyKind>,
}

impl Decision {
    fn keep(prev: &SessState) -> Self {
        Self {
            status: prev.status,
            attention: prev.attention,
            seen: prev.seen,
            hooks_active: None,
            notify: None,
        }
    }
}

/// Attention for a state the user may not have looked at: none while visible.
fn unless_visible(prev: &SessState, a: Attention) -> (Attention, bool) {
    if prev.visible { (Attention::None, true) } else { (a, false) }
}

/// Fold a hook [`StatusChange`] (ARCHITECTURE §7.6 table).
pub fn apply_hook(prev: &SessState, change: &StatusChange) -> Decision {
    let mut d = Decision::keep(prev);
    match change.status {
        // PostToolUse & co: status unchanged.
        SessionStatus::Unknown => {}
        SessionStatus::Starting | SessionStatus::Running => {
            d.status = SessionStatus::Running;
            d.hooks_active = Some(true);
        }
        SessionStatus::Working => {
            d.status = SessionStatus::Working;
            let (a, seen) = unless_visible(prev, Attention::Activity);
            d.attention = a;
            d.seen = seen;
        }
        SessionStatus::NeedsInput => {
            d.status = SessionStatus::NeedsInput;
            d.attention = Attention::NeedsInput;
            d.seen = prev.visible;
            d.notify = Some(NotifyKind::ClaudeNeedsInput);
        }
        SessionStatus::WaitingUser => {
            d.status = SessionStatus::WaitingUser;
            if !prev.seen && !prev.visible {
                d.attention = Attention::NeedsInput;
                d.notify = Some(NotifyKind::ClaudeNeedsInput);
            } else if prev.attention == Attention::NeedsInput && prev.status != SessionStatus::NeedsInput {
                d.attention = Attention::None;
            }
        }
        SessionStatus::Done => {
            d.status = SessionStatus::Done;
            let (a, seen) = unless_visible(prev, Attention::Done);
            d.attention = a;
            d.seen = seen;
            d.notify = Some(NotifyKind::ClaudeDone);
        }
        SessionStatus::Error => {
            d.status = SessionStatus::Error;
            d.attention = Attention::Error;
            d.seen = prev.visible;
        }
        SessionStatus::Exited => {
            d.status = SessionStatus::Exited;
            if prev.attention == Attention::NeedsInput {
                d.attention = Attention::None;
            }
        }
    }
    d
}

/// Inputs of the hooks-inactive heuristic (ARCHITECTURE §7.6, last paragraph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// Terminal output.
    Output,
    /// BEL / OSC 9 / OSC 777.
    Alert,
    /// 3 s without output after output.
    Quiet,
}

pub fn apply_heuristic(prev: &SessState, ev: Heuristic) -> Decision {
    let change =
        |status| StatusChange { status, preview: None, file_edited: None, raw_event: "heuristic".into() };
    match ev {
        Heuristic::Output => {
            if prev.status == SessionStatus::Working {
                Decision::keep(prev)
            } else {
                apply_hook(prev, &change(SessionStatus::Working))
            }
        }
        Heuristic::Alert => apply_hook(prev, &change(SessionStatus::NeedsInput)),
        Heuristic::Quiet => {
            if prev.status == SessionStatus::Working {
                apply_hook(prev, &change(SessionStatus::Done))
            } else {
                Decision::keep(prev)
            }
        }
    }
}

/// Output on a session not driven by hooks or heuristics (shells, editors, tools, hooked Claude).
pub fn apply_activity(prev: &SessState, kind_is_hooked_claude: bool) -> Decision {
    let mut d = Decision::keep(prev);
    if prev.status == SessionStatus::Starting {
        d.status = SessionStatus::Running;
    }
    if !kind_is_hooked_claude && !prev.visible && prev.attention < Attention::Activity {
        d.attention = Attention::Activity;
        d.seen = false;
    }
    d
}

/// BEL on a plain session (`terminal.bell = attention`).
pub fn apply_bell(prev: &SessState) -> Decision {
    let mut d = Decision::keep(prev);
    if !prev.visible {
        d.attention = d.attention.max(Attention::Activity);
        d.seen = false;
        d.notify = Some(NotifyKind::BellBackground);
    }
    d
}

/// `session_mark_seen`: clears everything but a pending needs-input.
pub fn mark_seen(prev: &SessState) -> Decision {
    let mut d = Decision::keep(prev);
    d.seen = true;
    if !(prev.attention == Attention::NeedsInput && prev.status == SessionStatus::NeedsInput) {
        d.attention = Attention::None;
    }
    d
}

/// Maximum preview length (chars).
pub const PREVIEW_MAX: usize = 200;

pub fn truncate_preview(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= PREVIEW_MAX { t.to_owned() } else { t.chars().take(PREVIEW_MAX).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_on_chars() {
        let s = "é".repeat(300);
        assert_eq!(truncate_preview(&s).chars().count(), PREVIEW_MAX);
    }
}
