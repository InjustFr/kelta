//! ActionId catalog with platform default chords (SPEC §4). Exported to `ui/src/lib/gen/actions.ts`.
//!
//! Chord syntax: lowercase modifiers `ctrl`, `shift`, `alt`/`opt`, `cmd`, `mod` joined with `+`, then a
//! key named after `KeyboardEvent.code` semantics (`k`, `1`, `]`, `enter`, `left`, `pagedown`, `,`).

use std::collections::BTreeMap;

use serde::Serialize;

/// Action id: a built-in id from [`ACTIONS`] or `plugin.command.<command-id>`.
pub type ActionId = String;

/// Where an action may be triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionContext {
    /// Anywhere, including terminals.
    Global,
    /// Ticket views only (not in terminals).
    TicketViews,
    /// Editor pane.
    Editor,
    /// Terminal pane.
    Terminal,
    /// Not bound in-app (compositor / `kelta-ctl`).
    External,
}

/// One catalog entry.
#[derive(Debug, Clone, Serialize)]
pub struct ActionMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub mac: &'static [&'static str],
    pub linux: &'static [&'static str],
    /// Key after the app-prefix.
    pub prefix: Option<&'static str>,
    pub context: ActionContext,
}

macro_rules! a {
    ($id:literal, $label:literal, [$($m:literal),*], [$($l:literal),*], $p:expr, $ctx:ident) => {
        ActionMeta {
            id: $id,
            label: $label,
            mac: &[$($m),*],
            linux: &[$($l),*],
            prefix: $p,
            context: ActionContext::$ctx,
        }
    };
}

