//! Layout tree operations (ARCHITECTURE §5.1), mirroring `ui/src/lib/layout` for backend-initiated
//! changes (`layout_open`, templates, session moves). Pure functions over `Layout`.

use kelta_proto::ids::{PaneId, ProjectId, SessionId, TabId};
use kelta_proto::model::{Layout, LayoutNode, OpenPaneRequest, PaneContent, Placement, SplitDir, Tab};

pub const MIN_RATIO: f32 = 0.05;
const EPS: f32 = 1e-6;

pub fn empty(project: &ProjectId) -> Layout {
    Layout { project_id: project.clone(), tabs: Vec::new(), active_tab: None, rev: 0 }
}

pub fn equal_ratios(n: usize) -> Vec<f32> {
    if n == 0 { Vec::new() } else { vec![1.0 / n as f32; n] }
}

/// Finite, ≥ `min`, summing to 1 (proportions kept where possible).
pub fn normalize_ratios(ratios: &[f32], min: f32) -> Vec<f32> {
    let n = ratios.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    let floor = min.min(1.0 / n as f32);
    let clean: Vec<f32> = ratios.iter().map(|r| if r.is_finite() && *r > 0.0 { *r } else { 0.0 }).collect();
    let sum: f32 = clean.iter().sum();
    if sum <= EPS {
        return equal_ratios(n);
    }
    let mut out: Vec<f32> = clean.iter().map(|r| r / sum).collect();
    for _ in 0..n {
        let low: Vec<bool> = out.iter().map(|r| *r < floor - EPS).collect();
        if !low.iter().any(|b| *b) {
            break;
        }
        let fixed = low.iter().filter(|b| **b).count() as f32 * floor;
        let rest: f32 = out.iter().zip(&low).filter(|(_, l)| !**l).map(|(r, _)| *r).sum();
        let scale = if rest > EPS { (1.0 - fixed) / rest } else { 0.0 };
        out = out.iter().zip(&low).map(|(r, l)| if *l { floor } else { r * scale }).collect();
    }
    let total: f32 = out.iter().sum();
    out.iter().map(|r| r / total).collect()
}

pub fn pane(content: PaneContent) -> LayoutNode {
    LayoutNode::Pane { id: PaneId::generate(), content }
}

pub fn all_panes(node: &LayoutNode) -> Vec<(&PaneId, &PaneContent)> {
    let mut out = Vec::new();
    fn walk<'a>(n: &'a LayoutNode, out: &mut Vec<(&'a PaneId, &'a PaneContent)>) {
        match n {
            LayoutNode::Pane { id, content } => out.push((id, content)),
            LayoutNode::Split { children, .. } => children.iter().for_each(|c| walk(c, out)),
        }
    }
    walk(node, &mut out);
    out
}

fn first_pane(node: &LayoutNode) -> Option<PaneId> {
    all_panes(node).first().map(|(id, _)| (*id).clone())
}

/// Session ids shown by terminal panes of a tree.
pub fn sessions_in(node: &LayoutNode) -> Vec<SessionId> {
    all_panes(node)
        .into_iter()
        .filter_map(|(_, c)| match c {
            PaneContent::Terminal { session_id } => Some(session_id.clone()),
            _ => None,
        })
        .collect()
}

/// Every session id referenced by the layout.
pub fn layout_sessions(l: &Layout) -> Vec<SessionId> {
    l.tabs.iter().flat_map(|t| sessions_in(&t.root)).collect()
}

pub fn active_tab(l: &Layout) -> Option<&Tab> {
    l.active_tab.as_ref().and_then(|id| l.tabs.iter().find(|t| &t.id == id)).or_else(|| l.tabs.first())
}

/// Sessions on screen in this layout: the active tab (only the zoomed pane when zoomed).
pub fn visible_sessions(l: &Layout) -> Vec<SessionId> {
    let Some(tab) = active_tab(l) else { return Vec::new() };
    if let Some(z) = &tab.zoomed_pane
        && let Some((_, PaneContent::Terminal { session_id })) =
            all_panes(&tab.root).into_iter().find(|(id, _)| *id == z)
    {
        return vec![session_id.clone()];
    }
    sessions_in(&tab.root)
}

