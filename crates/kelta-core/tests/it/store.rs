//! SQLite store: migrations up from empty + idempotent, Work/Grant/Trust stores, trigger_log cap.

use std::path::Path;

use kelta_core::store::{Store, migrations, q};
use kelta_proto::api::{GrantStore, TrustStore, WorkStore};
use kelta_proto::ids::{PluginId, SessionId, WorkItemId};
use kelta_proto::model::{NextUpItem, NoteState, ReviewNote, StepStatus, WorkState};
use kelta_proto::store::{SCHEMA_VERSION, TriggerLogRow, tables};
use kelta_proto::tracker::TicketRef;

#[tokio::test]
async fn migrations_from_empty_and_idempotent() {
    assert_eq!(migrations::MIGRATIONS.len() as u32, SCHEMA_VERSION);
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("data").join("kelta.db");
    {
        let s = Store::open(&db).unwrap();
        let t = s.call(|c| q::tables(c)).await.unwrap();
        for name in tables::ALL {
            assert!(t.iter().any(|x| x == name), "missing table {name}: {t:?}");
        }
        let v = s.call(|c| migrations::current_version(c).map_err(kelta_core::store::db_err)).await.unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        s.call(|c| q::ui_state_set(c, "onboarding_done", "true")).await.unwrap();
    }
    // reopen: no re-run, data kept
    let s = Store::open(&db).unwrap();
    let rows: u32 = s
        .call(|c| {
            c.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
                .map_err(kelta_core::store::db_err)
        })
        .await
        .unwrap();
    assert_eq!(rows, SCHEMA_VERSION, "one row per applied migration");
    assert_eq!(s.call(|c| q::ui_state_get(c, "onboarding_done")).await.unwrap().as_deref(), Some("true"));
    // migrate again explicitly: still one version row
    let v = s.call(|c| migrations::migrate(c, &[]).map_err(kelta_core::store::db_err)).await.unwrap();
    assert_eq!(v, SCHEMA_VERSION);
}

#[tokio::test]
async fn next_up_keeps_its_order_snoozes_and_seen_marks_across_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("kelta.db");
    let t = |key: &str| TicketRef { account: "jira".into(), key: key.into(), id: key.into() };
    let it = |key: &str, rank: Option<f64>, snoozed: Option<&str>| NextUpItem {
        project_id: "shop".into(),
        ticket: t(key),
        rank,
        snoozed_until: snoozed.map(Into::into),
    };
    {
        let s = Store::open(&db).unwrap();
        for i in [
            it("A-1", Some(1.0), None),
            it("A-2", Some(0.5), None),
            it("A-3", None, Some("2026-10-17T00:00:00Z")),
        ] {
            s.call(move |c| q::next_up_put(c, &i)).await.unwrap();
        }
        // Re-rank (J/K) is an upsert.
        let moved = it("A-1", Some(0.25), None);
        s.call(move |c| q::next_up_put(c, &moved)).await.unwrap();
        s.call(move |c| q::ticket_seen(c, &[t("A-2")])).await.unwrap();
        s.call(move |c| q::ticket_seen(c, &[t("A-2"), t("A-2")])).await.unwrap();
    }
    let s = Store::open(&db).unwrap();
    let got = s.call(|c| q::next_up(c)).await.unwrap();
    let keys: Vec<_> = got.items.iter().map(|i| i.ticket.key.as_str()).collect();
    assert_eq!(keys, ["A-1", "A-2", "A-3"], "rank order, snoozed-only last");
    assert_eq!(got.items[2].snoozed_until.as_deref(), Some("2026-10-17T00:00:00Z"));
    assert_eq!(got.seen, ["jira:A-2"]);
    s.call(move |c| q::next_up_remove(c, &t("A-1"))).await.unwrap();
    assert_eq!(s.call(|c| q::next_up(c)).await.unwrap().items.len(), 2);
}

