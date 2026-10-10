//! Review notes (#133): Louis's notes from nvim, anchored at `path:line[-line]`, sent to the item's
//! Claude in one message by [`WorkService::deliver_prompt`]. The next `Stop` marks each sent note
//! `touched` / `untouched`: a heuristic, any `git diff -U0` hunk within [`NEAR`] lines of its anchor
//! since `refs/kelta/wi/<id>/notes` (the worktree snapshot taken at send time).

use std::path::Path;

use kelta_proto::error::KeltaError;
use kelta_proto::events::Toast;
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{
    Lifecycle, NoteState, ReviewNote, ReviewNotes, SessionInfo, StatusSource, WorkItem,
};

use crate::fixloop::{busy, claude_of};
use crate::git::{self, LOCAL_TIMEOUT};
use crate::nvim::{LUA_NOTES, NvimClient};
use crate::review::{numstat, shape, wi_ref};
use crate::{WorkService, files};

/// A hunk this close to a note's lines counts as addressing it.
const NEAR: u32 = 5;

/// The nvim side, loaded only by the nvim Kelta spawns (`--cmd`, see [`WorkService::nvim_plugin_args`]).
const NVIM_PLUGIN: &str = include_str!("../resources/nvim/kelta.lua");

/// How a prompt reaches the item's Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// Live and idle: bracketed paste + Enter.
    Paste,
    /// Mid-turn (or asking for permission): held until its next `Stop`.
    AtStop,
    /// Dormant or gone: `claude --resume <uuid> <prompt>`.
    Resume,
}

pub(crate) fn route(claude: Option<&SessionInfo>) -> Route {
    match claude {
        Some(s) if s.lifecycle == Lifecycle::Live => {
            // Without hooks no Stop would ever come: the paste refuses with `hooks_inactive` instead.
            if s.status_source == StatusSource::Hook && busy(s) { Route::AtStop } else { Route::Paste }
        }
        _ => Route::Resume,
    }
}

/// The one message for every note sent.
pub(crate) fn notes_message(notes: &[&ReviewNote]) -> String {
    let mut s = String::from("Review notes:\n");
    for n in notes {
        let at = crate::selection_ref(Path::new(&n.path), Path::new(""), n.line_start, n.line_end);
        s.push_str(&format!("- {at}— {}\n", n.body.trim().replace('\n', " ")));
    }
    s.push_str("Address each, then reply with one line per note saying what you changed.");
    s
}

/// Old-side line ranges `(path, first, last)` of the hunks of a `git diff -U0`.
pub(crate) fn hunks(diff: &str) -> Vec<(&str, u32, u32)> {
    let (mut path, mut header) = ("", false);
    let mut out = Vec::new();
    for l in diff.lines() {
        if l.starts_with("diff --git ") {
            header = true;
        } else if header && let Some(p) = l.strip_prefix("--- ") {
            path = p.strip_prefix("a/").unwrap_or(p);
        } else if let Some(h) = l.strip_prefix("@@ -") {
            header = false;
            let old = h.split(' ').next().unwrap_or_default();
            let (start, count) = old.split_once(',').unwrap_or((old, "1"));
            let (Ok(s), Ok(c)) = (start.parse::<u32>(), count.parse::<u32>()) else { continue };
            // A pure insertion (`-s,0`) sits after line s.
            out.push((path, s, s + c.saturating_sub(1)));
        }
    }
    out
}

pub(crate) fn touched(hunks: &[(&str, u32, u32)], n: &ReviewNote) -> bool {
    hunks.iter().any(|&(p, s, e)| p == n.path && s <= n.line_end + NEAR && e + NEAR >= n.line_start)
}

