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
    /// Anywhere but a text field: ticket views and terminals (`work.start`, #441).
    NoTextField,
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
    /// Linux chords when they are not the derived Mac set; `None` derives them, see [`ActionMeta::linux`].
    #[serde(skip)]
    pub linux_override: Option<&'static [&'static str]>,
    /// Key after the app-prefix.
    pub prefix: Option<&'static str>,
    pub context: ActionContext,
}

macro_rules! a {
    ($id:literal, $label:literal, [$($m:literal),*], $p:expr, $ctx:ident) => {
        ActionMeta {
            id: $id,
            label: $label,
            mac: &[$($m),*],
            linux_override: None,
            prefix: $p,
            context: ActionContext::$ctx,
        }
    };
    ($id:literal, $label:literal, [$($m:literal),*], linux [$($l:literal),*], $p:expr, $ctx:ident) => {
        ActionMeta {
            id: $id,
            label: $label,
            mac: &[$($m),*],
            linux_override: Some(&[$($l),*]),
            prefix: $p,
            context: ActionContext::$ctx,
        }
    };
}

impl ActionMeta {
    /// Linux default chords: the Mac set with `cmd` → `ctrl`. A derived plain `ctrl+<letter>` also gets
    /// `ctrl+shift+<letter>` (the terminal keeps `ctrl+<letter>` when it has focus) unless another
    /// action's Mac set already takes that chord.
    pub fn linux(&self) -> Vec<String> {
        if let Some(chords) = self.linux_override {
            return chords.iter().map(|c| (*c).to_owned()).collect();
        }
        let mut out = Vec::new();
        for mac in self.mac {
            let chord = mac.replace("cmd", "ctrl");
            if let Some(letter) =
                chord.strip_prefix("ctrl+").filter(|k| k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase())
            {
                let alias = format!("ctrl+shift+{letter}");
                let taken = ACTIONS.iter().any(|a| a.mac.contains(&format!("cmd+shift+{letter}").as_str()));
                out.push(chord);
                if !taken {
                    out.push(alias);
                }
            } else {
                out.push(chord);
            }
        }
        out
    }
}

