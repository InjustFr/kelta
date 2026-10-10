//! SQLite store: migrations up from empty + idempotent, Work/Grant/Trust stores, trigger_log cap.
#![allow(clippy::unwrap_used)] // fixture helpers outside #[test] fns

use std::path::Path;

use kelta_core::store::{Store, migrations, q};
use kelta_proto::api::{GrantStore, TrustStore, WorkStore};
use kelta_proto::ids::{PluginId, SessionId, WorkItemId};
use kelta_proto::model::{StepStatus, WorkState};
use kelta_proto::store::{SCHEMA_VERSION, TriggerLogRow, tables};

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
    let v = s.call(|c| migrations::migrate(c).map_err(kelta_core::store::db_err)).await.unwrap();
    assert_eq!(v, SCHEMA_VERSION);
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

    let mut w2 = w.clone();
    w2.state = WorkState::Failed { step: "x".into(), message: "y".into() };
    w2.review_due = true;
    w2.claude_at = Some("2026-10-10T09:00:00Z".into());
    s.put_item(&w2).await.unwrap();
    let got = s.get_item(&w.id).await.unwrap().unwrap();
    assert_eq!((got.state, got.review_due, got.claude_replied), (w2.state.clone(), true, false));
    assert_eq!(got.claude_at, w2.claude_at);
    assert_eq!(s.list_items(Some(&w.project_id)).await.unwrap().len(), 1);
    assert_eq!(s.list_items(Some(&"other".into())).await.unwrap().len(), 0);
    s.delete_item(&w.id).await.unwrap();
    assert!(s.get_item(&w.id).await.unwrap().is_none());
    assert!(s.steps(&w.id).await.unwrap().is_empty());
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