impl WorkService {
    /// Deliver `text` to the item's Claude conversation (also for red CI and PR comments): pasted
    /// when idle, held until the next `Stop` while it works, `--resume`d when dormant or gone.
    /// Returns true when held.
    pub async fn deliver_prompt(&self, id: &WorkItemId, text: &str) -> Result<bool, KeltaError> {
        self.ensure_listener();
        let item = self.load(id).await?;
        if item.claude_uuid.is_none() {
            return Err(KeltaError::invalid("this work item has no Claude conversation"));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let mut j = self.load_journal(id);
        let held = j.pending_prompt.take();
        let text = match &held {
            Some(p) => format!("{p}\n\n{text}"),
            None => text.to_owned(),
        };
        if route(claude_of(&env.core, &item, &j).as_ref()) != Route::AtStop {
            // A held message goes along now: left held, a later Stop would paste it again.
            if held.is_some() {
                self.save_journal(id, &j)?;
            }
            if let Err(e) = self.resume_item(id, Some(text)).await {
                if held.is_some() {
                    let mut j = self.load_journal(id);
                    j.pending_prompt = held;
                    self.save_journal(id, &j)?;
                }
                return Err(e);
            }
            return Ok(false);
        }
        j.pending_prompt = Some(text);
        self.save_journal(id, &j)?;
        env.core.toast(Toast::info("Claude is working: it gets your message when it stops."));
        Ok(true)
    }

    /// Claude of `sid` went idle (`Stop`, or the `idle_prompt` Notification after an Esc): deliver
    /// the held prompt. Returns true when there was one.
    pub(crate) async fn flush_pending(&self, sid: &SessionId) -> Result<bool, KeltaError> {
        let Some(item) = self.for_session(sid).await else { return Ok(false) };
        let Some(p) = self.load_journal(&item.id).pending_prompt else { return Ok(false) };
        let delivered = self.resume_item(&item.id, Some(p)).await;
        // Dropped either way: held past this Stop it would land after some unrelated turn.
        let mut j = self.load_journal(&item.id);
        j.pending_prompt = None;
        self.save_journal(&item.id, &j)?;
        let Err(e) = delivered else { return Ok(true) };
        // Reopen the notes it carried so `s` / `<leader>ks` can send them again.
        for mut n in self.store.notes(&item.id).await?.into_iter().filter(|n| n.state == NoteState::Sent) {
            (n.state, n.sent_at) = (NoteState::Open, None);
            self.store.put_note(&n).await?;
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        env.core.toast(Toast::error(format!("Your held message did not reach Claude: {}", e.message)));
        self.notes_changed(&item.id).await.map(|_| true)
    }

    /// A Claude `Stop` of `sid`: deliver the held prompt, else judge the notes sent before this turn.
    pub(crate) async fn notes_on_stop(&self, sid: &SessionId) -> Result<(), KeltaError> {
        if self.flush_pending(sid).await? {
            return Ok(());
        }
        let Some(item) = self.for_session(sid).await else { return Ok(()) };
        let mut sent: Vec<ReviewNote> =
            self.store.notes(&item.id).await?.into_iter().filter(|n| n.state == NoteState::Sent).collect();
        if sent.is_empty() {
            return Ok(());
        }
        let wt = &item.worktree;
        let last = git::snapshot(wt, &wi_ref(&item.id, "last")).await?;
        let from = wi_ref(&item.id, "notes");
        let args = [
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &from,
            &last,
        ];
        let diff = git::run_ok(wt, &args, LOCAL_TIMEOUT).await?;
        let hunks = hunks(&diff.stdout);
        for n in &mut sent {
            n.state = if touched(&hunks, n) { NoteState::Touched } else { NoteState::Untouched };
            self.store.put_note(n).await?;
        }
        self.notes_changed(&item.id).await.map(|_| ())
    }

    /// `work_notes`: every note of the item, with `+N/−M since feedback` once a sent batch was judged.
    pub async fn notes(&self, id: &WorkItemId) -> Result<ReviewNotes, KeltaError> {
        let item = self.load(id).await?;
        let notes = self.store.notes(id).await?;
        let judged = notes.iter().any(|n| matches!(n.state, NoteState::Touched | NoteState::Untouched));
        let mut since = None;
        if judged && item.worktree.is_dir() {
            let (from, last) = (wi_ref(id, "notes"), wi_ref(id, "last"));
            let out =
                git::run(&item.worktree, &["diff", "--numstat", "-z", &from, &last], LOCAL_TIMEOUT).await?;
            if out.ok() {
                since = Some(shape(&numstat(&out.stdout), |_| false));
            }
        }
        Ok(ReviewNotes { worktree: item.worktree, notes, since })
    }

    /// The work item of an editor session (ctl requests from Kelta's nvim).
    async fn item_of_session(&self, session: &SessionId) -> Result<WorkItem, KeltaError> {
        let core = self.api()?;
        match core.session_get(session).and_then(|s| s.work_item_id) {
            Some(id) => self.load(&id).await,
            None => self
                .for_session(session)
                .await
                .ok_or_else(|| KeltaError::not_found("this editor is not part of a work item")),
        }
    }

    /// ctl `note_add`: a note on `path` (absolute, inside the item's worktree).
    pub async fn note_add(
        &self,
        session: &SessionId,
        path: &Path,
        lines: (u32, u32),
        body: &str,
    ) -> Result<ReviewNotes, KeltaError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(KeltaError::invalid("the note is empty"));
        }
        let (l1, l2) = (lines.0.min(lines.1).max(1), lines.0.max(lines.1).max(1));
        let item = self.item_of_session(session).await?;
        let rel = crate::ops::rel_to(path, &item.worktree)
            .ok_or_else(|| KeltaError::invalid("notes go on files of the work item's worktree"))?;
        self.store
            .put_note(&ReviewNote {
                id: 0,
                work_item_id: item.id.clone(),
                path: rel.to_string_lossy().into_owned(),
                line_start: l1,
                line_end: l2,
                body: body.to_owned(),
                source: "nvim".into(),
                ext_ref: None,
                state: NoteState::Open,
                sent_at: None,
            })
            .await?;
        self.notes_changed(&item.id).await
    }