/// Built-in actions. Plugin commands use ids `plugin.command.<command-id>`.
pub const ACTIONS: &[ActionMeta] = &[
    a!("palette.open", "Command palette", ["cmd+k"], ["ctrl+shift+k"], Some(":"), Global),
    a!("project.switcher", "Switch project", ["cmd+p"], ["ctrl+shift+p"], Some("p"), Global),
    a!("project.goto.1", "Go to project 1", ["cmd+1"], ["ctrl+shift+1"], Some("1"), Global),
    a!("project.goto.2", "Go to project 2", ["cmd+2"], ["ctrl+shift+2"], Some("2"), Global),
    a!("project.goto.3", "Go to project 3", ["cmd+3"], ["ctrl+shift+3"], Some("3"), Global),
    a!("project.goto.4", "Go to project 4", ["cmd+4"], ["ctrl+shift+4"], Some("4"), Global),
    a!("project.goto.5", "Go to project 5", ["cmd+5"], ["ctrl+shift+5"], Some("5"), Global),
    a!("project.goto.6", "Go to project 6", ["cmd+6"], ["ctrl+shift+6"], Some("6"), Global),
    a!("project.goto.7", "Go to project 7", ["cmd+7"], ["ctrl+shift+7"], Some("7"), Global),
    a!("project.goto.8", "Go to project 8", ["cmd+8"], ["ctrl+shift+8"], Some("8"), Global),
    a!("project.goto.9", "Go to project 9", ["cmd+9"], ["ctrl+shift+9"], Some("9"), Global),
    a!("inbox.open", "Open Now", ["cmd+0"], ["ctrl+shift+0"], Some("0"), Global),
    a!("project.next", "Next project", ["cmd+ctrl+]"], ["ctrl+shift+pagedown"], Some(")"), Global),
    a!("project.prev", "Previous project", ["cmd+ctrl+["], ["ctrl+shift+pageup"], Some("("), Global),
    a!("tab.next", "Next tab", ["cmd+shift+]"], ["ctrl+shift+]"], Some("n"), Global),
    a!("tab.prev", "Previous tab", ["cmd+shift+["], ["ctrl+shift+["], Some("N"), Global),
    a!("session.new", "New session", ["cmd+t"], ["ctrl+shift+t"], Some("c"), Global),
    a!("pane.split_right", "Split right", ["cmd+d"], ["ctrl+shift+e"], Some("%"), Global),
    a!("pane.split_down", "Split down", ["cmd+shift+d"], ["ctrl+shift+o"], Some("\""), Global),
    a!("pane.focus_left", "Focus pane left", ["cmd+alt+left"], [], Some("h"), Global),
    a!("pane.focus_down", "Focus pane down", ["cmd+alt+down"], [], Some("j"), Global),
    a!("pane.focus_up", "Focus pane up", ["cmd+alt+up"], [], Some("k"), Global),
    a!("pane.focus_right", "Focus pane right", ["cmd+alt+right"], [], Some("l"), Global),
    a!("pane.zoom", "Zoom pane", ["cmd+shift+enter"], ["ctrl+shift+z"], Some("z"), Global),
    a!("pane.close", "Close pane", ["cmd+w"], ["ctrl+shift+w"], Some("x"), Global),
    a!("tickets.open", "Open tickets", ["cmd+shift+j"], ["ctrl+shift+j"], Some("t"), Global),
    a!("reviews.open", "Open reviews", ["cmd+shift+r"], ["ctrl+shift+r"], Some("r"), Global),
    a!("attention.next", "Next waiting", ["cmd+shift+u"], ["ctrl+shift+u"], Some("u"), Global),
    a!("work.start", "Start work", ["cmd+enter"], ["ctrl+enter"], Some("s"), TicketViews),
    // Linux: ctrl+shift+o is pane.split_down.
    a!("toast.run_last", "Run last toast action", ["cmd+shift+o"], ["ctrl+shift+a"], Some("o"), Global),
    a!("work.new", "New work item", ["cmd+shift+n"], ["ctrl+shift+n"], Some("w"), Global),
    a!(
        "editor.send_selection",
        "Send selection to Claude",
        ["cmd+shift+l"],
        ["ctrl+shift+l"],
        Some("@"),
        Editor
    ),
    a!("editor.quickfix_claude", "Quickfix: files Claude touched", [], [], None, Global),
    a!("terminal.search", "Search terminal", ["cmd+f"], ["ctrl+shift+f"], Some("/"), Terminal),
    a!("terminal.copy", "Copy", ["cmd+c"], ["ctrl+shift+c"], Some("["), Terminal),
    a!("terminal.paste", "Paste", ["cmd+v"], ["ctrl+shift+v"], Some("]"), Terminal),
    a!("settings.open", "Open settings", ["cmd+,"], ["ctrl+shift+,"], Some(","), Global),
    // Work menu and its entries for the focused tab's work item (FLOW §2.5, §3.5).
    a!("work.menu", "Work menu", ["cmd+."], ["ctrl+shift+."], Some("."), Global),
    a!("work.next", "Work: Next action", [], [], None, Global),
    a!("work.review_diff", "Work: Review diff", [], [], None, Global),
    a!("work.ship", "Work: Ship / Push", [], [], None, Global),
    a!("work.mark_reviewed", "Work: Mark reviewed", [], [], None, Global),
    a!("work.fix", "Work: Fix with Claude", [], [], None, Global),
    a!("work.rebase", "Work: Rebase", [], [], None, Global),
    a!("work.rebase_continue", "Work: Continue rebase", [], [], None, Global),
    a!("work.rebase_abort", "Work: Abort rebase", [], [], None, Global),
    a!("work.conflicts", "Work: Open conflicts in nvim", [], [], None, Global),
    a!("work.skip_step", "Work: Skip step", [], [], None, Global),
    a!("work.go_claude", "Work: Go to Claude", [], [], None, Global),
    a!("work.link", "Work: Link to ticket", [], [], None, Global),
    a!("work.open_ticket", "Work: Open ticket", [], [], None, Global),
    a!("work.open_pr", "Work: Open PR", [], [], None, Global),
    a!("work.finish", "Work: Finish", [], [], None, Global),
    a!("work.force_push", "Work: Force push…", [], [], None, Global),
    a!("work.rebase_ask_claude", "Work: Ask Claude to resolve conflicts", [], [], None, Global),
    a!("work.rerequest_review", "Work: Re-request review", [], [], None, Global),
    a!("work.resolve_threads", "Work: Resolve sent threads", [], [], None, Global),
    a!("work.finish_merged", "Finish all merged", [], [], None, Global),
    a!("window.toggle", "Toggle window (kelta-ctl toggle)", [], [], None, External),
];

/// Prefix for actions generated from plugin/config commands.
pub const PLUGIN_COMMAND_PREFIX: &str = "plugin.command.";

/// Chords never stolen from the terminal on Linux (SPEC §4 principles); used by `findConflicts`.
/// `ctrl+<letter>` and `alt+<any>` are reserved as classes (see `RESERVED_CLASSES`).
pub const RESERVED_CHORDS: &[&str] = &["shift+tab", "ctrl+space", "ctrl+\\"];

/// Reserved chord classes: `ctrl+<letter>`, `alt+*`, `meta+*`, `ctrl+alt+*`, `super+*`.
pub const RESERVED_CLASSES: &[&str] = &["ctrl+<letter>", "alt+*", "meta+*", "ctrl+alt+*", "super+*"];

pub fn find(id: &str) -> Option<&'static ActionMeta> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// Default `keys.prefix_bindings` (platform independent).
pub fn default_prefix_bindings() -> BTreeMap<String, String> {
    ACTIONS.iter().filter_map(|a| a.prefix.map(|p| (a.id.to_owned(), p.to_owned()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_prefixes_unique() {
        let mut ids: Vec<_> = ACTIONS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
        let mut prefixes: Vec<_> = ACTIONS.iter().filter_map(|a| a.prefix).collect();
        let n = prefixes.len();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), n);
        assert!(find("palette.open").is_some());
    }
}
