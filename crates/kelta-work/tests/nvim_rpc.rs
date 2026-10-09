//! nvim msgpack-RPC against a real headless `nvim --listen` (skipped when nvim is absent).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::dirs::Dirs;
use kelta_proto::events::BusEvent;
use kelta_proto::ids::SessionId;
use kelta_proto::model::{EditorMeta, EditorTarget, Lifecycle, SessionInfo, SessionKind};
use kelta_proto::samples;
use kelta_proto::testing::{FakeCore, MemWorkStore};
use kelta_work::WorkService;
use kelta_work::nvim::NvimClient;
use rmpv::Value;

struct Nvim {
    _child: tokio::process::Child,
    sock: PathBuf,
}

async fn start_nvim(dir: &Path) -> Option<Nvim> {
    let Ok(bin) = which::which("nvim") else {
        eprintln!("skipping: nvim not found");
        return None;
    };
    let sock = dir.join("nvim.sock");
    let child = tokio::process::Command::new(bin)
        .args(["--headless", "--clean", "-n", "--listen"])
        .arg(&sock)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    for _ in 0..500 {
        if sock.exists() && NvimClient::connect(&sock).await.is_ok() {
            return Some(Nvim { _child: child, sock });
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("nvim did not create its socket");
}

async fn buf_name(c: &mut NvimClient) -> String {
    c.call("nvim_buf_get_name", vec![Value::from(0)]).await.unwrap().as_str().unwrap().to_owned()
}

async fn cursor_line(c: &mut NvimClient) -> u64 {
    let v = c.call("nvim_win_get_cursor", vec![Value::from(0)]).await.unwrap();
    v.as_array().unwrap()[0].as_u64().unwrap()
}

async fn lines(c: &mut NvimClient) -> Vec<String> {
    let v = c
        .call("nvim_buf_get_lines", vec![Value::from(0), Value::from(0), Value::from(-1), Value::from(false)])
        .await
        .unwrap();
    v.as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_owned()).collect()
}

async fn select_lines_2_3(c: &mut NvimClient) {
    c.exec_lua(
        "vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('2GVj<Esc>', true, false, true), 'x', false)",
        vec![],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn edit_checktime_selection_mksession() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let Some(nvim) = start_nvim(&dir).await else { return };
    std::fs::write(dir.join("a.txt"), "line1\nline2\nline3\nline4\nline5\n").unwrap();
    std::fs::write(dir.join("b c#.txt"), "one\ntwo\nthree\n").unwrap();

    let mut c = NvimClient::connect(&nvim.sock).await.unwrap();
    // fnameescape: spaces and `#` in the name.
    c.edit(&dir.join("b c#.txt"), Some(2), true).await.unwrap();
    assert!(buf_name(&mut c).await.ends_with("b c#.txt"));
    assert_eq!(cursor_line(&mut c).await, 2);

    // External change + checktime → buffer reloaded.
    std::fs::write(dir.join("b c#.txt"), "uno\ndos\ntres\ncuatro\n").unwrap();
    c.checktime().await.unwrap();
    assert_eq!(lines(&mut c).await, vec!["uno", "dos", "tres", "cuatro"]);

    c.edit(&dir.join("a.txt"), None, true).await.unwrap();
    select_lines_2_3(&mut c).await;
    let sel = c.selection().await.unwrap();
    assert!(sel.path.ends_with("a.txt"));
    assert_eq!((sel.l1, sel.l2), (2, 3));
    assert_eq!(sel.text, "line2\nline3");
    assert_eq!(kelta_work::selection_ref(Path::new(&sel.path), &dir, sel.l1, sel.l2), "@a.txt#L2-3 ");

    // Non-focus open keeps a modified buffer and only :badd's the file.
    c.command("normal! ggOchanged").await.unwrap();
    c.edit(&dir.join("b c#.txt"), None, false).await.unwrap();
    assert!(buf_name(&mut c).await.ends_with("a.txt"));

    let session = dir.join("s.vim");
    c.mksession(&session).await.unwrap();
    assert!(std::fs::read_to_string(&session).unwrap().contains("a.txt"));
    // An RPC error surfaces as Upstream.
    let e = c.command("definitely-not-a-command").await.unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::Upstream);
}

fn session(id: &str, kind: SessionKind, cwd: &Path, sock: Option<&Path>) -> SessionInfo {
    let mut s = samples::session_info();
    s.id = SessionId::new(id);
    s.kind = kind;
    s.cwd = cwd.to_path_buf();
    s.lifecycle = Lifecycle::Live;
    s.work_item_id = None;
    s.editor = sock.map(|p| EditorMeta { adapter: "nvim".into(), socket: Some(p.to_path_buf()) });
    s
}

#[tokio::test]
async fn service_editor_open_send_selection_follow_and_quit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("w");
    std::fs::create_dir_all(&dir).unwrap();
    let Some(nvim) = start_nvim(&dir).await else { return };
    std::fs::write(dir.join("a.txt"), "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let core = FakeCore::new();
    core.add_project(samples::project_info());
    let editor = session("ed", SessionKind::Editor { adapter: "nvim".into() }, &dir, Some(&nvim.sock));
    let claude = session("cl", SessionKind::Claude, &dir, None);
    core.insert_session(editor.clone());
    core.insert_session(claude.clone());
    let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
    let dirs = Dirs::under(&tmp.path().join("k"));
    let w = WorkService::new(weak, Arc::new(MemWorkStore::new()), dirs.clone());
    w.startup().await.unwrap();

    w.editor_open(EditorTarget::Session { id: editor.id.clone() }, Path::new("a.txt"), Some(4))
        .await
        .unwrap();
    let mut c = NvimClient::connect(&nvim.sock).await.unwrap();
    assert!(buf_name(&mut c).await.ends_with("a.txt"));
    assert_eq!(cursor_line(&mut c).await, 4);

    select_lines_2_3(&mut c).await;
    w.send_selection(&editor.id, &claude.id).await.unwrap();
    assert_eq!(core.written_text(&claude.id), "\x1b[200~@a.txt#L2-3 \x1b[201~");

    // Not an editor → InvalidArgument; helix-like preset → Unsupported.
    let e = w
        .editor_open(EditorTarget::Session { id: claude.id.clone() }, Path::new("a.txt"), None)
        .await
        .unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::InvalidArgument);
    core.insert_session(session("hx", SessionKind::Editor { adapter: "helix".into() }, &dir, None));
    let e = w
        .editor_open(EditorTarget::Session { id: SessionId::new("hx") }, Path::new("a.txt"), None)
        .await
        .unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::Unsupported);

    // follow_claude_edits = reload: claude.file_edited → :checktime.
    std::fs::write(dir.join("a.txt"), "edited by claude\n").unwrap();
    core.publish(
        BusEvent::new("claude.file_edited", serde_json::json!({ "path": dir.join("a.txt"), "tool": "Edit" }))
            .with_session(claude.id.clone()),
    );
    let mut reloaded = false;
    for _ in 0..300 {
        if lines(&mut c).await == vec!["edited by claude"] {
            reloaded = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(reloaded, "buffer reloaded after claude.file_edited");

    // vim keys adapter: keystrokes written to the PTY.
    core.insert_session(session("vi", SessionKind::Editor { adapter: "vim".into() }, &dir, None));
    w.editor_open(EditorTarget::Session { id: SessionId::new("vi") }, Path::new("my file.rs"), Some(7))
        .await
        .unwrap();
    let typed = core.written_text(&SessionId::new("vi"));
    assert_eq!(
        typed,
        format!("\x1c\x0e:edit +7 {}\r", dir.join("my file.rs").display().to_string().replace(' ', "\\ "))
    );

    w.quit_hook().await.unwrap();
    let saved = dirs.data.join("sessions").join("ed.vim");
    assert!(saved.exists(), "mksession on quit");
}
