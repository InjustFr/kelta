//! Local tags (#349): stored by core, cleaned on the way in, kept across a restart.

use crate::common::*;
use kelta_proto::model::{LocalTag, LocalTags};
use kelta_proto::settings::Settings;

#[tokio::test]
async fn tags_survive_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![]);
    assert_eq!(h.core.tags_get().await.unwrap(), LocalTags::default());
    let set = LocalTags {
        tags: vec![LocalTag { name: "retest".into(), color: Some("theme:red".into()) }],
        tickets: [("jira:A-1".into(), vec!["retest".into(), "gone".into()])].into(),
        prs: [("gh:acme/shop#3".into(), vec!["retest".into()])].into(),
    };
    let stored = h.core.tags_set(set).await.unwrap();
    assert_eq!(stored.tickets["jira:A-1"], ["retest"], "an unknown tag is dropped");
    h.core.shutdown().await.unwrap();

    let h = start(tmp.path(), Settings::defaults(), vec![]);
    assert_eq!(h.core.tags_get().await.unwrap(), stored);
}