    /// ctl `note_list`.
    pub async fn notes_of_session(&self, session: &SessionId) -> Result<ReviewNotes, KeltaError> {
        let item = self.item_of_session(session).await?;
        self.notes(&item.id).await
    }

    /// ctl `note_send`.
    pub async fn notes_send_of_session(&self, session: &SessionId) -> Result<ReviewNotes, KeltaError> {
        let item = self.item_of_session(session).await?;
        self.notes_send(&item.id).await
    }

    /// ctl `note_resolve`.
    pub async fn note_resolve_of_session(
        &self,
        session: &SessionId,
        note: i64,
    ) -> Result<ReviewNotes, KeltaError> {
        let item = self.item_of_session(session).await?;
        self.note_resolve(&item.id, note).await
    }

    /// `work_note_resolve`.
    pub async fn note_resolve(&self, id: &WorkItemId, note: i64) -> Result<ReviewNotes, KeltaError> {
        let mut n = self
            .store
            .notes(id)
            .await?
            .into_iter()
            .find(|n| n.id == note)
            .ok_or_else(|| KeltaError::not_found(format!("note {note}")))?;
        n.state = NoteState::Resolved;
        self.store.put_note(&n).await?;
        self.notes_changed(id).await
    }

    /// `work_notes_send`: the open (and still untouched) notes in one message to the item's Claude.
    pub async fn notes_send(&self, id: &WorkItemId) -> Result<ReviewNotes, KeltaError> {
        let item = self.load(id).await?;
        let mut open: Vec<ReviewNote> = self
            .store
            .notes(id)
            .await?
            .into_iter()
            .filter(|n| matches!(n.state, NoteState::Open | NoteState::Untouched))
            .collect();
        if open.is_empty() {
            return Err(KeltaError::invalid("no open review notes"));
        }
        // shortcut: snapshot at send time, so a held message still counts the rest of Claude's
        // current turn as feedback work; snapshot on delivery if that misleads.
        git::snapshot(&item.worktree, &wi_ref(id, "notes")).await?;
        self.deliver_prompt(id, &notes_message(&open.iter().collect::<Vec<_>>())).await?;
        let now = kelta_proto::now_rfc3339();
        for n in &mut open {
            (n.state, n.sent_at) = (NoteState::Sent, Some(now.clone()));
            self.store.put_note(n).await?;
        }
        self.notes_changed(id).await
    }

    /// Tell the UI (`work.updated`) and the item's nvim (signs re-placed) that the notes changed.
    async fn notes_changed(&self, id: &WorkItemId) -> Result<ReviewNotes, KeltaError> {
        let view = self.notes(id).await?;
        let item = self.load(id).await?;
        self.publish_updated(&item);
        if let Some(sock) = item.nvim_socket {
            let json = serde_json::to_string(&view)?;
            // Detached: the nvim that asked may be blocked on this very ctl answer.
            tokio::spawn(async move {
                let push =
                    async { NvimClient::connect(&sock).await?.exec_lua(LUA_NOTES, vec![json.into()]).await };
                if let Err(e) = push.await {
                    tracing::debug!(error = %e.message, "review notes not pushed to nvim");
                }
            });
        }
        Ok(view)
    }

