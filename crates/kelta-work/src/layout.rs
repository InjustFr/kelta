//! Session template expansion (SETTINGS `[[session_templates]]`) into `CoreApi::layout_open` calls.
//!
//! `layout_open` can only split the focused pane, so a tree is materialised top-down: the first leaf
//! of every child of a split is opened by splitting the previous sibling's first leaf, then each child
//! is expanded from its own first leaf. Ratios are left to core (equal split).

use kelta_proto::model::{Placement, SplitDir};
use kelta_proto::settings::{SessionTemplate, TemplateNode};

/// What a template leaf runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    Claude { profile: Option<String> },
    Editor,
    Shell { command: Option<String> },
    Setup,
    Tool { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// Pre-order leaf index.
    pub idx: usize,
    pub kind: SlotKind,
    pub name: Option<String>,
}

/// One pane operation: open `leaf` by splitting `anchor` in `dir` (`anchor = None` → tab root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub leaf: usize,
    pub anchor: Option<usize>,
    pub placement: Placement,
}

fn kind_of(session: &str, profile: &Option<String>, command: &Option<String>) -> SlotKind {
    match session {
        "claude" => SlotKind::Claude { profile: profile.clone() },
        "editor" => SlotKind::Editor,
        "setup" => SlotKind::Setup,
        s if s.starts_with("tool:") => SlotKind::Tool { id: s["tool:".len()..].to_owned() },
        _ => SlotKind::Shell { command: command.clone() },
    }
}

/// Leaves in pre-order.
pub fn slots(t: &TemplateNode) -> Vec<Slot> {
    fn walk(n: &TemplateNode, out: &mut Vec<Slot>) {
        match n {
            TemplateNode::Split { children, .. } => children.iter().for_each(|c| walk(c, out)),
            TemplateNode::Session { session, name, profile, command } => out.push(Slot {
                idx: out.len(),
                kind: kind_of(session, profile, command),
                name: name.clone(),
            }),
        }
    }
    let mut out = Vec::new();
    walk(t, &mut out);
    out
}

fn leaf_count(n: &TemplateNode) -> usize {
    match n {
        TemplateNode::Split { children, .. } => children.iter().map(leaf_count).sum(),
        TemplateNode::Session { .. } => 1,
    }
}

/// Pane operations in order; the first op is the tab root (`placement` given by the caller).
pub fn ops(t: &TemplateNode, root: Placement) -> Vec<Op> {
    fn expand(n: &TemplateNode, first: usize, out: &mut Vec<Op>) {
        let TemplateNode::Split { split, children, .. } = n else { return };
        let placement = match split {
            SplitDir::Row => Placement::SplitRight,
            SplitDir::Column => Placement::SplitDown,
        };
        let mut firsts = Vec::with_capacity(children.len());
        let mut at = first;
        for c in children {
            firsts.push(at);
            at += leaf_count(c);
        }
        for i in 1..firsts.len() {
            out.push(Op { leaf: firsts[i], anchor: Some(firsts[i - 1]), placement });
        }
        for (c, f) in children.iter().zip(firsts) {
            expand(c, f, out);
        }
    }
    let mut out = vec![Op { leaf: 0, anchor: None, placement: root }];
    expand(t, 0, &mut out);
    out
}

/// Template by id (enabled), with the built-in defaults as fallback.
pub fn template(all: &[SessionTemplate], id: &str) -> Option<SessionTemplate> {
    all.iter()
        .find(|t| t.id == id && t.enabled)
        .cloned()
        .or_else(|| kelta_proto::settings::default_session_templates().into_iter().find(|t| t.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_template_ops() {
        let t = template(&[], "review").unwrap();
        let s = slots(&t.layout);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].kind, SlotKind::Claude { profile: Some("review".into()) });
        assert_eq!(
            s[2].kind,
            SlotKind::Shell { command: Some("git diff --stat origin/{base}...HEAD".into()) }
        );
        let o = ops(&t.layout, Placement::NewTab);
        assert_eq!(
            o,
            vec![
                Op { leaf: 0, anchor: None, placement: Placement::NewTab },
                Op { leaf: 1, anchor: Some(0), placement: Placement::SplitRight },
                Op { leaf: 2, anchor: Some(1), placement: Placement::SplitDown },
            ]
        );
    }

    #[test]
    fn nested_column_of_rows() {
        let t = TemplateNode::Split {
            split: SplitDir::Column,
            ratios: vec![0.5, 0.5],
            children: vec![
                TemplateNode::Split {
                    split: SplitDir::Row,
                    ratios: vec![0.5, 0.5],
                    children: vec![TemplateNode::session("claude"), TemplateNode::session("editor")],
                },
                TemplateNode::session("shell"),
            ],
        };
        let o = ops(&t, Placement::NewTab);
        // shell (leaf 2) is split below claude (leaf 0) before editor splits claude to the right.
        assert_eq!(o[1], Op { leaf: 2, anchor: Some(0), placement: Placement::SplitDown });
        assert_eq!(o[2], Op { leaf: 1, anchor: Some(0), placement: Placement::SplitRight });
    }
}
