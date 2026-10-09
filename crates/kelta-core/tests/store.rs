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
    assert_eq!(rows, SCHEMA_VERSION);
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

    assert!(!got.review_due);

    let mut w2 = w.clone();
    w2.state = WorkState::Merged { detail: Some("choose Done status".into()) };
    w2.review_due = true;
    s.put_item(&w2).await.unwrap();
    let got = s.get_item(&w.id).await.unwrap().unwrap();
    assert_eq!((got.state, got.review_due), (w2.state.clone(), true));
    assert_eq!(s.list_items(Some(&w.project_id)).await.unwrap().len(), 1);
    assert_eq!(s.list_items(Some(&"other".into())).await.unwrap().len(), 0);
    s.delete_item(&w.id).await.unwrap();
    assert!(s.get_item(&w.id).await.unwrap().is_none());
    assert!(s.steps(&w.id).await.unwrap().is_empty());
    assert!(s.get_item(&WorkItemId::new("nope")).await.unwrap().is_none());
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