#[tokio::test]
async fn notes_survive_a_restart_and_go_with_their_item() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("kelta.db");
    let w = kelta_proto::samples::work_item();
    let note = |path: &str, line: u32| ReviewNote {
        id: 0,
        work_item_id: w.id.clone(),
        path: path.into(),
        line_start: line,
        line_end: line + 2,
        body: format!("fix {path}"),
        source: "nvim".into(),
        ext_ref: None,
        state: NoteState::Open,
        sent_at: None,
    };
    {
        let s = Store::open(&db).unwrap();
        s.put_item(&w).await.unwrap();
        let b = s.put_note(&note("src/b.rs", 3)).await.unwrap();
        let a = s.put_note(&note("src/a.rs", 10)).await.unwrap();
        assert_ne!(a, b);
        let mut sent = s.notes(&w.id).await.unwrap()[0].clone();
        (sent.state, sent.sent_at) = (NoteState::Untouched, Some("t".into()));
        assert_eq!(s.put_note(&sent).await.unwrap(), a);
    }
    let s = Store::open(&db).unwrap();
    let got = s.notes(&w.id).await.unwrap();
    assert_eq!(got.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(), ["src/a.rs", "src/b.rs"]);
    assert_eq!(
        (got[0].state, got[0].sent_at.as_deref(), got[0].line_end),
        (NoteState::Untouched, Some("t"), 12)
    );
}

#[tokio::test]
async fn claim_item_is_atomic_and_round_trips_the_run() {
    use kelta_proto::model::{RunClaim, RunState};
    let s = std::sync::Arc::new(Store::open_in_memory().unwrap());
    let claim = |sid: &str| {
        let mut w = kelta_proto::samples::work_item();
        w.id = WorkItemId::generate();
        w.steps.clear();
        w.run = Some(RunClaim {
            name: Some("kelta-ticket-pass".into()),
            run_id: None,
            session_id: Some(SessionId::new(sid)),
            phase: Some("Pick".into()),
            state: RunState::Running,
            note: None,
            sha: None,
        });
        w
    };
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let (s, w) = (s.clone(), claim(&format!("s{i}")));
            tokio::spawn(async move { s.claim_item(&w).await.unwrap() })
        })
        .collect();
    let mut won = Vec::new();
    for t in tasks {
        if let Ok(w) = t.await.unwrap() {
            won.push(w);
        }
    }
    assert_eq!(won.len(), 1, "exactly one concurrent claim wins");
    let got = s.get_item(&won[0].id).await.unwrap().unwrap();
    assert_eq!(got.run, won[0].run);
    // Orphaned: the next claim takes the same item over.
    let mut orphan = got.clone();
    orphan.run.as_mut().unwrap().state = RunState::Orphaned;
    s.put_item(&orphan).await.unwrap();
    let again = s.claim_item(&claim("s9")).await.unwrap().unwrap();
    assert_eq!(again.id, got.id);
    assert_eq!(again.run.unwrap().session_id, Some(SessionId::new("s9")));
}

#[tokio::test]
async fn v17_backfills_the_ticket_source_once_from_the_only_source_on_the_account() {
    use kelta_proto::settings::Source;
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("kelta.db");
    let src = |id: &str, account: &str| {
        let mut s = Source { id: id.into(), ..Source::default() };
        s.tracker.account = account.into();
        s
    };
    let item = |id: &str, account: &str| {
        let mut w = kelta_proto::samples::work_item();
        w.id = WorkItemId::new(id);
        w.ticket = Some(TicketRef { account: account.into(), key: id.into(), id: id.into() });
        w.ticket_source = None;
        w
    };
    {
        let s = Store::open(&db).unwrap();
        for w in [item("a", "jira"), item("b", "linear"), item("c", "gone")] {
            s.put_item(&w).await.unwrap();
        }
        // back to a v16 database holding items started before sources were stored
        s.call(|c| {
            c.execute("DELETE FROM schema_version WHERE v = 17", []).map_err(kelta_core::store::db_err)
        })
        .await
        .unwrap();
    }
    let sources = vec![src("product", "jira"), src("eng", "linear"), src("ops", "linear")];
    async fn source_of(s: &Store, id: &str) -> Option<String> {
        s.get_item(&WorkItemId::new(id)).await.unwrap().unwrap().ticket_source
    }
    {
        let s = Store::open_with_sources(&db, sources.clone()).unwrap();
        assert_eq!(source_of(&s, "a").await.as_deref(), Some("product"), "the only source on jira");
        assert_eq!(source_of(&s, "b").await, None, "two sources on linear: unknown");
        assert_eq!(source_of(&s, "c").await, None, "no source on the account");
        s.call(|c| {
            c.execute("UPDATE work_items SET ticket_source = NULL", []).map_err(kelta_core::store::db_err)
        })
        .await
        .unwrap();
    }
    // once: a later open never guesses again
    let s = Store::open_with_sources(&db, sources).unwrap();
    assert_eq!(source_of(&s, "a").await, None);
}

