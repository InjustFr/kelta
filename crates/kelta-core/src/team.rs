//! The team behind a scope (SPEC §3.4a): people seen in cached tickets and PRs, identities
//! merged across trackers and code hosts, and who each ticket or PR waits on. Pure functions.

use std::collections::HashMap;

use kelta_proto::codehost::{PrState, Review, ReviewKind};
use kelta_proto::ids::AccountId;
use kelta_proto::tracker::{Person, PersonId, StatusCategory, Ticket, TicketItem, User, WaitWhy, Waiting};

/// One identity as a provider names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub account: AccountId,
    pub id: String,
    pub login: Option<String>,
    pub name: String,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
    /// Seen on a code host (else on a tracker).
    pub code_host: bool,
}

impl Ident {
    pub fn of(account: &AccountId, u: &User, code_host: bool) -> Self {
        Self {
            account: account.clone(),
            id: u.id.clone(),
            login: u.login.clone(),
            name: u.name.clone(),
            avatar_url: u.avatar_url.clone(),
            email: u.email.clone(),
            code_host,
        }
    }

    fn login_only(account: &AccountId, login: &str) -> Self {
        Self::of(
            account,
            &User { id: login.into(), name: login.into(), login: Some(login.into()), ..User::default() },
            true,
        )
    }
}

