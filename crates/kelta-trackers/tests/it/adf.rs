//! ADF → Markdown snapshots, including unknown and malformed nodes.

use crate::support;

use kelta_trackers::adf::adf_to_markdown;

#[test]
fn rich_document() {
    insta::assert_snapshot!(adf_to_markdown(&support::fixture("adf/rich.json")));
}

#[test]
fn unknown_and_malformed_nodes_never_fail() {
    insta::assert_snapshot!(adf_to_markdown(&support::fixture("adf/unknown_nodes.json")));
}

#[test]
fn empty_and_degenerate_inputs() {
    assert_eq!(adf_to_markdown(&serde_json::json!({"type": "doc", "content": []})), "");
    assert_eq!(adf_to_markdown(&serde_json::json!({})), "");
    assert_eq!(adf_to_markdown(&serde_json::json!(null)), "");
}