#[tokio::test]
async fn work_store_round_trip() {
    let s = Store::open_in_memory().unwrap();
    let mut w = kelta_proto::samples::work_item();
    w.session_ids = vec![SessionId::new("s1"), SessionId::new("s2")];
    w.steps.clear();
    s.put_item(&w).await.unwrap();
    s.set_step(&w.id, "worktree", StepStatus::Done, None).await.unwrap();
    s.set_step(&w.id, "fetch_ticket", StepStatus::Failed, Some("boom".into())).await.unwrap();
    s.set_step(&w.id, "zz_custom", StepStatus::Pending, None).await.unwrap();
    let got = s.get_item(&w.id).await.unwrap().unwrap();
    assert_eq!(got.session_ids, w.session_ids);
    assert_eq!(got.ticket, w.ticket);
    assert_eq!(got.state, w.state);
    let steps: Vec<&str> = got.steps.iter().map(|x| x.step.as_str()).collect();
    assert_eq!(steps, vec!["fetch_ticket", "worktree", "zz_custom"]);
    assert_eq!(got.steps[0].detail.as_deref(), Some("boom"));

    assert!(!got.review_due && !got.claude_replied);
    assert_eq!(got.ticket_source, None, "an item without a source loads with none");
    assert_eq!(got.feedback_at, None, "an item never sent feedback loads with none");

    let mut w2 = w.clone();
    w2.state = WorkState::Failed { step: "x".into(), message: "y".into() };
    w2.review_due = true;
    w2.claude_at = Some("2026-10-10T09:00:00Z".into());
    w2.claude_message = Some("Done.\n\nNext: tests.".into());
    w2.delta =
        Some(kelta_proto::model::ReviewDelta { lines: 212, files: 9, generated: 1800, ..Default::default() });
    w2.next_note = Some("check the retry path".into());
    w2.left_at = Some("2026-10-10T09:30:00Z".into());
    w2.ticket_source = Some("product".into());
    w2.feedback_at = Some("2026-10-10T08:00:00Z".into());
    s.put_item(&w2).await.unwrap();
    let got = s.get_item(&w.id).await.unwrap().unwrap();
    assert_eq!(got.ticket_source.as_deref(), Some("product"));
    assert_eq!(got.feedback_at.as_deref(), Some("2026-10-10T08:00:00Z"));
    assert_eq!((got.state.clone(), got.review_due, got.claude_replied), (w2.state.clone(), true, false));
    assert_eq!(got.claude_at, w2.claude_at);
    assert_eq!(
        (&got.claude_message, got.delta, &got.next_note, &got.left_at),
        (&w2.claude_message, w2.delta, &w2.next_note, &w2.left_at)
    );
    assert_eq!(s.list_items(Some(&w.project_id)).await.unwrap().len(), 1);
    assert_eq!(s.list_items(Some(&"other".into())).await.unwrap().len(), 0);
    assert!(s.get_item(&WorkItemId::new("nope")).await.unwrap().is_none());
}

#[tokio::test]
async fn corrupt_work_row_is_skipped_by_list_only() {
    let s = Store::open_in_memory().unwrap();
    let good = kelta_proto::samples::work_item();
    let bad = kelta_proto::model::WorkItem { id: WorkItemId::new("bad"), ..good.clone() };
    s.put_item(&good).await.unwrap();
    s.put_item(&bad).await.unwrap();
    s.call(|c| {
        c.execute("UPDATE work_items SET state_json = '{\"kind\":\"from_the_future\"}' WHERE id = 'bad'", [])
            .map_err(kelta_core::store::db_err)
    })
    .await
    .unwrap();
    let ids: Vec<WorkItemId> = s.list_items(None).await.unwrap().into_iter().map(|w| w.id).collect();
    assert_eq!(ids, [good.id]);
    assert!(s.get_item(&WorkItemId::new("bad")).await.is_err(), "a direct read still reports it");
}

#[tokio::test]
async fn grant_and_trust_stores() {
    let s = Store::open_in_memory().unwrap();
    let p = PluginId::new("tools-pack");
    s.grant(&p, &["projects.read".into(), "notify".into()], "sha1").await.unwrap();
    s.grant(&p, &["notify".into(), "ui.open".into()], "sha2").await.unwrap();
    let g = s.grants(&p).await.unwrap();
    assert_eq!(g.len(), 3);
    assert!(g.iter().all(|x| x.manifest_sha256 == "sha2"));
    s.revoke_all(&p).await.unwrap();
    assert!(s.grants(&p).await.unwrap().is_empty());

    let path = Path::new("/r/.kelta/config.toml");
    assert_eq!(s.trusted_hash(path).await.unwrap(), None);
    s.set_trust(path, Some("abc".into())).await.unwrap();
    assert_eq!(s.trusted_hash(path).await.unwrap().as_deref(), Some("abc"));
    s.set_trust(path, None).await.unwrap();
    assert_eq!(s.trusted_hash(path).await.unwrap(), None);
}