/// Who a scope's tickets and PRs mention: assignees, reporters, PR authors and asked reviewers.
pub fn seen<'a>(tickets: impl IntoIterator<Item = &'a Ticket>, reviews: &[Review]) -> Vec<Ident> {
    let mut out = Vec::new();
    for t in tickets {
        for u in t.assignee.iter().chain(&t.reporter) {
            out.push(Ident::of(&t.r#ref.account, u, false));
        }
    }
    for r in reviews {
        out.push(Ident::of(&r.r#ref.account, &r.author, true));
        // a team slug (`acme/backend`) is not a person
        for l in r.waiting_on.iter().filter(|l| !l.contains('/')) {
            out.push(Ident::login_only(&r.r#ref.account, l));
        }
    }
    out
}

fn same(a: &Option<String>, b: &Option<String>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if !a.is_empty() && a.eq_ignore_ascii_case(b))
}

/// Merge `seen` identities into people. `mes` are the accounts' own users (all become "You");
/// `hosts` is each account's normalized base URL. Rules, in order, each exact: me; same host and
/// login across a tracker and a code host; same email. Display names never merge people.
pub fn people(seen: Vec<Ident>, mes: Vec<Ident>, hosts: &HashMap<AccountId, String>) -> Vec<Person> {
    let mut seen = seen;
    seen.sort_by(|a, b| (&a.account, &a.id, a.code_host).cmp(&(&b.account, &b.id, b.code_host)));
    let mut all: Vec<(Ident, bool)> = Vec::new();
    for (i, me) in mes.iter().cloned().map(|i| (i, true)).chain(seen.into_iter().map(|i| (i, false))) {
        match all
            .iter_mut()
            .find(|(x, _)| (&x.account, &x.id, x.code_host) == (&i.account, &i.id, i.code_host))
        {
            Some((x, m)) => {
                x.login = x.login.take().or(i.login);
                x.avatar_url = x.avatar_url.take().or(i.avatar_url);
                x.email = x.email.take().or(i.email);
                *m |= me;
            }
            None => all.push((i, me)),
        }
    }
    // a seen identity is me when an account's own user has its id or login
    for (x, is_me) in &mut all {
        *is_me |= mes.iter().any(|m| {
            m.account == x.account && m.code_host == x.code_host && (m.id == x.id || same(&m.login, &x.login))
        });
    }
    let mut group: Vec<usize> = (0..all.len()).collect();
    for a in 0..all.len() {
        for b in a + 1..all.len() {
            let ((x, xm), (y, ym)) = (&all[a], &all[b]);
            let host = |i: &Ident| hosts.get(&i.account);
            let merge = (*xm && *ym)
                || (x.code_host != y.code_host
                    && host(x).is_some()
                    && host(x) == host(y)
                    && same(&x.login, &y.login))
                || same(&x.email, &y.email);
            if merge && group[a] != group[b] {
                let (old, new) = (group[b], group[a]);
                group.iter_mut().filter(|g| **g == old).for_each(|g| *g = new);
            }
        }
    }
    let mut out = Vec::new();
    let mut done = Vec::new();
    for g in group.iter().copied() {
        if done.contains(&g) {
            continue;
        }
        done.push(g);
        let members: Vec<&(Ident, bool)> =
            all.iter().zip(&group).filter(|(_, x)| **x == g).map(|(m, _)| m).collect();
        let me = members.iter().any(|(_, m)| *m);
        let named = members
            .iter()
            .map(|(i, _)| i)
            .filter(|i| !i.code_host)
            .chain(members.iter().map(|(i, _)| i).filter(|i| i.code_host));
        // the key follows the tracker identity, so a saved filter survives a code host merging in
        let first = &members.iter().map(|(i, _)| i).find(|i| !i.code_host).unwrap_or(&members[0].0);
        out.push(Person {
            key: if me { "me".into() } else { format!("{}:{}", first.account, first.id) },
            name: if me {
                "You".into()
            } else {
                named
                    .clone()
                    .find(|i| !i.name.is_empty())
                    .map_or_else(|| first.id.clone(), |i| i.name.clone())
            },
            avatar_url: named.clone().find_map(|i| i.avatar_url.clone()),
            ids: members
                .iter()
                .map(|(i, _)| PersonId {
                    account: i.account.clone(),
                    id: i.id.clone(),
                    login: i.login.clone(),
                    code_host: i.code_host,
                })
                .collect(),
            matched: me
                || (members.iter().any(|(i, _)| i.code_host) && members.iter().any(|(i, _)| !i.code_host)),
            me,
        });
    }
    out
}

fn find<'a>(people: &'a [Person], account: &AccountId, code_host: bool, ident: &str) -> Option<&'a Person> {
    people.iter().find(|p| {
        p.ids.iter().any(|i| {
            &i.account == account
                && i.code_host == code_host
                && (i.id == ident || i.login.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(ident)))
        })
    })
}

fn wait(
    people: &[Person],
    account: &AccountId,
    code_host: bool,
    ident: &str,
    why: WaitWhy,
    since: Option<String>,
) -> Waiting {
    match find(people, account, code_host, ident) {
        Some(p) => Waiting { person: p.key.clone(), name: p.name.clone(), why, since, matched: p.matched },
        None => {
            Waiting { person: format!("{account}:{ident}"), name: ident.into(), why, since, matched: false }
        }
    }
}

/// Who a PR waits on: the reviewers still asked (since `waiting_since`), then the author when
/// changes were requested (since that review).
// shortcut: one `since` for every reviewer, because `Review` keeps a single request time; carry one
// per reviewer if the chip must age them apart.
pub fn review_waiting(r: &Review, people: &[Person]) -> Vec<Waiting> {
    let mut out: Vec<Waiting> = r
        .waiting_on
        .iter()
        .map(|l| wait(people, &r.r#ref.account, true, l, WaitWhy::Review, r.waiting_since.clone()))
        .collect();
    if r.author_to_act {
        out.push(wait(
            people,
            &r.r#ref.account,
            true,
            r.author.login.as_deref().unwrap_or(&r.author.id),
            WaitWhy::Author,
            r.author_since.clone(),
        ));
    }
    out
}

/// A ticket not done and assigned to someone but me (no `since`: trackers give no assignment time).
fn assigned_wait(t: &Ticket, people: &[Person]) -> Option<Waiting> {
    let a = t.assignee.as_ref().filter(|_| t.status.category != StatusCategory::Done)?;
    Some(wait(people, &t.r#ref.account, false, &a.id, WaitWhy::Assigned, None)).filter(|w| w.person != "me")
}

/// A PR row's waits, then Assigned from its linked `tickets`. A review wait on me is dropped on
/// someone else's PR: the tab says it.
pub fn review_row_waiting(r: &Review, tickets: &[Ticket], people: &[Person]) -> Vec<Waiting> {
    let mut w = review_waiting(r, people);
    if r.kind == ReviewKind::ReviewRequested {
        w.retain(|w| !(w.person == "me" && w.why == WaitWhy::Review));
    }
    let assigned = tickets
        .iter()
        .filter(|t| r.linked_tickets.contains(&t.r#ref.key))
        .find_map(|t| assigned_wait(t, people));
    if let Some(a) = assigned
        && !w.iter().any(|x| x.person == a.person)
    {
        w.push(a);
    }
    w
}

/// A ticket's waits: its main open PR's, then Assigned (a ticket not done, on someone but me).
pub fn ticket_waiting(item: &TicketItem, reviews: &[Review], people: &[Person]) -> Vec<Waiting> {
    let mut out = item
        .prs
        .iter()
        .find(|p| p.state == PrState::Open)
        .and_then(|p| reviews.iter().find(|r| r.url == p.url))
        .map(|r| review_waiting(r, people))
        .unwrap_or_default();
    if let Some(w) = assigned_wait(&item.ticket, people)
        && !out.iter().any(|x| x.person == w.person)
    {
        out.push(w);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::codehost::{CiState, PrLink, PrSource};
    use kelta_proto::samples;

    fn id(
        account: &str,
        id: &str,
        login: Option<&str>,
        name: &str,
        email: Option<&str>,
        code_host: bool,
    ) -> Ident {
        Ident {
            account: account.into(),
            id: id.into(),
            login: login.map(Into::into),
            name: name.into(),
            avatar_url: None,
            email: email.map(Into::into),
            code_host,
        }
    }

    fn hosts() -> HashMap<AccountId, String> {
        [
            ("gh", "https://api.github.com"),
            ("gh-issues", "https://api.github.com"),
            ("jira", "https://acme.atlassian.net"),
        ]
        .into_iter()
        .map(|(a, h)| (a.into(), h.into()))
        .collect()
    }

    #[test]
    fn every_accounts_me_is_one_you() {
        let mes = vec![
            id("jira", "j1", Some("louis"), "Louis", None, false),
            id("gh", "louis-dev", Some("louis-dev"), "L", None, true),
        ];
        let seen = vec![
            id("gh", "louis-dev", Some("louis-dev"), "L", None, true),
            id("jira", "j2", None, "Anna", None, false),
        ];
        let p = people(seen, mes, &hosts());
        assert_eq!(p.len(), 2);
        let me = p.iter().find(|p| p.me).unwrap();
        assert_eq!((me.key.as_str(), me.name.as_str(), me.ids.len(), me.matched), ("me", "You", 2, true));
    }

    #[test]
    fn same_host_and_login_merge_ignoring_case() {
        let seen = vec![
            id("gh-issues", "Anna-Dev", Some("Anna-Dev"), "Anna", None, false),
            id("gh", "anna-dev", Some("anna-dev"), "anna-dev", None, true),
        ];
        let p = people(seen, vec![], &hosts());
        assert_eq!(p.len(), 1);
        assert!(p[0].matched);
        assert_eq!(p[0].name, "Anna", "the tracker's display name wins");
        // another host, same login: two strangers
        let seen = vec![
            id("jira", "j2", Some("anna-dev"), "Anna", None, false),
            id("gh", "anna-dev", Some("anna-dev"), "anna-dev", None, true),
        ];
        assert_eq!(people(seen, vec![], &hosts()).len(), 2);
    }

    #[test]
    fn matching_emails_merge() {
        let seen = vec![
            id("jira", "j2", Some("anna"), "Anna", Some("Anna@acme.test"), false),
            id("gh", "adev", Some("adev"), "adev", Some("anna@acme.test"), true),
        ];
        let p = people(seen, vec![], &hosts());
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].ids.len(), 2);
    }

    #[test]
    fn no_proof_keeps_two_unmatched_people() {
        let seen = vec![
            id("jira", "j2", Some("anna"), "Anna", None, false),
            id("gh", "adev", Some("adev"), "adev", None, true),
        ];
        let p = people(seen, vec![], &hosts());
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|p| !p.matched));
    }

    #[test]
    fn two_people_with_the_same_display_name_stay_two() {
        let seen = vec![
            id("jira", "j2", Some("alex1"), "Alex", None, false),
            id("jira", "j3", Some("alex2"), "Alex", None, false),
        ];
        assert_eq!(people(seen, vec![], &hosts()).len(), 2);
    }

    fn pr_item(url: &str) -> TicketItem {
        let mut i = TicketItem { ticket: samples::ticket(), ..TicketItem::default() };
        i.ticket.assignee = Some(User { id: "j2".into(), name: "Anna".into(), ..User::default() });
        i.prs = vec![PrLink {
            url: url.into(),
            account: None,
            repo: String::new(),
            number: 1,
            title: String::new(),
            branch: String::new(),
            state: PrState::Open,
            draft: false,
            ci: CiState::None,
            review: None,
            source: PrSource::KeyMatch,
        }];
        i
    }

    fn pr(kind: ReviewKind) -> Review {
        let mut r = samples::review();
        r.kind = kind;
        r.waiting_on = vec!["bob".into(), "louis".into()];
        r.requested_at = Some("2026-09-28T10:00:00Z".into());
        r
    }

    fn team() -> Vec<Person> {
        let mes = vec![id("github-work", "louis", Some("louis"), "Louis", None, true)];
        let seen = vec![
            id("github-work", "bob", Some("bob"), "bob", None, true),
            id("jira-acme", "j2", Some("anna"), "Anna", None, false),
        ];
        people(seen, mes, &HashMap::new())
    }

    #[test]
    fn review_and_author_waits_carry_their_since_on_every_kind() {
        let people = team();
        for kind in [ReviewKind::Authored, ReviewKind::ReviewRequested] {
            let mut r = pr(kind);
            r.waiting_since = Some("2026-09-28T10:00:00Z".into());
            r.author_to_act = true;
            r.author_since = Some("2026-09-29T09:00:00Z".into());
            r.author = User { id: "bob".into(), login: Some("bob".into()), ..User::default() };
            let w = review_row_waiting(&r, &[], &people);
            let got: Vec<_> = w.iter().map(|w| (w.name.as_str(), w.why, w.since.as_deref())).collect();
            let mut want = vec![("bob", WaitWhy::Review, Some("2026-09-28T10:00:00Z"))];
            // a wait on me is shown on my PRs only
            if kind == ReviewKind::Authored {
                want.push(("You", WaitWhy::Review, Some("2026-09-28T10:00:00Z")));
            }
            want.push(("bob", WaitWhy::Author, Some("2026-09-29T09:00:00Z")));
            assert_eq!(got, want, "{kind:?}");
        }
    }

    #[test]
    fn a_pr_waits_on_the_assignee_of_its_linked_ticket() {
        let people = team();
        let mut r = pr(ReviewKind::Authored);
        r.waiting_on.clear();
        let mut t = samples::ticket();
        t.assignee = Some(User { id: "j2".into(), name: "Anna".into(), ..User::default() });
        r.linked_tickets = vec![t.r#ref.key.clone()];
        let w = review_row_waiting(&r, std::slice::from_ref(&t), &people);
        assert_eq!(
            w.iter().map(|w| (w.name.as_str(), w.why, w.since.clone())).collect::<Vec<_>>(),
            [("Anna", WaitWhy::Assigned, None)]
        );
        assert!(review_row_waiting(&r, &[], &people).is_empty(), "no cached ticket, no wait");
    }

    #[test]
    fn a_saved_key_survives_a_code_host_identity_merging_in() {
        let jira = id("jira", "j2", None, "Anna", Some("anna@acme.test"), false);
        let before = people(vec![jira.clone()], vec![], &hosts());
        let gh = id("gh", "adev", Some("adev"), "adev", Some("anna@acme.test"), true);
        let after = people(vec![jira, gh], vec![], &hosts());
        assert_eq!((after.len(), after[0].key.as_str()), (1, "jira:j2"));
        assert_eq!(before[0].key, after[0].key);
    }

    #[test]
    fn ticket_waits_put_pr_waits_before_assigned_and_drop_assigned_on_me() {
        let people = team();
        let r = pr(ReviewKind::Authored);
        let w = ticket_waiting(&pr_item(&r.url), std::slice::from_ref(&r), &people);
        let got: Vec<_> = w.iter().map(|w| (w.name.as_str(), w.why)).collect();
        assert_eq!(got, [("bob", WaitWhy::Review), ("You", WaitWhy::Review), ("Anna", WaitWhy::Assigned)]);
        // assigned to me: no Assigned wait; to nobody waiting on a PR: assigned only
        let mut mine = pr_item("https://elsewhere");
        mine.ticket.assignee = Some(User { id: "louis".into(), ..User::default() });
        let mut me_people = team();
        me_people.iter_mut().find(|p| p.me).unwrap().ids.push(PersonId {
            account: mine.ticket.r#ref.account.clone(),
            id: "louis".into(),
            login: None,
            code_host: false,
        });
        assert!(ticket_waiting(&mine, &[], &me_people).is_empty());
        assert_eq!(ticket_waiting(&pr_item("https://elsewhere"), &[], &people).len(), 1);
        // done: nothing to wait for
        let mut done = pr_item("https://elsewhere");
        done.ticket.status.category = StatusCategory::Done;
        assert!(ticket_waiting(&done, &[], &people).is_empty());
    }
}