/// Content of visible panes of the active tab (scheduler subscriptions).
pub fn visible_contents(l: &Layout) -> Vec<PaneContent> {
    let Some(tab) = active_tab(l) else { return Vec::new() };
    let panes = all_panes(&tab.root);
    if let Some(z) = &tab.zoomed_pane
        && let Some((_, c)) = panes.iter().find(|(id, _)| *id == z)
    {
        return vec![(*c).clone()];
    }
    panes.into_iter().map(|(_, c)| c.clone()).collect()
}

fn path_to(node: &LayoutNode, pane_id: &PaneId) -> Option<Vec<usize>> {
    match node {
        LayoutNode::Pane { id, .. } => (id == pane_id).then(Vec::new),
        LayoutNode::Split { children, .. } => children.iter().enumerate().find_map(|(i, c)| {
            path_to(c, pane_id).map(|mut p| {
                p.insert(0, i);
                p
            })
        }),
    }
}

fn node_at_mut<'a>(node: &'a mut LayoutNode, path: &[usize]) -> Option<&'a mut LayoutNode> {
    let Some((head, rest)) = path.split_first() else { return Some(node) };
    match node {
        LayoutNode::Split { children, .. } => children.get_mut(*head).and_then(|c| node_at_mut(c, rest)),
        LayoutNode::Pane { .. } => None,
    }
}

/// Split `pane_id` in `dir` and insert `new` after it (sibling when the parent has the same dir).
pub fn split_pane(root: &mut LayoutNode, pane_id: &PaneId, dir: SplitDir, new: LayoutNode) -> bool {
    let Some(path) = path_to(root, pane_id) else { return false };
    if let Some((index, parent_path)) = path.split_last()
        && let Some(LayoutNode::Split { dir: pdir, ratios, children }) = node_at_mut(root, parent_path)
        && *pdir == dir
    {
        let old = ratios.get(*index).copied().unwrap_or(1.0 / children.len() as f32);
        children.insert(index + 1, new);
        ratios[*index] = old * 0.5;
        ratios.insert(index + 1, old * 0.5);
        *ratios = normalize_ratios(ratios, MIN_RATIO);
        return true;
    }
    let Some(target) = node_at_mut(root, &path) else { return false };
    let old = std::mem::replace(target, LayoutNode::Split { dir, ratios: Vec::new(), children: Vec::new() });
    *target = LayoutNode::Split { dir, ratios: vec![0.5, 0.5], children: vec![old, new] };
    true
}

/// Replace the content of a pane.
pub fn replace_content(root: &mut LayoutNode, pane_id: &PaneId, content: PaneContent) -> bool {
    let Some(path) = path_to(root, pane_id) else { return false };
    match node_at_mut(root, &path) {
        Some(LayoutNode::Pane { content: c, .. }) => {
            *c = content;
            true
        }
        _ => false,
    }
}

fn remove_node(node: LayoutNode, pane_id: &PaneId) -> Option<LayoutNode> {
    match node {
        LayoutNode::Pane { ref id, .. } if id == pane_id => None,
        LayoutNode::Pane { .. } => Some(node),
        LayoutNode::Split { dir, ratios, children } => {
            let mut kept = Vec::new();
            let mut kept_r = Vec::new();
            for (i, c) in children.into_iter().enumerate() {
                if let Some(n) = remove_node(c, pane_id) {
                    kept.push(n);
                    kept_r.push(ratios.get(i).copied().unwrap_or(0.0));
                }
            }
            (!kept.is_empty()).then_some(LayoutNode::Split { dir, ratios: kept_r, children: kept })
        }
    }
}

/// Canonical form: single-child splits collapse, same-direction nesting flattens.
pub fn normalize_tree(node: LayoutNode) -> LayoutNode {
    match node {
        LayoutNode::Pane { .. } => node,
        LayoutNode::Split { dir, ratios, children } => {
            let base = if ratios.len() == children.len() { ratios } else { equal_ratios(children.len()) };
            let shares = normalize_ratios(&base, 0.0);
            let mut out_c = Vec::new();
            let mut out_r = Vec::new();
            for (i, c) in children.into_iter().enumerate() {
                let share = shares.get(i).copied().unwrap_or(0.0);
                match normalize_tree(c) {
                    LayoutNode::Split { dir: d, ratios: r, children: cc } if d == dir => {
                        for (j, gc) in cc.into_iter().enumerate() {
                            out_c.push(gc);
                            out_r.push(share * r.get(j).copied().unwrap_or(0.0));
                        }
                    }
                    other => {
                        out_c.push(other);
                        out_r.push(share);
                    }
                }
            }
            if out_c.len() == 1 {
                return out_c.remove(0);
            }
            LayoutNode::Split { dir, ratios: normalize_ratios(&out_r, MIN_RATIO), children: out_c }
        }
    }
}