/// Built-in actions. Plugin commands use ids `plugin.command.<command-id>`.
pub const ACTIONS: &[ActionMeta] = &[
    a!("palette.open", "Command palette", ["cmd+k"], Some(":"), Global),
    a!("project.switcher", "Switch project", ["cmd+p"], Some("p"), Global),
    // The switcher filtered to closed projects (#363).
    a!("project.open", "Open project", [], None, Global),
    a!("project.new", "Add project", [], None, Global),
    // shortcut: `project.goto.N` selects rail row N (any project); the id is misnamed but keeps
    // `[keys.bindings]` overrides valid. Rename it with an alias in `lib/keys/manager.ts` if that matters.
    a!("project.goto.1", "Go to rail row 1", ["cmd+1"], Some("1"), Global),
    a!("project.goto.2", "Go to rail row 2", ["cmd+2"], Some("2"), Global),
    a!("project.goto.3", "Go to rail row 3", ["cmd+3"], Some("3"), Global),
    a!("project.goto.4", "Go to rail row 4", ["cmd+4"], Some("4"), Global),
    a!("project.goto.5", "Go to rail row 5", ["cmd+5"], Some("5"), Global),
    a!("project.goto.6", "Go to rail row 6", ["cmd+6"], Some("6"), Global),
    a!("project.goto.7", "Go to rail row 7", ["cmd+7"], Some("7"), Global),
    a!("project.goto.8", "Go to rail row 8", ["cmd+8"], Some("8"), Global),
    a!("project.goto.9", "Go to rail row 9", ["cmd+9"], Some("9"), Global),
    // Tool N of the rail's Tools section.
    a!("tool.goto.1", "Open tool 1", ["cmd+alt+1"], None, Global),
    a!("tool.goto.2", "Open tool 2", ["cmd+alt+2"], None, Global),
    a!("tool.goto.3", "Open tool 3", ["cmd+alt+3"], None, Global),
    a!("tool.goto.4", "Open tool 4", ["cmd+alt+4"], None, Global),
    a!("tool.goto.5", "Open tool 5", ["cmd+alt+5"], None, Global),
    a!("tool.goto.6", "Open tool 6", ["cmd+alt+6"], None, Global),
    a!("tool.goto.7", "Open tool 7", ["cmd+alt+7"], None, Global),
    a!("tool.goto.8", "Open tool 8", ["cmd+alt+8"], None, Global),
    a!("tool.goto.9", "Open tool 9", ["cmd+alt+9"], None, Global),
    a!("inbox.open", "Open Queue", ["cmd+0"], Some("0"), Global),
    // `cmd+ctrl` has no Linux twin: `ctrl+<arrow>` is word motion in shells and editors.
    a!("project.next", "Next project", ["cmd+ctrl+]"], linux["ctrl+shift+pagedown"], Some(")"), Global),
    a!("project.prev", "Previous project", ["cmd+ctrl+["], linux["ctrl+shift+pageup"], Some("("), Global),
    a!("tab.next", "Next tab", ["cmd+shift+]"], Some("n"), Global),
    a!("tab.prev", "Previous tab", ["cmd+shift+["], Some("N"), Global),
    a!("session.new", "New session", ["cmd+t"], Some("c"), Global),
    a!("session.make_work", "Make this a work item", [], None, Global),
    a!("pane.split_right", "Split right", ["cmd+d"], Some("%"), Global),
    a!("pane.split_down", "Split down", ["cmd+shift+d"], Some("\""), Global),
    a!("pane.focus_left", "Focus pane left", ["cmd+alt+left"], Some("h"), Global),
    a!("pane.focus_down", "Focus pane down", ["cmd+alt+down"], Some("j"), Global),
    a!("pane.focus_up", "Focus pane up", ["cmd+alt+up"], Some("k"), Global),
    a!("pane.focus_right", "Focus pane right", ["cmd+alt+right"], Some("l"), Global),
    a!("pane.zoom", "Zoom pane", ["cmd+shift+enter"], Some("z"), Global),
    a!("pane.close", "Close pane", ["cmd+w"], Some("x"), Global),
    a!("pane.detach", "Move pane to new window", [], None, Global),
    // Mod+J / Mod+Shift+J belong to the jump queue (ticket #136).
    a!("tickets.open", "Open tickets", ["cmd+shift+b"], Some("t"), Global),
    a!("tickets.board", "Open project board", [], None, Global),
    a!("tickets.groom", "Groom tickets", [], None, Global),
    a!("reviews.open", "Open pull requests", ["cmd+shift+r"], Some("r"), Global),
    // The active project's Tickets beside Reviews For me over Reviews Mine, in one tab (#374).
    a!("tickets.overview", "Open tickets and pull requests side by side", [], None, Global),
    a!("attention.next", "Next waiting", ["cmd+j"], Some("u"), Global),
    a!("attention.prev", "Previous waiting", ["cmd+shift+j"], Some("U"), Global),
    a!("attention.peek", "Peek waiting session", ["cmd+shift+y"], Some("y"), Global),
    a!("nav.back", "Go back", ["cmd+ctrl+left"], linux["ctrl+alt+shift+left"], Some("-"), Global),
    a!("nav.forward", "Go forward", ["cmd+ctrl+right"], linux["ctrl+alt+shift+right"], Some("="), Global),
    a!("work.start", "Start or continue work", ["cmd+enter"], Some("s"), NoTextField),
    a!("toast.run_last", "Run last toast action", ["cmd+shift+o"], Some("o"), Global),
    a!("work.new", "New work item", ["cmd+shift+n"], Some("w"), Global),
    a!("editor.send_selection", "Send selection to Claude", ["cmd+shift+l"], Some("@"), Editor),
    a!("editor.quickfix_claude", "Quickfix: files Claude touched", [], None, Global),
    a!("terminal.search", "Search terminal", ["cmd+f"], Some("/"), Terminal),
    // The Linux terminal convention: `ctrl+c` / `ctrl+v` stay SIGINT / literal-next even with `kelta_first`.
    a!("terminal.copy", "Copy", ["cmd+c"], linux["ctrl+shift+c"], Some("["), Terminal),
    a!("terminal.paste", "Paste", ["cmd+v"], linux["ctrl+shift+v"], Some("]"), Terminal),
    a!("settings.open", "Open settings", ["cmd+,"], Some(","), Global),
    a!("rail.toggle", "Toggle sidebar", ["cmd+b"], Some("b"), Global),
    // Hides the rail and the docked Story panel, holds notifications about other work (#421).
    a!("view.focus", "Focus mode", ["cmd+shift+f"], Some("f"), Global),
    a!("app.restart", "Restart Kelta", [], None, Global),
    a!("claude.setup", "Set up with Claude", [], None, Global),
    // Work menu and its entries for the focused tab's work item (FLOW §2.5, §3.5).
    a!("work.menu", "Work menu", ["cmd+."], Some("."), Global),
    a!("work.next", "Work: Next action", [], Some("Enter"), Global),
    a!("work.story", "Work: Story panel", ["cmd+i"], None, Global),
    a!("work.review_delta", "Work: Review changes since last look", [], Some("v"), Global),
    a!("work.review_diff", "Work: Review diff", [], None, Global),
    a!("work.ship", "Work: Ship / Push", [], Some("d"), Global),
    a!("work.mark_reviewed", "Work: Mark reviewed", [], None, Global),
    a!("work.edit_note", "Work: Edit next: note", [], Some("e"), Global),
    a!("work.fix", "Work: Fix with Claude", [], None, Global),
    a!("work.rebase", "Work: Rebase", [], None, Global),
    a!("work.rebase_continue", "Work: Continue rebase", [], None, Global),
    a!("work.rebase_abort", "Work: Abort rebase", [], None, Global),
    a!("work.conflicts", "Work: Open conflicts in nvim", [], None, Global),
    a!("work.skip_step", "Work: Skip step", [], None, Global),
    a!("work.go_claude", "Work: Go to Claude", [], Some("g"), Global),
    a!("work.start_claude", "Work: Start Claude", [], None, Global),
    a!("work.link", "Work: Link to ticket", [], None, Global),
    a!("work.create_ticket", "Work: Create ticket", [], None, Global),
    a!("work.open_ticket", "Work: Open ticket", [], None, Global),
    a!("work.next_step", "Work: Next step", [], None, Global),
    a!("work.move_step", "Work: Move to step…", [], None, Global),
    a!("work.open_pr", "Work: Open PR", [], None, Global),
    a!("work.finish", "Work: Finish", [], None, Global),
    a!("work.park", "Work: Park", [], None, Global),
    a!("work.force_push", "Work: Force push…", [], None, Global),
    a!("work.rebase_ask_claude", "Work: Ask Claude to resolve conflicts", [], None, Global),
    a!("work.rerequest_review", "Work: Re-request review", [], None, Global),
    a!("work.resolve_threads", "Work: Resolve sent threads", [], None, Global),
    a!("work.finish_merged", "Finish all merged", [], None, Global),
    a!("window.toggle", "Toggle window (kelta-ctl toggle)", [], None, External),
];

