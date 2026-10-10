//! Provider conformance contract, shared by the built-in providers' tests (wiremock) and by
//! `cargo run -p xtask -- kpp-check <plugin dir>` (process plugins, PLUGINS §9.4). Returns the
//! first broken rule instead of panicking so the author tool can report it.

use std::collections::HashSet;

use crate::api::{CodeHost, Tracker};
use crate::codehost::{CodeHostKind, PrCreate, ReviewKind, ReviewQuery};
use crate::error::ErrorCode;
use crate::settings::{TrackerBinding, TrackerView};
use crate::tracker::{Assignee, TicketRef, TrackerKind};

macro_rules! ensure {
    ($cond:expr, $($fmt:tt)+) => {
        if !$cond {
            return Err(format!($($fmt)+));
        }
    };
}

/// Inputs of the tracker contract.
pub struct TrackerCase {
    pub kind: TrackerKind,
    pub account_id: String,
    pub view: TrackerView,
    pub binding: TrackerBinding,
    /// Ticket used for detail / moves / writes; `None` = the first listed one.
    pub ticket: Option<TicketRef>,
    /// Expected `branch_key` of `ticket`; `None` = any non-empty key without whitespace.
    pub branch_key: Option<String>,
}

/// Every rule a `Tracker` must honour (read paths, discovered moves, column order, writes per caps).
pub async fn tracker_contract(t: &dyn Tracker, case: &TrackerCase) -> Result<(), String> {
    ensure!(t.kind() == case.kind, "kind: {:?}, expected {:?}", t.kind(), case.kind);
    let caps = t.caps();

    let me = t.me().await.map_err(|e| format!("me: {e}"))?;
    ensure!(!me.id.is_empty() && !me.name.is_empty(), "me: empty id or name");

    let page = t.list(&case.view, None).await.map_err(|e| format!("list: {e}"))?;
    ensure!(!page.items.is_empty(), "list is empty");
    let mut keys = HashSet::new();
    for it in &page.items {
        ensure!(it.r#ref.account.as_str() == case.account_id, "account on ticket {}", it.r#ref.key);
        ensure!(!it.r#ref.key.is_empty() && !it.r#ref.id.is_empty(), "ticket ids: {it:?}");
        ensure!(!it.title.is_empty() && !it.url.is_empty() && !it.updated_at.is_empty(), "ticket: {it:?}");
        ensure!(!it.status.name.is_empty(), "status name of {}", it.r#ref.key);
        ensure!(keys.insert(it.r#ref.key.clone()), "duplicate key {}", it.r#ref.key);
        ensure!(!t.browser_url(&it.r#ref).is_empty(), "browser_url of {}", it.r#ref.key);
        let bk = t.branch_key(&it.r#ref);
        ensure!(!bk.is_empty() && !bk.contains(char::is_whitespace), "branch_key {bk:?}");
    }

    let ticket = case.ticket.clone().unwrap_or_else(|| page.items[0].r#ref.clone());
    if let Some(want) = &case.branch_key {
        ensure!(&t.branch_key(&ticket) == want, "branch_key: {:?}, expected {want:?}", t.branch_key(&ticket));
    }
    let key_head = ticket.key.split('#').next().unwrap_or("");
    ensure!(
        t.browser_url(&ticket).contains(key_head),
        "browser_url {:?} lacks the key",
        t.browser_url(&ticket)
    );

    let d = t.get(&ticket).await.map_err(|e| format!("get: {e}"))?;
    ensure!(d.ticket.r#ref.key == ticket.key, "detail key {}", d.ticket.r#ref.key);
    ensure!(!d.body_html.to_ascii_lowercase().contains("<script"), "html must be sanitized");
    ensure!(d.comments.len() <= 20, "at most 20 comments");
    ensure!(!d.body_md.is_empty(), "empty body");
    for c in &d.comments {
        ensure!(!c.body_html.to_ascii_lowercase().contains("<script"), "comment html must be sanitized");
    }

    // moves are discovered, never assumed
    let ts = t.transitions(&ticket).await.map_err(|e| format!("transitions: {e}"))?;
    ensure!(!ts.is_empty(), "no transitions");
    let ids: HashSet<_> = ts.iter().map(|t| t.id.clone()).collect();
    ensure!(ids.len() == ts.len(), "duplicate transition ids");
    ensure!(ts.iter().all(|t| !t.id.is_empty() && !t.name.is_empty()), "transition fields");
    let moved = t.transition(&ticket, &ts[0].id, None).await.map_err(|e| format!("transition: {e}"))?;
    ensure!(moved.r#ref.key == ticket.key, "transition returns another ticket");

    let cols = t.columns(&case.binding).await.map_err(|e| format!("columns: {e}"))?;
    ensure!(!cols.is_empty(), "no columns");
    ensure!(
        cols.iter().map(|c| c.order).collect::<Vec<_>>() == (0..cols.len() as u32).collect::<Vec<_>>(),
        "column order must be 0..n"
    );

    // writes, as advertised by caps
    if caps.comment {
        t.comment(&ticket, "hello\nworld").await.map_err(|e| format!("comment: {e}"))?;
    } else {
        let code = t.comment(&ticket, "x").await.err().map(|e| e.code);
        ensure!(code == Some(ErrorCode::Unsupported), "comment without caps.comment: {code:?}");
    }
    if caps.assign {
        let a = t.assign(&ticket, Assignee::Me).await.map_err(|e| format!("assign: {e}"))?;
        ensure!(a.r#ref.key == ticket.key, "assign returns another ticket");
    } else {
        let code = t.assign(&ticket, Assignee::Me).await.err().map(|e| e.code);
        ensure!(code == Some(ErrorCode::Unsupported), "assign without caps.assign: {code:?}");
    }
    Ok(())
}

/// Inputs of the code-host contract.
pub struct CodeHostCase {
    pub kind: CodeHostKind,
    pub account_id: String,
    /// Repo used for create / find_for_branch; `None` = the repo of the first requested review.
    pub repo: Option<String>,
    /// Start of the fetch refspec of review `{n}` (the rest is `:<local branch>`); `None` = any.
    pub refspec: Option<String>,
}

pub fn pr_create(repo: &str) -> PrCreate {
    PrCreate {
        repo: repo.into(),
        head: "feature/x".into(),
        base: "main".into(),
        title: "SHOP-1 x".into(),
        body: "b".into(),
        draft: false,
    }
}

/// Every rule a `CodeHost` must honour (both lists, detail, review actions, create, find, refspec).
pub async fn code_host_contract(h: &dyn CodeHost, case: &CodeHostCase) -> Result<(), String> {
    ensure!(h.kind() == case.kind, "kind: {:?}, expected {:?}", h.kind(), case.kind);
    let me = h.me().await.map_err(|e| format!("me: {e}"))?;
    ensure!(!me.id.is_empty() && me.login.is_some(), "me: empty id or no login");
    h.changed_since_last().await.map_err(|e| format!("changed_since_last: {e}"))?;

    let mut first = None;
    for kind in [ReviewKind::ReviewRequested, ReviewKind::Authored] {
        let q = ReviewQuery { kind, include_team: true, include_drafts: false };
        let list = h.list_reviews(&q).await.map_err(|e| format!("list {kind:?}: {e}"))?;
        ensure!(!list.is_empty(), "{kind:?} list is empty");
        let mut seen = HashSet::new();
        for r in &list {
            let n = r.r#ref.number;
            ensure!(r.kind == kind, "kind of #{n}");
            ensure!(r.r#ref.account.as_str() == case.account_id, "account on #{n}");
            ensure!(n > 0 && !r.r#ref.repo.is_empty(), "ref of #{n}");
            ensure!(!r.title.is_empty() && !r.url.is_empty() && !r.head_sha.is_empty(), "review: {r:?}");
            ensure!(
                !r.source_branch.is_empty() && !r.target_branch.is_empty() && !r.updated_at.is_empty(),
                "review: {r:?}"
            );
            ensure!(!r.draft, "drafts are excluded when include_drafts is false (#{n})");
            ensure!(r.author.login.is_some(), "author login of #{n}");
            ensure!(seen.insert(r.r#ref.clone()), "duplicate ref #{n}");
        }
        if kind == ReviewKind::ReviewRequested {
            first = list.into_iter().next();
        }
    }
    let first = first.ok_or("no review")?;
    let repo = case.repo.clone().unwrap_or_else(|| first.r#ref.repo.clone());

    let d = h.get(&first.r#ref).await.map_err(|e| format!("get: {e}"))?;
    ensure!(d.review.r#ref == first.r#ref, "detail ref");
    ensure!(!d.body_html.to_ascii_lowercase().contains("<script"), "html must be sanitized");

    h.approve(&first.r#ref, &first.head_sha).await.map_err(|e| format!("approve: {e}"))?;
    h.comment(&first.r#ref, "hello").await.map_err(|e| format!("comment: {e}"))?;
    h.request_changes(&first.r#ref, "please fix").await.map_err(|e| format!("request_changes: {e}"))?;
    let created = h.create(&pr_create(&repo)).await.map_err(|e| format!("create: {e}"))?;
    ensure!(created.r#ref.number > 0 && created.kind == ReviewKind::Authored, "created: {created:?}");
    let found = h.find_for_branch(&repo, "feature/mine").await.map_err(|e| format!("find: {e}"))?;
    ensure!(found.is_some(), "find_for_branch found nothing");

    let spec = h.fetch_refspec(&first.r#ref, "kelta/review-1");
    let want = case.refspec.as_deref().unwrap_or("").replace("{n}", &first.r#ref.number.to_string());
    ensure!(spec.starts_with(&want) && spec.ends_with(":kelta/review-1"), "fetch_refspec {spec:?}");
    Ok(())
}