fn fix_tab_refs(tab: &mut Tab) {
    let ids: Vec<PaneId> = all_panes(&tab.root).into_iter().map(|(id, _)| id.clone()).collect();
    if tab.focused_pane.as_ref().is_none_or(|f| !ids.contains(f)) {
        tab.focused_pane = ids.first().cloned();
    }
    if tab.zoomed_pane.as_ref().is_some_and(|z| !ids.contains(z)) {
        tab.zoomed_pane = None;
    }
}

/// Close a pane; the tab goes away with its last pane.
pub fn close_pane(l: &mut Layout, tab_id: &TabId, pane_id: &PaneId) {
    let Some(idx) = l.tabs.iter().position(|t| &t.id == tab_id) else { return };
    let root = std::mem::replace(
        &mut l.tabs[idx].root,
        LayoutNode::Pane { id: PaneId::new(""), content: PaneContent::Empty },
    );
    match remove_node(root, pane_id) {
        Some(r) => {
            l.tabs[idx].root = normalize_tree(r);
            fix_tab_refs(&mut l.tabs[idx]);
        }
        None => {
            l.tabs.remove(idx);
            if l.active_tab.as_ref() == Some(tab_id) {
                l.active_tab = l.tabs.get(idx.min(l.tabs.len().saturating_sub(1))).map(|t| t.id.clone());
            }
        }
    }
}

/// Where a content is shown.
pub fn find_content(l: &Layout, pred: impl Fn(&PaneContent) -> bool) -> Option<(TabId, PaneId)> {
    l.tabs.iter().find_map(|t| {
        all_panes(&t.root).into_iter().find(|(_, c)| pred(c)).map(|(p, _)| (t.id.clone(), p.clone()))
    })
}

pub fn find_session(l: &Layout, sid: &SessionId) -> Option<(TabId, PaneId)> {
    find_content(l, |c| matches!(c, PaneContent::Terminal { session_id } if session_id == sid))
}

pub fn focus_pane(l: &mut Layout, tab_id: &TabId, pane_id: &PaneId) {
    if let Some(t) = l.tabs.iter_mut().find(|t| &t.id == tab_id)
        && path_to(&t.root, pane_id).is_some()
    {
        t.focused_pane = Some(pane_id.clone());
        l.active_tab = Some(tab_id.clone());
    }
}

pub fn default_title(c: &PaneContent) -> String {
    match c {
        PaneContent::Terminal { .. } => "Terminal".into(),
        PaneContent::Web { .. } => "Web".into(),
        PaneContent::PluginScreen { screen_id, .. } => screen_id.clone(),
        PaneContent::Tickets { mode, .. } => {
            if *mode == kelta_proto::model::TicketsMode::Board {
                "Board".into()
            } else {
                "Tickets".into()
            }
        }
        PaneContent::TicketDetail { ticket } => ticket.key.clone(),
        PaneContent::Reviews { .. } => "Reviews".into(),
        PaneContent::ReviewDetail { review } => format!("{}#{}", review.repo, review.number),
        PaneContent::Inbox => "Inbox".into(),
        PaneContent::WorkItem { .. } => "Work item".into(),
        PaneContent::Settings { .. } => "Settings".into(),
        PaneContent::Diagnostics => "Diagnostics".into(),
        PaneContent::Welcome => "Welcome".into(),
        PaneContent::Empty => "Empty".into(),
    }
}

fn new_tab(title: String, root: LayoutNode, work_item_id: Option<kelta_proto::ids::WorkItemId>) -> Tab {
    let focused = first_pane(&root);
    Tab { id: TabId::generate(), title, work_item_id, root, focused_pane: focused, zoomed_pane: None }
}