#[tokio::test]
async fn plugin_kv_is_namespaced_capped_in_bytes_and_cleared() {
    let s = Store::open_in_memory().unwrap();
    let (a, b) = (PluginId::new("a"), PluginId::new("b"));
    s.kv_set(&a, "k", "1".into(), 100).await.unwrap();
    s.kv_set(&a, "k", "2".into(), 100).await.unwrap();
    assert_eq!(s.kv_get(&a, "k").await.unwrap().as_deref(), Some("2"));
    assert_eq!(s.kv_get(&b, "k").await.unwrap(), None);
    // Quota counts key + value bytes ("é" is 2): "k2" + "x" + 48 * 2 = 99 fits, one more char does not.
    s.kv_set(&a, "x", "é".repeat(48), 100).await.unwrap();
    assert!(s.kv_set(&a, "x", "é".repeat(49), 100).await.is_err());
    assert_eq!(s.kv_get(&a, "x").await.unwrap().unwrap().len(), 96, "refused write kept the old value");
    s.kv_set(&b, "k", "1".into(), 100).await.unwrap();
    assert_eq!(s.kv_keys(&a).await.unwrap(), vec!["k", "x"]);
    s.kv_delete(&a, "x").await.unwrap();
    assert_eq!(s.kv_keys(&a).await.unwrap(), vec!["k"]);
    s.kv_clear(&a).await.unwrap();
    assert!(s.kv_keys(&a).await.unwrap().is_empty());
    assert_eq!(s.kv_keys(&b).await.unwrap(), vec!["k"]);
}

#[tokio::test]
async fn trigger_log_is_capped() {
    let s = Store::open_in_memory().unwrap();
    s.call(|c| {
        for i in 0..1005 {
            q::trigger_log_append(
                c,
                &TriggerLogRow {
                    id: 0,
                    ts: kelta_proto::now_rfc3339(),
                    trigger_id: format!("t{i}"),
                    event: "app.started".into(),
                    ok: true,
                    detail: None,
                    depth: 0,
                },
            )?;
        }
        Ok(())
    })
    .await
    .unwrap();
    let all = s.call(|c| q::trigger_log(c, 5000)).await.unwrap();
    assert_eq!(all.len(), 1000);
    assert_eq!(all[0].trigger_id, "t1004");
}

#[tokio::test]
async fn startup_prunes_old_cache_rows_and_seen_reviews() {
    use kelta_proto::store::{ProviderCacheRow, SeenReviewRow};
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("kelta.db");
    let now = kelta_proto::now_rfc3339();
    let cache = |key: &str, at: &str| ProviderCacheRow {
        key: key.into(),
        etag: None,
        body_json: "[]".into(),
        fetched_at: at.into(),
    };
    let seen = |number: u64, at: &str| SeenReviewRow {
        account: "gh".into(),
        repo: "acme/shop".into(),
        number,
        head_sha: "sha".into(),
        first_seen: at.into(),
    };
    {
        let s = Store::open(&db).unwrap();
        let rows = [cache("old", "2020-01-01T00:00:00Z"), cache("fresh", &now)];
        let seens = [seen(1, "2020-01-01T00:00:00Z"), seen(2, &now)];
        s.call(move |c| {
            for r in &rows {
                q::cache_put(c, r)?;
            }
            for r in &seens {
                q::seen_review_put(c, r)?;
            }
            // Approved in Kelta long ago: the stamp must outlive the 90-day prune.
            let rr = kelta_proto::codehost::ReviewRef {
                account: "gh".into(),
                repo: "acme/shop".into(),
                number: 3,
            };
            q::seen_review_stamp(c, &rr, "sha")?;
            c.execute("UPDATE seen_reviews SET first_seen = '2020-01-01T00:00:00Z' WHERE number = 3", [])
                .map_err(kelta_core::store::db_err)?;
            Ok(())
        })
        .await
        .unwrap();
    }
    let s = Store::open(&db).unwrap();
    assert!(s.call(|c| q::cache_get(c, "old")).await.unwrap().is_none());
    assert!(s.call(|c| q::cache_get(c, "fresh")).await.unwrap().is_some());
    let left: Vec<u64> =
        s.call(|c| q::seen_reviews(c, "gh")).await.unwrap().iter().map(|r| r.number).collect();
    assert_eq!(left, [2, 3]);
}