    /// `--cmd` loading Kelta's nvim side (review notes) into an nvim Kelta spawns; written to
    /// `<data>/nvim/kelta.lua` on each spawn so it follows app updates. Empty when it cannot be written.
    pub(crate) fn nvim_plugin_args(&self) -> Vec<String> {
        let file = self.dirs.data.join("nvim").join("kelta.lua");
        let write = || -> Result<(), KeltaError> {
            if std::fs::read(&file).ok().as_deref() != Some(NVIM_PLUGIN.as_bytes()) {
                files::private_dir(&self.dirs.data.join("nvim"))?;
                files::write_private(&file, NVIM_PLUGIN.as_bytes())?;
            }
            Ok(())
        };
        if let Err(e) = write() {
            tracing::warn!(error = %e.message, "nvim review notes plugin not written");
            return Vec::new();
        }
        // `dofile`, not 'runtimepath': plugin managers (lazy.nvim) reset the runtimepath.
        vec!["--cmd".into(), format!("lua dofile([==[{}]==])", file.display())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::model::SessionStatus;

    fn note(path: &str, l1: u32, l2: u32, body: &str) -> ReviewNote {
        ReviewNote {
            id: 0,
            work_item_id: WorkItemId::new("w"),
            path: path.into(),
            line_start: l1,
            line_end: l2,
            body: body.into(),
            source: "nvim".into(),
            ext_ref: None,
            state: NoteState::Open,
            sent_at: None,
        }
    }

    #[test]
    fn route_by_session_state() {
        let mut s = kelta_proto::samples::session_info();
        (s.lifecycle, s.status_source) = (Lifecycle::Live, StatusSource::Hook);
        for (st, want) in [
            (SessionStatus::Done, Route::Paste),
            (SessionStatus::WaitingUser, Route::Paste),
            (SessionStatus::Working, Route::AtStop),
            (SessionStatus::NeedsInput, Route::AtStop),
        ] {
            s.status = st;
            assert_eq!(route(Some(&s)), want, "{st:?}");
        }
        s.status_source = StatusSource::Heuristic;
        assert_eq!(route(Some(&s)), Route::Paste, "no hooks: never held, the paste refuses");
        s.lifecycle = Lifecycle::Dormant;
        assert_eq!(route(Some(&s)), Route::Resume);
        assert_eq!(route(None), Route::Resume);
    }

    #[test]
    fn message_lists_every_note() {
        let (a, b) = (note("src/a.rs", 10, 14, " use the backoff helper\n"), note("src/b.rs", 3, 3, "why?"));
        assert_eq!(
            notes_message(&[&a, &b]),
            "Review notes:\n- @src/a.rs#L10-14 — use the backoff helper\n- @src/b.rs#L3 — why?\n\
             Address each, then reply with one line per note saying what you changed."
        );
    }

    #[test]
    fn hunks_near_the_anchor_touch_it() {
        let diff = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n\
                    @@ -30,2 +30,3 @@ fn x()\n--- a removed comment line\n+x\n@@ -50,0 +52 @@\n+y\n\
                    diff --git a/src/new.rs b/src/new.rs\n--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1 @@\n+z\n";
        let h = hunks(diff);
        assert_eq!(h, vec![("src/a.rs", 30, 31), ("src/a.rs", 50, 50), ("/dev/null", 0, 0)]);
        assert!(touched(&h, &note("src/a.rs", 20, 25, "")), "5 lines before the hunk");
        assert!(touched(&h, &note("src/a.rs", 36, 36, "")), "5 lines after");
        assert!(!touched(&h, &note("src/a.rs", 37, 44, "")));
        assert!(touched(&h, &note("src/a.rs", 55, 60, "")), "an insertion after line 50");
        assert!(!touched(&h, &note("src/b.rs", 30, 30, "")), "another file");
    }
}
