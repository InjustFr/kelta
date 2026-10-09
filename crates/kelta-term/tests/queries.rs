//! The answered query set (ARCHITECTURE §7.4) against `docs/contracts/terminal-queries.md`:
//! exact replies from the model, `SWALLOWED_QUERIES` consistency, and exactly-once over a PTY.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{LoginEnv, SWALLOWED_QUERIES, TerminalLimits};
use kelta_term::PtyTerminalHost;
use kelta_term::backend::{PortablePty, PtyBackend, RustixPty};
use kelta_term::model::{Output, TermModel};
use kelta_term::palette::Palette;

struct Row {
    name: String,
    query: Vec<u8>,
    reply: Option<Vec<u8>>,
    swallowed: bool,
}

fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut b = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            continue;
        }
        match it.next() {
            Some('e') => out.push(0x1b),
            Some('a') => out.push(0x07),
            Some('\\') => out.push(b'\\'),
            other => panic!("bad escape \\{other:?} in {s:?}"),
        }
    }
    out
}

fn code(cell: &str) -> &str {
    cell.trim().trim_matches('`')
}

fn rows() -> Vec<Row> {
    let doc = std::fs::read_to_string(workspace_root().join("docs/contracts/terminal-queries.md")).unwrap();
    let body = doc
        .split("<!-- queries:begin -->")
        .nth(1)
        .and_then(|s| s.split("<!-- queries:end -->").next())
        .expect("query table markers");
    let mut out = Vec::new();
    for line in body.lines().map(str::trim).filter(|l| l.starts_with('|')) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 || cells[0] == "Name" || cells[0].starts_with("---") {
            continue;
        }
        let reply = code(cells[2]);
        out.push(Row {
            name: cells[0].to_owned(),
            query: unescape(code(cells[1])),
            reply: (reply != "—").then(|| unescape(reply)),
            swallowed: match cells[3] {
                "yes" => true,
                "no" => false,
                other => panic!("bad swallowed cell {other:?}"),
            },
        });
    }
    assert!(out.len() > 20, "query table not parsed");
    out
}

fn model_replies(query: &[u8]) -> Vec<Vec<u8>> {
    let mut m = TermModel::new(80, 24, 100);
    let mut out = Vec::new();
    m.advance(query, &Palette::default(), &mut out);
    // A reply never depends on a later flush.
    m.flush_sync(&Palette::default(), &mut out);
    out.into_iter()
        .filter_map(|o| match o {
            Output::Reply(r) => Some(r),
            Output::Event(_) => None,
        })
        .collect()
}

#[test]
fn model_answers_exactly_the_documented_set() {
    for r in rows() {
        let got = model_replies(&r.query);
        let want: Vec<Vec<u8>> = r.reply.clone().into_iter().collect();
        assert_eq!(
            got,
            want,
            "{}: query {:?} → {:?}",
            r.name,
            escape(&r.query),
            got.iter().map(|g| escape(g)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn swallowed_queries_match_the_contract() {
    let rows = rows();
    let doc_swallowed: BTreeSet<&str> =
        rows.iter().filter(|r| r.swallowed).map(|r| r.name.as_str()).collect();
    let proto: BTreeSet<&str> = SWALLOWED_QUERIES.iter().map(|q| q.name).collect();
    assert_eq!(doc_swallowed, proto, "docs/contracts/terminal-queries.md vs SWALLOWED_QUERIES");
    // Every swallowed query is answered by the model (otherwise nobody would answer it).
    for r in rows.iter().filter(|r| r.swallowed) {
        assert!(r.reply.is_some(), "{} is swallowed but unanswered", r.name);
    }
    // Every query the model answers is swallowed (the swallowed list is the exact answered set).
    let extra: Vec<&str> =
        rows.iter().filter(|r| r.reply.is_some() && !r.swallowed).map(|r| r.name.as_str()).collect();
    assert!(extra.is_empty(), "answered but not swallowed: {extra:?}");
}

/// All queries go through a real PTY: the child reads back every reply exactly once, in order.
#[test]
fn pty_child_reads_each_reply_exactly_once() {
    let rows = rows();
    let queries: Vec<u8> = rows.iter().flat_map(|r| r.query.clone()).collect();
    let expected: Vec<u8> = rows.iter().filter_map(|r| r.reply.clone()).flatten().collect();
    let backends: Vec<Arc<dyn PtyBackend>> = vec![Arc::new(PortablePty), Arc::new(RustixPty)];
    for b in backends {
        let name = b.name();
        let h = PtyTerminalHost::with_backend(LoginEnv::inherited(), TerminalLimits::default(), b);
        let dir = tempfile::tempdir().unwrap();
        let q = dir.path().join("q.bin");
        let out = dir.path().join("out.bin");
        std::fs::write(&q, &queries).unwrap();
        // Raw, no echo; `cat` ends after 1 s without input (VMIN 0, VTIME 10).
        let script = format!(
            "stty raw -echo; stty min 0 time 10; cat '{}'; cat > '{}'; echo DONE",
            q.display(),
            out.display()
        );
        let ev = Arc::new(Events::default());
        h.spawn(spec("q", "/bin/sh", &["-c", &script], 80, 24, ev.clone())).unwrap();
        assert_eq!(ev.wait_exit(Duration::from_secs(20)), (Some(0), None), "{name}");
        let got = std::fs::read(&out).unwrap();
        assert_eq!(
            escape(&got),
            escape(&expected),
            "{name}: {}",
            h.text_tail(&SessionId::new("q"), 5).unwrap()
        );
    }
}