/// Apply an `OpenPaneRequest` with a whole subtree (templates) or a single pane.
/// Returns `(tab, pane)` of the first pane of `node`.
pub fn open_node(l: &mut Layout, node: LayoutNode, req: &OpenPaneRequest) -> (TabId, PaneId) {
    let first = first_pane(&node).unwrap_or_else(|| PaneId::new(""));
    let existing = find_content(l, |c| c == &req.content);
    if let Some((t, p)) = &existing
        && req.placement == Placement::Focused
    {
        if req.focus {
            focus_pane(l, t, p);
        }
        return (t.clone(), p.clone());
    }
    let current =
        active_tab(l).map(|t| (t.id.clone(), t.focused_pane.clone().or_else(|| first_pane(&t.root))));
    let location = match (current, req.placement) {
        (Some((tab_id, Some(focused))), Placement::ReplaceFocused)
            if matches!(node, LayoutNode::Pane { .. }) =>
        {
            if let Some(t) = l.tabs.iter_mut().find(|t| t.id == tab_id) {
                replace_content(&mut t.root, &focused, req.content.clone());
            }
            (tab_id, focused)
        }
        (
            Some((tab_id, Some(focused))),
            Placement::SplitRight | Placement::SplitDown | Placement::ReplaceFocused,
        ) => {
            let dir = if req.placement == Placement::SplitDown { SplitDir::Column } else { SplitDir::Row };
            if let Some(t) = l.tabs.iter_mut().find(|t| t.id == tab_id) {
                if req.placement == Placement::ReplaceFocused {
                    let path = path_to(&t.root, &focused);
                    if let Some(target) = path.and_then(|p| node_at_mut(&mut t.root, &p)) {
                        *target = node;
                    }
                    t.root = normalize_tree(std::mem::replace(
                        &mut t.root,
                        LayoutNode::Pane { id: PaneId::new(""), content: PaneContent::Empty },
                    ));
                } else {
                    split_pane(&mut t.root, &focused, dir, node);
                    t.root = normalize_tree(std::mem::replace(
                        &mut t.root,
                        LayoutNode::Pane { id: PaneId::new(""), content: PaneContent::Empty },
                    ));
                }
                fix_tab_refs(t);
            }
            (tab_id, first.clone())
        }
        _ => {
            let title = req.tab_title.clone().unwrap_or_else(|| default_title(&req.content));
            let tab = new_tab(title, node, req.work_item_id.clone());
            let id = tab.id.clone();
            let activate = req.focus || l.tabs.is_empty();
            l.tabs.push(tab);
            if activate {
                l.active_tab = Some(id.clone());
            }
            (id, first.clone())
        }
    };
    // A session lives in at most one pane: vacate the old one.
    if let Some((t, p)) = existing
        && !(t == location.0 && p == location.1)
        && matches!(req.content, PaneContent::Terminal { .. })
    {
        close_pane(l, &t, &p);
    }
    if req.focus {
        focus_pane(l, &location.0, &location.1);
    }
    location
}

/// Remove every pane showing `sid` (session removed).
pub fn remove_session(l: &mut Layout, sid: &SessionId) -> bool {
    let mut changed = false;
    while let Some((t, p)) = find_session(l, sid) {
        close_pane(l, &t, &p);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(s: &str) -> PaneContent {
        PaneContent::Terminal { session_id: SessionId::new(s) }
    }

    fn req(c: PaneContent, p: Placement) -> OpenPaneRequest {
        OpenPaneRequest { content: c, placement: p, focus: true, tab_title: None, work_item_id: None }
    }

    #[test]
    fn open_split_and_move() {
        let mut l = empty(&ProjectId::new("p"));
        open_node(&mut l, pane(term("a")), &req(term("a"), Placement::NewTab));
        open_node(&mut l, pane(term("b")), &req(term("b"), Placement::SplitRight));
        assert_eq!(l.tabs.len(), 1);
        assert_eq!(visible_sessions(&l), vec![SessionId::new("a"), SessionId::new("b")]);
        // moving "a" to a new tab vacates its old pane
        open_node(&mut l, pane(term("a")), &req(term("a"), Placement::NewTab));
        assert_eq!(l.tabs.len(), 2);
        assert_eq!(layout_sessions(&l), vec![SessionId::new("b"), SessionId::new("a")]);
        // focused placement focuses the existing pane
        let (t, _) = open_node(&mut l, pane(term("b")), &req(term("b"), Placement::Focused));
        assert_eq!(l.active_tab.as_ref(), Some(&t));
        assert!(remove_session(&mut l, &SessionId::new("b")));
        assert_eq!(l.tabs.len(), 1);
    }

    #[test]
    fn ratios_normalize() {
        let r = normalize_ratios(&[0.01, 0.99], MIN_RATIO);
        assert!((r[0] - 0.05).abs() < 1e-4 && (r.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert_eq!(normalize_ratios(&[0.0, 0.0], MIN_RATIO), vec![0.5, 0.5]);
    }
}
