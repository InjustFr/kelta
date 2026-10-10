//! Review notes (#133): added from Kelta's nvim, sent in one message (held while Claude works and
//! delivered on its `Stop`), then judged touched / untouched on the next `Stop`.

use std::path::Path;
use std::time::Duration;

use crate::common::{Fx, git, has_git, project};
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::error::ErrorCode;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ids::SessionId;
use kelta_proto::model::{
    Lifecycle, NoteState, ReviewNote, SessionKind, SessionStatus, StatusSource, WorkItem, WorkSource,
};
use kelta_proto::samples;
use serde_json::json;

fn stop(fx: &Fx, sid: &SessionId) {
    let mut s = fx.core.sessions().into_iter().find(|s| &s.id == sid).unwrap();
    (s.status, s.status_source) = (SessionStatus::Done, StatusSource::Hook);
    fx.core.insert_session(s);
    let payload = json!({ "hook_event_name": "Stop" });
    fx.core.publish(
        BusEvent::new(bus::CLAUDE_HOOK, json!({ "event": "Stop", "payload": payload }))
            .with_session(sid.clone()),
    );
}

async fn wait_notes(fx: &Fx, item: &WorkItem, pred: impl Fn(&[ReviewNote]) -> bool) -> Vec<ReviewNote> {
    for _ in 0..500 {
        let notes = fx.store.notes(&item.id).await.unwrap();
        if pred(&notes) {
            return notes;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("notes never reached the expected state");
}

#[tokio::test]
async fn notes_go_in_one_message_after_the_busy_turn_and_are_judged_on_the_next_stop() {
    if !has_git() {
        eprintln!("skipping: git not found");
        return;
    }
    let fx = Fx::new();
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let item = w.start(w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap()).await.unwrap();
    let live = |k: fn(&SessionKind) -> bool| {
        fx.spawned_of(k).into_iter().find(|s| s.lifecycle == Lifecycle::Live).unwrap().id
    };
    let claude = live(|k| *k == SessionKind::Claude);
    let editor = live(|k| matches!(k, SessionKind::Editor { .. }));

    // Kelta's nvim loads the notes plugin; any other nvim never sees it.
    let call = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "editor");
    let args: Vec<String> = serde_json::from_value(call.unwrap().args["args"].clone()).unwrap();
    assert_eq!(args[0], "--cmd");
    let lua = fx.dirs.data.join("nvim/kelta.lua");
    assert_eq!(args[1], format!("lua dofile([==[{}]==])", lua.display()));
    assert!(std::fs::read_to_string(&lua).unwrap().contains("note_send"));

    let wt = &item.worktree;
    let text: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    std::fs::write(wt.join("a.rs"), &text).unwrap();
    git(wt, &["add", "a.rs"]);
    git(wt, &["commit", "-q", "-m", "a"]);
    w.note_add(&editor, &wt.join("a.rs"), (12, 10), " use the backoff helper ").await.unwrap();
    let view = w.note_add(&editor, &wt.join("a.rs"), (35, 35), "rename").await.unwrap();
    assert_eq!(view.notes.len(), 2);
    let e = w.note_add(&editor, Path::new("/etc/hosts"), (1, 1), "x").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument, "outside the worktree");

    // Claude is mid-turn: nothing typed, the message waits for its Stop.
    let mut s = fx.core.session_get(&claude).unwrap();
    (s.status, s.status_source) = (SessionStatus::Working, StatusSource::Hook);
    fx.core.insert_session(s);
    let out = w.notes_send(&item.id).await.unwrap();
    assert!(out.notes.iter().all(|n| n.state == NoteState::Sent && n.sent_at.is_some()));
    assert_eq!(fx.core.written_text(&claude), "");

    stop(&fx, &claude);
    let msg = "Review notes:\n- @a.rs#L10-12 — use the backoff helper\n- @a.rs#L35 — rename\n\
               Address each, then reply with one line per note saying what you changed.";
    for _ in 0..500 {
        if !fx.core.written_text(&claude).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(fx.core.written_text(&claude), format!("\x1b[200~{msg}\x1b[201~\r"));
    assert!(fx.store.notes(&item.id).await.unwrap().iter().all(|n| n.state == NoteState::Sent));

    // Claude's turn changes line 15 only: the first note is touched, the second is not.
    std::fs::write(wt.join("a.rs"), text.replace("line 15\n", "line fifteen\n")).unwrap();
    stop(&fx, &claude);
    let notes = wait_notes(&fx, &item, |n| n.iter().all(|n| n.state != NoteState::Sent)).await;
    assert_eq!(
        notes.iter().map(|n| (n.line_start, n.state)).collect::<Vec<_>>(),
        [(10, NoteState::Touched), (35, NoteState::Untouched)]
    );
    let since = w.notes(&item.id).await.unwrap().since.unwrap();
    assert_eq!((since.insertions, since.deletions), (1, 1));

    // Resolved notes stay out; the untouched one goes again, pasted now that Claude is idle.
    w.note_resolve(&item.id, notes[0].id).await.unwrap();
    w.notes_send(&item.id).await.unwrap();
    let typed = fx.core.written_text(&claude);
    assert!(typed.ends_with("Review notes:\n- @a.rs#L35 — rename\nAddress each, then reply with one line per note saying what you changed.\x1b[201~\r"), "{typed}");
    let e = w.notes_send(&item.id).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument, "nothing open");
}