/// Prefix for actions generated from plugin/config commands.
pub const PLUGIN_COMMAND_PREFIX: &str = "plugin.command.";

/// Chords terminals use on Linux (SPEC §4 principles); used by `findConflicts`. `ctrl+<letter>` and
/// `alt+<any>` are reserved as classes (see `RESERVED_CLASSES`). Kelta's plain `ctrl+<letter>` defaults
/// leave the key to a focused terminal (`keys.linux_terminal`).
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

    #[test]
    fn linux_follows_mac_with_ctrl() {
        let linux = |id| find(id).unwrap().linux();
        assert_eq!(linux("palette.open"), ["ctrl+k", "ctrl+shift+k"]);
        assert_eq!(linux("pane.split_down"), ["ctrl+shift+d"]);
        assert_eq!(linux("pane.focus_left"), ["ctrl+alt+left"]);
        assert_eq!(linux("project.goto.1"), ["ctrl+1"]);
        // `ctrl+shift+d` is split down, so split right has no alias.
        assert_eq!(linux("pane.split_right"), ["ctrl+d"]);
        assert_eq!(linux("terminal.copy"), ["ctrl+shift+c"]);
        // Every Mac chord has a Linux one, no Linux chord repeats a modifier and none is shared.
        let mut seen = std::collections::BTreeSet::new();
        for a in ACTIONS {
            let chords = a.linux();
            assert_eq!(chords.is_empty(), a.mac.is_empty(), "{}", a.id);
            for c in chords {
                let mods: Vec<_> = c.split('+').collect();
                let mut uniq = mods.clone();
                uniq.sort_unstable();
                uniq.dedup();
                assert_eq!(uniq.len(), mods.len(), "{}: {c}", a.id);
                assert!(!c.contains("cmd"), "{}: {c}", a.id);
                assert!(seen.insert(c.clone()), "{}: {c} bound twice", a.id);
            }
        }
    }
}
