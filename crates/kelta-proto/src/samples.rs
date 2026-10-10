//! Deterministic sample values of every DTO and event. `xtask codegen` writes them to
//! `crates/kelta-proto/fixtures/<name>.json`; tests round-trip each fixture through its Rust type
//! (and `ui/src/lib/gen/fixtures.test.ts` through TypeScript).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::api::PluginGrant;
use crate::codehost::*;
use crate::ctl::*;
use crate::error::*;
use crate::events::*;
use crate::ext::*;
use crate::hooks::HookPayload;
use crate::ids::*;
use crate::ipc::*;
use crate::model::*;
use crate::secret::SecretBackendStatus;
use crate::settings::*;
use crate::term::{
    HistoryHit, LoginEnv, LoginEnvSource, SessionTermStats, TerminalLimits, TerminalPalette, TerminalStats,
};
use crate::tracker::*;

/// One fixture: name (file stem), Rust type name, JSON value, and a round-trip function.
pub struct Fixture {
    pub name: &'static str,
    pub type_name: &'static str,
    pub value: Value,
    /// Deserialize into the Rust type and serialize back.
    pub roundtrip: fn(&Value) -> Result<Value, String>,
}

fn rt<T: Serialize + DeserializeOwned>(v: &Value) -> Result<Value, String> {
    let t: T = serde_json::from_value(v.clone()).map_err(|e| format!("deserialize: {e}"))?;
    serde_json::to_value(&t).map_err(|e| format!("serialize: {e}"))
}

fn val<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

macro_rules! fx {
    ($name:literal, $t:ty, $v:expr) => {
        Fixture { name: $name, type_name: stringify!($t), value: val::<$t>(&$v), roundtrip: rt::<$t> }
    };
}

// ---- building blocks -------------------------------------------------------------------------

pub const TS: &str = "2026-10-09T12:00:00Z";
pub const SID: &str = "01928f6e-2b4c-7a10-9c3d-5e6f70819203";
pub const SID2: &str = "01928f6e-2b4c-7a10-9c3d-5e6f70819204";
pub const WID: &str = "01928f6e-3c5d-7b20-8d4e-6f7081920314";
pub const CLAUDE_UUID: &str = "6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b";

pub fn user() -> User {
    User {
        id: "5b10ac8d82e05b22cc7d4ef5".into(),
        name: "Ada Lovelace".into(),
        login: Some("ada".into()),
        avatar_url: None,
    }
}

pub fn ticket_ref() -> TicketRef {
    TicketRef { account: AccountId::new("jira-acme"), key: "SHOP-142".into(), id: "10142".into() }
}

pub fn status(id: &str, name: &str, category: StatusCategory) -> Status {
    Status { id: id.into(), name: name.into(), category }
}

pub fn ticket() -> Ticket {
    Ticket {
        r#ref: ticket_ref(),
        title: "Rate-limit login".into(),
        url: "https://acme.atlassian.net/browse/SHOP-142".into(),
        status: status("3", "In Progress", StatusCategory::InProgress),
        kind: Some("Story".into()),
        assignee: Some(user()),
        labels: vec!["api".into()],
        priority: Some("High".into()),
        updated_at: TS.into(),
        project_hint: Some("SHOP".into()),
        priority_rank: Some(1),
        status_since: Some(TS.into()),
        sprint: Some(Sprint {
            id: "42".into(),
            name: "SHOP Sprint 12".into(),
            active: true,
            ends_at: Some("2026-10-16".into()),
        }),
        estimate: Some("3".into()),
        due: Some("2026-10-15".into()),
    }
}

pub fn review_ref() -> ReviewRef {
    ReviewRef { account: AccountId::new("github-work"), repo: "acme/shop-api".into(), number: 87 }
}

pub fn review() -> Review {
    Review {
        r#ref: review_ref(),
        title: "SHOP-140: Cache product prices".into(),
        url: "https://github.com/acme/shop-api/pull/87".into(),
        author: User { id: "bob".into(), name: "Bob".into(), login: Some("bob".into()), avatar_url: None },
        draft: false,
        head_sha: "3f2a9c1d8e7b6a5f4e3d2c1b0a998877665544aa".into(),
        source_branch: "feat/SHOP-140-cache-prices".into(),
        target_branch: "main".into(),
        ci: CiState::Success,
        decision: Some(ReviewDecision::ReviewRequired),
        my_state: Some(MyReviewState::Pending),
        mergeable: Some(true),
        reviewed_head: None,
        labels: vec![],
        kind: ReviewKind::ReviewRequested,
        updated_at: TS.into(),
        linked_tickets: vec!["SHOP-140".into()],
        additions: Some(120),
        deletions: Some(14),
        decision_head: None,
        requested_at: None,
        blocking: false,
    }
}

pub fn tracker_binding() -> TrackerBinding {
    TrackerBinding {
        account: AccountId::new("jira-acme"),
        views: vec![TrackerView {
            id: "mine".into(),
            label: "My open".into(),
            jql: Some("project = SHOP AND assignee = currentUser()".into()),
            ..TrackerView::default()
        }],
        columns: vec![
            ColumnSpec {
                id: "todo".into(),
                label: "To do".into(),
                categories: vec![StatusCategory::Todo],
                names: vec![],
            },
            ColumnSpec {
                id: "review".into(),
                label: "Review".into(),
                categories: vec![],
                names: vec!["In Review".into()],
            },
        ],
        status_map: StatusMap {
            start: Some(TransitionTarget::Category { category: StatusCategory::InProgress }),
            review: Some(TransitionTarget::Name { name: "In Review".into() }),
            done: Some(TransitionTarget::Category { category: StatusCategory::Done }),
        },
        repo_rules: vec![RepoRule {
            r#match: RepoMatch { component: Some("frontend".into()), ..RepoMatch::default() },
            repo: "web".into(),
        }],
    }
}

pub fn project_info() -> ProjectInfo {
    ProjectInfo {
        id: ProjectId::new("shop"),
        name: "Shop".into(),
        color: Some("#e07a5f".into()),
        icon: Some("S".into()),
        repos: vec![RepoInfo {
            id: "api".into(),
            path: PathBuf::from("/home/ada/code/shop-api"),
            primary: true,
            remote: "origin".into(),
            base: "main".into(),
            code_host: Some(CodeHostBinding {
                account: AccountId::new("github-work"),
                repo: "acme/shop-api".into(),
            }),
            exists: true,
        }],
        tracker: Some(tracker_binding()),
        open: true,
        active: true,
        attention: AttentionSummary { level: Attention::NeedsInput, needs_input: 1 },
        builtin: false,
    }
}

pub fn session_info() -> SessionInfo {
    SessionInfo {
        id: SessionId::new(SID),
        project_id: ProjectId::new("shop"),
        kind: SessionKind::Claude,
        name: "SHOP-142 claude".into(),
        title: Some("✳ Rate-limit login".into()),
        cwd: PathBuf::from("/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login"),
        status: SessionStatus::NeedsInput,
        status_source: StatusSource::Hook,
        attention: Attention::NeedsInput,
        seen: false,
        lifecycle: Lifecycle::Live,
        pid: Some(4242),
        exit_code: None,
        work_item_id: Some(WorkItemId::new(WID)),
        claude: Some(ClaudeMeta {
            session_uuid: CLAUDE_UUID.into(),
            model: Some("opus".into()),
            preview: Some("I need permission to run `cargo test`.".into()),
            files_touched: vec![PathBuf::from("src/login.rs")],
            hooks_active: true,
            usage: statusline(true).usage().map(|u| ClaudeUsage { unsaved_usd: u.cost_usd, ..u }),
        }),
        editor: None,
        cols: 120,
        rows: 40,
        created_at: TS.into(),
    }
}

pub fn layout() -> Layout {
    Layout {
        project_id: ProjectId::new("shop"),
        tabs: vec![
            Tab {
                id: TabId::new("tab-1"),
                title: "SHOP-142 Rate-limit login".into(),
                work_item_id: Some(WorkItemId::new(WID)),
                root: LayoutNode::Split {
                    dir: SplitDir::Row,
                    ratios: vec![0.5, 0.5],
                    children: vec![
                        LayoutNode::Pane {
                            id: PaneId::new("pane-1"),
                            content: PaneContent::Terminal { session_id: SessionId::new(SID) },
                        },
                        LayoutNode::Pane {
                            id: PaneId::new("pane-2"),
                            content: PaneContent::Terminal { session_id: SessionId::new(SID2) },
                        },
                    ],
                },
                focused_pane: Some(PaneId::new("pane-1")),
                zoomed_pane: None,
            },
            Tab {
                id: TabId::new("tab-2"),
                title: "Board".into(),
                work_item_id: None,
                root: LayoutNode::Pane {
                    id: PaneId::new("pane-3"),
                    content: PaneContent::Tickets {
                        scope: Scope::Project { id: ProjectId::new("shop") },
                        view_id: Some("mine".into()),
                        mode: TicketsMode::Board,
                        who: Some(Who::Mine),
                        group: Some(TicketGroupBy::Sprint),
                        sort: Some(TicketSort::Age),
                        person: Some("5b10ac8d82e05b22cc7d4ef5".into()),
                    },
                },
                focused_pane: None,
                zoomed_pane: None,
            },
        ],
        active_tab: Some(TabId::new("tab-1")),
        rev: 7,
    }
}

pub fn work_item() -> WorkItem {
    WorkItem {
        id: WorkItemId::new(WID),
        project_id: ProjectId::new("shop"),
        kind: WorkKind::Ticket,
        ticket: Some(ticket_ref()),
        review: None,
        repo_id: "api".into(),
        worktree: PathBuf::from("/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login"),
        branch: "feat/SHOP-142-rate-limit-login".into(),
        base: "main".into(),
        claude_uuid: Some(CLAUDE_UUID.into()),
        nvim_socket: Some(PathBuf::from("/tmp/kelta-1000/s/01928f6e/nvim.sock")),
        session_ids: vec![SessionId::new(SID), SessionId::new(SID2)],
        tab_id: Some(TabId::new("tab-1")),
        pr_url: None,
        state: WorkState::Active,
        steps: crate::model::WORK_STEPS
            .iter()
            .map(|s| WorkStepStatus {
                step: (*s).into(),
                status: StepStatus::Done,
                detail: None,
                updated_at: TS.into(),
            })
            .collect(),
        created_at: TS.into(),
        title: None,
        pr_title_needs_key: false,
        review_due: false,
        claude_replied: false,
        claude_at: None,
        sent_threads: Vec::new(),
        rebase: None,
        claude_message: None,
        delta: None,
        next_note: None,
        left_at: None,
        port_base: Some(20140),
        cost_usd: 3.5,
        auto_finish: false,
    }
}

/// Feedback on PR #74 (Fix with Claude).
pub fn feedback() -> Feedback {
    Feedback {
        threads: vec![FeedbackThread {
            id: "PRRT_kwDOA1".into(),
            author: "bob".into(),
            path: Some("src/login.rs".into()),
            line: Some(42),
            body_md: "bob: Reset the counter after a successful login.".into(),
            url: "https://github.com/acme/shop-api/pull/74#discussion_r1".into(),
        }],
        reviews: vec![FeedbackReview {
            author: "bob".into(),
            state: Some(MyReviewState::ChangesRequested),
            body_md: "Close, two things to fix.".into(),
        }],
        failed_checks: vec![FailedCheck {
            name: "ci / test".into(),
            url: Some("https://github.com/acme/shop-api/actions/runs/2".into()),
            log_tail: Some("test login::lockout ... FAILED".into()),
        }],
        reviewers: vec!["bob".into()],
    }
}

pub fn start_work_plan() -> StartWorkPlan {
    StartWorkPlan {
        project_id: ProjectId::new("shop"),
        source: WorkSource::Ticket { ticket: ticket_ref() },
        repo_id: "api".into(),
        repo_choices: vec!["api".into(), "web".into()],
        base: "main".into(),
        branch: "feat/SHOP-142-rate-limit-login".into(),
        branch_exists: None,
        worktree_path: PathBuf::from("/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login"),
        template_id: "claude+editor".into(),
        claude: ClaudePlan {
            profile: "default".into(),
            model: "opus".into(),
            effort: ClaudeEffort::High,
            permission_mode: PermissionMode::AcceptEdits,
            prompt: "Work on SHOP-142: Rate-limit login. The full ticket is in /tmp/kelta-1000/s/01928f6e/ticket.md. Read it, then propose a short plan before editing.".into(),
        },
        side_effects: SideEffects {
            assign_me: true,
            transition_to: Some(TransitionTarget::Category { category: StatusCategory::InProgress }),
            comment: None,
            run_setup: true,
        },
        existing: None,
        adopt_pr: None,
    }
}

pub fn plugin_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::new("sprint-burndown"),
        name: "Sprint Burndown".into(),
        version: "0.2.0".into(),
        description: "Burndown chart of the active sprint for the current project".into(),
        author: "Jane Doe".into(),
        license: "MIT".into(),
        homepage: None,
        kelta_api: "^0.1".into(),
        platforms: vec![PlatformName::Macos, PlatformName::Linux],
        permissions: vec![
            "tickets.read".into(),
            "settings.read".into(),
            "events:ticket.*".into(),
            "notify".into(),
        ],
        activation: vec!["onScreen:burndown".into(), "onCommand:sprint-burndown.open".into()],
        contributes: Contributes {
            screens: vec![ScreenDef {
                id: "burndown".into(),
                title: "Burndown".into(),
                icon: Some("chart".into()),
                entry: "dist/index.html".into(),
                scope: ScreenScope::Project,
                placement: vec![ScreenPlacement::Tab, ScreenPlacement::Pane],
                keep_alive: false,
                min_width: None,
            }],
            commands: vec![CommandDef {
                id: "sprint-burndown.open".into(),
                title: "Burndown: open".into(),
                r#do: vec![ActionDef::OpenScreen {
                    plugin: None,
                    screen: "burndown".into(),
                    params: None,
                    placement: Some(Placement::NewTab),
                }],
                ..CommandDef::default()
            }],
            keybindings: vec![KeybindingDef { command: "sprint-burndown.open".into(), key: "mod+b".into() }],
            triggers: vec![TriggerDef {
                id: "sprint-done-toast".into(),
                on: "ticket.transitioned".into(),
                r#match: BTreeMap::from([("payload.to.category".to_owned(), Matcher::Str("done".into()))]),
                r#do: vec![ActionDef::Toast { text: "{ticket.key} done".into(), level: None }],
                ..TriggerDef::default()
            }],
            settings: Some(SettingsContribution { schema: "settings.schema.json".into() }),
            ..Contributes::default()
        },
        provider: None,
    }
}

pub fn isl_tool() -> ToolDef {
    ToolDef {
        id: "isl".into(),
        label: "Sapling ISL".into(),
        icon: Some("branch".into()),
        kind: ToolKind::Web,
        start: Some(WebStart {
            command: "sl".into(),
            args: ["web", "--no-open", "--foreground", "--json", "--port", "{port}", "--cwd", "{repo.path}"]
                .iter()
                .map(|s| (*s).into())
                .collect(),
            cwd: None,
            env: BTreeMap::new(),
            ready: Ready::StdoutJson("url".into()),
            ready_timeout_ms: 10_000,
            stop: StopSpec::Signal { signal: "TERM".into(), grace_ms: 3000 },
        }),
        embed: Some(EmbedMode::Auto),
        url_is_secret: Some(true),
        lifecycle: WebLifecycle::OnClose,
        check: Some(vec!["sl".into(), "--version".into()]),
        install_hint: Some("https://sapling-scm.com/docs/introduction/installation".into()),
        ..ToolDef::default()
    }
}

pub fn trigger() -> TriggerDef {
    TriggerDef {
        id: "start-db".into(),
        on: "ticket.started".into(),
        r#match: BTreeMap::from([
            ("project.id".to_owned(), Matcher::Str("shop".into())),
            (
                "payload.status".to_owned(),
                Matcher::AnyOf(vec![Matcher::Str("needs_input".into()), Matcher::Str("!done".into())]),
            ),
        ]),
        r#do: vec![
            ActionDef::Run {
                command: "docker".into(),
                args: vec!["compose".into(), "up".into(), "-d".into(), "db".into()],
                cwd: Some("{worktree}".into()),
                env: BTreeMap::new(),
                stdin: None,
                timeout_ms: None,
                show: Some(RunShow::ToastOnError),
            },
            ActionDef::OpenTool { tool: "lazydocker".into(), placement: Some(Placement::SplitDown) },
            ActionDef::Prompt {
                text: "Remove worktree?".into(),
                yes: vec![ActionDef::Command { id: "kelta.work.finish".into() }],
                no: vec![],
            },
        ],
        debounce_ms: Some(1500),
        ..TriggerDef::default()
    }
}

fn hook(event: &str, extra: Value) -> HookPayload {
    let mut base = json!({
        "hook_event_name": event,
        "session_id": CLAUDE_UUID,
        "transcript_path": "/home/ada/.claude/projects/x/6f1d2c3b.jsonl",
        "cwd": "/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login",
        "permission_mode": "acceptEdits",
    });
    if let (Value::Object(b), Value::Object(e)) = (&mut base, extra) {
        b.extend(e);
    }
    serde_json::from_value(base).unwrap_or_default()
}

/// The statusline JSON of Claude Code 2.1.296 as `kelta-ctl statusline` relays it; `rate_limits`
/// is absent for API-key accounts.
pub fn statusline(subscription: bool) -> HookPayload {
    let mut extra = json!({
        "model": {"id": "claude-opus-4-1", "display_name": "Opus"},
        "workspace": {"current_dir": "/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login", "project_dir": "/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login", "added_dirs": []},
        "version": "2.1.296",
        "output_style": {"name": "default"},
        "cost": {"total_cost_usd": 1.8412, "total_duration_ms": 512000, "total_api_duration_ms": 98000, "total_lines_added": 210, "total_lines_removed": 40},
        "context_window": {"total_input_tokens": 144000, "total_output_tokens": 9100, "context_window_size": 200000, "current_usage": {"input_tokens": 12, "output_tokens": 410, "cache_creation_input_tokens": 2100, "cache_read_input_tokens": 141888}, "used_percentage": 72, "remaining_percentage": 28},
        "exceeds_200k_tokens": false,
        "fast_mode": false,
        "thinking": {"enabled": true},
    });
    if subscription {
        extra["rate_limits"] = json!({
            "five_hour": {"used_percentage": 64.2, "resets_at": 1_791_637_800},
            "seven_day": {"used_percentage": 31, "resets_at": 1_792_051_200},
        });
    }
    hook("Status", extra)
}

pub fn ui_event_samples() -> Vec<(&'static str, UiEvent)> {
    vec![
        ("ui_event_session_updated", UiEvent::SessionUpdated { session: session_info() }),
        ("ui_event_session_removed", UiEvent::SessionRemoved { id: SessionId::new(SID2) }),
        (
            "ui_event_attention_changed",
            UiEvent::AttentionChanged {
                project_id: ProjectId::new("shop"),
                level: Attention::NeedsInput,
                needs_input_count: 1,
                total_needs_input: 2,
            },
        ),
        ("ui_event_project_updated", UiEvent::ProjectUpdated { project: project_info() }),
        ("ui_event_project_removed", UiEvent::ProjectRemoved { id: ProjectId::new("old") }),
        (
            "ui_event_layout_changed",
            UiEvent::LayoutChanged { project_id: ProjectId::new("shop"), layout: layout() },
        ),
        ("ui_event_tickets_changed", UiEvent::TicketsChanged { scope: Scope::All }),
        (
            "ui_event_reviews_changed",
            UiEvent::ReviewsChanged {
                scope: Scope::Project { id: ProjectId::new("shop") },
                new_keys: vec![review_ref()],
            },
        ),
        ("ui_event_work_updated", UiEvent::WorkUpdated { work: Box::new(work_item()) }),
        (
            "ui_event_settings_changed",
            UiEvent::SettingsChanged {
                layers: vec![Layer::Global],
                paths: vec!["terminal.font_size".into(), "window.decorations".into()],
                requires_restart: vec!["window.decorations".into()],
            },
        ),
        (
            "ui_event_account_status",
            UiEvent::AccountStatusChanged {
                account_id: AccountId::new("jira-acme"),
                status: AccountStatus::NeedsAuth,
                detail: Some("401 Unauthorized".into()),
            },
        ),
        (
            "ui_event_toast",
            UiEvent::Toast {
                toast: Toast {
                    level: ToastLevel::Error,
                    text: "Transition failed".into(),
                    action: Some(ToastAction {
                        label: "Open in browser".into(),
                        command: "open_external".into(),
                        args: Some(json!({"url": "https://acme.atlassian.net/browse/SHOP-142"})),
                    }),
                },
            },
        ),
        (
            "ui_event_plugin_event",
            UiEvent::PluginEvent {
                instance_id: ScreenInstanceId::new("scr-1"),
                name: "ticket.transitioned".into(),
                payload: json!({"ticket": val(&ticket_ref())}),
            },
        ),
        ("ui_event_ctl_command", UiEvent::CtlCommand { cmd: CtlCommand::Palette }),
        (
            "ui_event_ui_open",
            UiEvent::UiOpen {
                request: OpenPaneRequest {
                    content: PaneContent::TicketDetail { ticket: ticket_ref() },
                    placement: Placement::SplitRight,
                    focus: true,
                    tab_title: None,
                    work_item_id: None,
                },
                project_id: ProjectId::new("shop"),
            },
        ),
    ]
}

/// Every fixture.
pub fn all() -> Vec<Fixture> {
    let mut v = vec![
        fx!("kelta_error", KeltaError, KeltaError::not_implemented("session_spawn")),
        fx!(
            "kelta_error_needs_fields",
            KeltaError,
            KeltaError::new(ErrorCode::NeedsFields, "Resolution is required").with_detail(
                json!({"fields": [{"id": "resolution", "name": "Resolution", "required": true}]})
            )
        ),
        fx!(
            "app_info",
            AppInfo,
            AppInfo {
                version: "0.1.0".into(),
                platform: "macos".into(),
                arch: "aarch64".into(),
                data_dir: PathBuf::from("/Users/ada/Library/Application Support/dev.kelta.Kelta"),
                config_dir: PathBuf::from("/Users/ada/.config/kelta"),
                runtime_dir: PathBuf::from("/tmp/kelta-501"),
                claude: Some(ToolVersion {
                    path: PathBuf::from("/opt/homebrew/bin/claude"),
                    version: "2.1.210".into(),
                    ok: true,
                }),
                safe_graphics: false,
                decorations: crate::settings::Decorations::Native,
            }
        ),
        fx!(
            "perf_snapshot",
            PerfSnapshot,
            PerfSnapshot {
                processes: vec![
                    ProcMem {
                        pid: 100,
                        name: "kelta".into(),
                        role: ProcRole::Core,
                        pss_or_footprint_kb: 48_000
                    },
                    ProcMem {
                        pid: 101,
                        name: "com.apple.WebKit.WebContent".into(),
                        role: ProcRole::WebContent,
                        pss_or_footprint_kb: 120_000,
                    },
                ],
                sessions: vec![SessionMem {
                    id: SessionId::new(SID),
                    name: "SHOP-142 claude".into(),
                    history_lines: 500,
                    model_bytes: 1_440_000,
                    child_kb: Some(210_000),
                }],
                live_views: 3,
                timers_armed: 0,
                http_server: false,
            }
        ),
        fx!(
            "diagnostics",
            Diagnostics,
            Diagnostics {
                checks: vec![
                    Check {
                        id: "claude".into(),
                        label: "Claude Code".into(),
                        status: CheckStatus::Ok,
                        detail: "2.1.210 at /opt/homebrew/bin/claude".into(),
                        fix: None,
                    },
                    Check {
                        id: "secret-service".into(),
                        label: "Secret Service".into(),
                        status: CheckStatus::Warn,
                        detail: "org.freedesktop.secrets not available".into(),
                        fix: Some("gnome-keyring-daemon --start --components=secrets".into()),
                    },
                ],
            }
        ),
        fx!("project_info", ProjectInfo, project_info()),
        fx!(
            "project_draft",
            ProjectDraft,
            ProjectDraft {
                suggested_id: ProjectId::new("shop"),
                name: "Shop".into(),
                color: None,
                icon: None,
                repos: vec![RepoDraft {
                    id: "api".into(),
                    path: PathBuf::from("/home/ada/code/shop-api"),
                    primary: true,
                    remote: "origin".into(),
                    base: "main".into(),
                    remote_url: Some("git@github.com:acme/shop-api.git".into()),
                    code_host: None,
                }],
                code_host_hints: vec![CodeHostHint {
                    repo_id: "api".into(),
                    kind: "github".into(),
                    host: "github.com".into(),
                    repo: "acme/shop-api".into(),
                    account: Some(AccountId::new("github-work")),
                }],
                tracker_hints: vec![TrackerHint {
                    kind: "jira".into(),
                    reason: "branch names contain SHOP-123".into(),
                    key: Some("SHOP".into()),
                    account: Some(AccountId::new("jira-acme")),
                }],
                tracker: None,
                default_template: None,
            }
        ),
        fx!(
            "project_patch",
            ProjectPatch,
            ProjectPatch {
                name: Some("Shop!".into()),
                color: Some("#81b29a".into()),
                ..ProjectPatch::default()
            }
        ),
        fx!("layout", Layout, layout()),
        fx!(
            "spawn_request",
            SpawnRequest,
            SpawnRequest {
                id: None,
                project_id: ProjectId::new("shop"),
                kind: SessionKind::Editor { adapter: "nvim".into() },
                name: Some("nvim".into()),
                program: Some("nvim".into()),
                args: vec!["--listen".into(), "/tmp/kelta-1000/s/01928f6e/nvim.sock".into(), ".".into()],
                cwd: Some(PathBuf::from("/home/ada/code/shop-api")),
                env: BTreeMap::from([("FOO".to_owned(), "bar".to_owned())]),
                cols: 120,
                rows: 40,
                work_item_id: None,
                restore: RestorePolicy::Editor { session_file: None },
                close_on_exit: CloseOnExit::Never,
                template_id: Some("claude+editor".into()),
            }
        ),
        fx!("session_info", SessionInfo, session_info()),
        fx!(
            "session_info_shell_dormant",
            SessionInfo,
            SessionInfo {
                id: SessionId::new(SID2),
                kind: SessionKind::Tool { tool_id: ToolId::new("lazydocker") },
                name: "Docker".into(),
                title: None,
                status: SessionStatus::Unknown,
                status_source: StatusSource::None,
                attention: Attention::None,
                seen: true,
                lifecycle: Lifecycle::Dormant,
                pid: None,
                claude: None,
                editor: None,
                work_item_id: None,
                ..session_info()
            }
        ),
        fx!("attach_info", AttachInfo, AttachInfo { generation: 3, cols: 120, rows: 40 }),
        fx!(
            "status_change",
            StatusChange,
            StatusChange {
                status: SessionStatus::Done,
                preview: Some("All tests pass.".into()),
                file_edited: None,
                raw_event: "Stop".into(),
                session_uuid: None,
            }
        ),
        fx!(
            "template_ctx",
            TemplateCtx,
            TemplateCtx {
                repo_id: Some("api".into()),
                session_id: Some(SessionId::new(SID)),
                ticket: Some(ticket_ref()),
                ..TemplateCtx::default()
            }
        ),
        fx!("terminal_palette", TerminalPalette, TerminalPalette::default()),
        fx!(
            "ticket_page",
            TicketPage,
            TicketPage {
                items: vec![TicketItem {
                    ticket: ticket(),
                    project_ids: vec![ProjectId::new("shop")],
                    work_item_id: Some(WorkItemId::new(WID)),
                    view_ids: vec!["mine".into()],
                    prs: vec![PrLink {
                        url: "https://github.com/acme/shop-api/pull/90".into(),
                        account: Some(AccountId::new("github-acme")),
                        repo: "acme/shop-api".into(),
                        number: 90,
                        title: "SHOP-142: Rate-limit login".into(),
                        branch: "feat/SHOP-142-rate-limit-login".into(),
                        state: PrState::Open,
                        draft: false,
                        ci: CiState::Pending,
                        review: Some(ReviewDecision::ReviewRequired),
                        source: PrSource::WorkItem,
                    }],
                    caps: TrackerCaps {
                        board_columns: true,
                        assign: true,
                        comment: true,
                        transitions_need_fetch: true,
                        projects_v2: false,
                    },
                }],
                next: Some(Cursor::Token("eyJuZXh0IjoyfQ".into())),
                stale: false,
                errors: vec![AccountError {
                    account_id: AccountId::new("redmine-client"),
                    error: KeltaError::network("offline"),
                }],
            }
        ),
        fx!("ticket", Ticket, ticket()),
        fx!(
            "source_hit",
            SourceHit,
            SourceHit {
                kind: "board".into(),
                label: "SHOP board".into(),
                detail: Some("Scrum".into()),
                view: TrackerView {
                    id: "board-12".into(),
                    label: "SHOP board".into(),
                    jql: Some("project = SHOP".into()),
                    board_id: Some(12),
                    who: Some(Who::Mine),
                    current_iteration: true,
                    account: Some(AccountId::new("jira-acme")),
                    ..TrackerView::default()
                },
            }
        ),
        fx!(
            "ticket_detail",
            TicketDetail,
            TicketDetail {
                ticket: ticket(),
                body_md: "Limit login attempts to **5/min**.".into(),
                body_html: "<p>Limit login attempts to <strong>5/min</strong>.</p>".into(),
                body_format: BodyFormat::Adf,
                comments: vec![Comment {
                    author: user(),
                    created_at: TS.into(),
                    body_html: "<p>On it.</p>".into(),
                }],
                parent: None,
                prs: Vec::new(),
                caps: Default::default(),
            }
        ),
        fx!(
            "columns",
            Vec<Column>,
            vec![
                Column {
                    id: "todo".into(),
                    name: "To do".into(),
                    category: StatusCategory::Todo,
                    order: 0,
                    match_names: vec![]
                },
                Column {
                    id: "doing".into(),
                    name: "In progress".into(),
                    category: StatusCategory::InProgress,
                    order: 1,
                    match_names: vec![],
                },
            ]
        ),
        fx!(
            "transitions",
            Vec<Transition>,
            vec![Transition {
                id: "31".into(),
                name: "Start review".into(),
                to: status("10", "In Review", StatusCategory::InReview),
                needs_fields: false,
            }]
        ),
        fx!(
            "review_page",
            ReviewPage,
            ReviewPage {
                items: vec![ReviewItem { review: review(), project_ids: vec![ProjectId::new("shop")] }],
                stale: true,
                errors: vec![],
            }
        ),
        fx!(
            "review_detail",
            ReviewDetail,
            ReviewDetail {
                review: review(),
                state: PrState::Open,
                body_html: "<p>Caches prices for 60 s.</p>".into(),
                reviewers: vec![Reviewer { user: user(), state: Some(MyReviewState::Pending) }],
                checks: vec![CiCheck {
                    name: "ci / test".into(),
                    state: CiState::Success,
                    url: Some("https://github.com/acme/shop-api/actions/runs/1".into()),
                }],
                files: vec![FileChange { path: "src/prices.rs".into(), additions: 120, deletions: 14 }],
                pending_comments: 0,
            }
        ),
        fx!("start_work_plan", StartWorkPlan, start_work_plan()),
        fx!("work_item", WorkItem, work_item()),
        fx!(
            "work_item_failed",
            WorkItem,
            WorkItem {
                state: WorkState::Failed { step: "worktree".into(), message: "branch exists".into() },
                ..work_item()
            }
        ),
        fx!(
            "work_item_merged",
            WorkItem,
            WorkItem {
                pr_url: Some("https://github.com/acme/shop-api/pull/90".into()),
                state: WorkState::Merged { detail: Some("choose Done status".into()) },
                ..work_item()
            }
        ),
        fx!(
            "finish_merged_report",
            FinishMergedReport,
            FinishMergedReport {
                finished: vec![],
                skipped: vec![SkippedItem { id: WorkItemId::new(WID), reason: "choose Done status".into() }],
            }
        ),
        fx!(
            "finish_opts",
            FinishOpts,
            FinishOpts {
                remove_worktree: true,
                delete_branch: true,
                force: false,
                transition_to: Some(TransitionTarget::Name { name: "Done".into() }),
            }
        ),
        fx!(
            "git_status",
            GitStatus,
            GitStatus {
                ahead: 2,
                behind: 0,
                dirty: true,
                unpushed: true,
                diverged: false,
                remote_new: 0,
                files: 3,
                insertions: 41,
                deletions: 7,
                missing: false
            }
        ),
        fx!(
            "work_item_rebase_stopped",
            WorkItem,
            WorkItem {
                pr_url: Some("https://github.com/acme/shop-api/pull/74".into()),
                state: WorkState::PrOpen,
                sent_threads: vec!["PRRT_kwDOA1".into()],
                rebase: Some(Box::new(RebaseState {
                    onto: "origin/main".into(),
                    pre_head: "9c1d8e7b6a5f4e3d2c1b0a998877665544aa3f2a".into(),
                    remote_sha: Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into()),
                    conflicts: vec![PathBuf::from("src/login.rs")],
                    step: 2,
                    total: 3,
                })),
                ..work_item()
            }
        ),
        fx!("rebase_op", RebaseOp, RebaseOp::Start { onto: RebaseOnto::Base, no_fetch: false }),
        fx!("feedback", Feedback, feedback()),
        fx!(
            "pr_draft",
            PrDraft,
            PrDraft { title: Some("SHOP-142: Rate-limit login".into()), body: None, draft: Some(true) }
        ),
        fx!("editor_target", EditorTarget, EditorTarget::Session { id: SessionId::new(SID2) }),
        fx!(
            "tool_infos",
            Vec<ToolInfo>,
            vec![
                ToolInfo {
                    id: ToolId::new("lazydocker"),
                    label: "Docker".into(),
                    icon: Some("container".into()),
                    kind: ToolKind::Pty,
                    installed: Some(true),
                    source: ToolSource::Layer { layer: Layer::Global },
                    keybinding: None,
                    description: None,
                },
                ToolInfo {
                    id: ToolId::new("tools-pack/k9s"),
                    label: "Kubernetes".into(),
                    icon: None,
                    kind: ToolKind::Pty,
                    installed: None,
                    source: ToolSource::Plugin { plugin_id: PluginId::new("tools-pack") },
                    keybinding: None,
                    description: None,
                },
            ]
        ),
        fx!(
            "tool_check",
            ToolCheck,
            ToolCheck {
                installed: false,
                version: None,
                install_hint: Some("brew install lazydocker".into())
            }
        ),
        fx!("tool_handle_pty", ToolHandle, ToolHandle::Pty { session_id: SessionId::new(SID2) }),
        fx!(
            "tool_handle_web",
            ToolHandle,
            ToolHandle::Web {
                instance_id: ToolInstanceId::new("0f1e2d3c4b5a69788796a5b4c3d2e1f0"),
                url: "http://127.0.0.1:3011/?token=abc".into(),
                embed: EmbedMode::Iframe,
            }
        ),
        fx!(
            "plugin_info",
            PluginInfo,
            PluginInfo {
                id: PluginId::new("sprint-burndown"),
                name: "Sprint Burndown".into(),
                version: "0.2.0".into(),
                description: "Burndown chart".into(),
                enabled: true,
                permissions: plugin_manifest().permissions,
                granted: vec!["tickets.read".into(), "settings.read".into()],
                problems: vec![],
                dir: PathBuf::from("/home/ada/.local/share/kelta/plugins/sprint-burndown"),
                dev: false,
                contributes: plugin_manifest().contributes,
            }
        ),
        fx!(
            "plugin_install_preview",
            PluginInstallPreview,
            PluginInstallPreview {
                manifest: plugin_manifest(),
                permissions: plugin_manifest()
                    .permissions
                    .iter()
                    .filter_map(|p| Permission::parse(p))
                    .map(|p| PermissionInfo { permission: p.to_string(), description: p.describe() })
                    .collect(),
                sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(),
                warnings: vec![],
            }
        ),
        fx!(
            "screen_open_result",
            ScreenOpenResult,
            ScreenOpenResult {
                instance_id: ScreenInstanceId::new("scr-1"),
                url: "kelta-plugin://sprint-burndown/dist/index.html".into(),
            }
        ),
        fx!(
            "trigger_infos",
            Vec<TriggerInfo>,
            vec![TriggerInfo {
                id: "start-db".into(),
                origin: TriggerOrigin::Project { project_id: ProjectId::new("shop") },
                on: "ticket.started".into(),
                enabled: true,
                description: None,
            }]
        ),
        fx!(
            "trigger_runs",
            Vec<TriggerRun>,
            vec![TriggerRun {
                ts: TS.into(),
                trigger_id: "start-db".into(),
                event: "ticket.started".into(),
                ok: false,
                detail: Some("docker: exit 1".into()),
                depth: 0,
            }]
        ),
        fx!(
            "blocking_outcome_veto",
            BlockingOutcome,
            BlockingOutcome::Veto {
                trigger_id: "guard-dirty-main".into(),
                reason: "main checkout is dirty".into()
            }
        ),
        fx!(
            "effective_settings",
            EffectiveSettings,
            EffectiveSettings {
                value: json!({"terminal": {"font_size": 14}}),
                sources: BTreeMap::from([("terminal.font_size".to_owned(), Layer::Project)]),
            }
        ),
        fx!(
            "layer_doc",
            LayerDoc,
            LayerDoc {
                path: PathBuf::from("/home/ada/code/shop-api/.kelta/config.toml"),
                value: json!({"worktree": {"setup": ["pnpm install"]}}),
                text: "[worktree]\nsetup = [\"pnpm install\"]\n".into(),
                trusted: Some(false),
            }
        ),
        fx!(
            "validation_issues",
            Vec<ValidationIssue>,
            vec![ValidationIssue {
                path: "terminal.font_size".into(),
                message: "must be ≤ 40".into(),
                line: Some(12),
                col: Some(13),
            }]
        ),
        fx!(
            "trust_info",
            TrustInfo,
            TrustInfo {
                path: PathBuf::from("/home/ada/code/shop-api/.kelta/config.toml"),
                hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                trusted: true,
            }
        ),
        fx!(
            "secret_backends_status",
            Vec<SecretBackendStatus>,
            vec![SecretBackendStatus { backend: "keychain".into(), available: true, detail: None }]
        ),
        fx!(
            "account_test_result",
            AccountTestResult,
            AccountTestResult { ok: true, user: Some(user()), error: None }
        ),
        fx!("settings_default", Settings, Settings::defaults()),
        fx!("plugin_manifest", PluginManifest, plugin_manifest()),
        fx!("tool_def", ToolDef, isl_tool()),
        fx!("trigger_def", TriggerDef, trigger()),
        fx!(
            "open_pane_request",
            OpenPaneRequest,
            OpenPaneRequest {
                content: PaneContent::PluginScreen {
                    plugin_id: PluginId::new("sprint-burndown"),
                    screen_id: "burndown".into(),
                    instance_id: ScreenInstanceId::new("scr-1"),
                    params: json!({}),
                },
                placement: Placement::NewTab,
                focus: true,
                tab_title: Some("Burndown".into()),
                work_item_id: None,
            }
        ),
        fx!("subscribe_result", SubscribeResult, SubscribeResult { sub_id: 1 }),
        fx!("layout_save_result", LayoutSaveResult, LayoutSaveResult { rev: 8 }),
        fx!(
            "bus_event",
            BusEvent,
            BusEvent {
                name: bus::SESSION_STATUS_CHANGED.into(),
                ts: TS.into(),
                project_id: Some(ProjectId::new("shop")),
                session_id: Some(SessionId::new(SID)),
                work_item_id: Some(WorkItemId::new(WID)),
                payload: json!({"status": "needs_input", "previous": "working", "source": "hook"}),
                chain: TriggerChain { depth: 1, origin_triggers: vec!["claude-needs-me".into()] },
            }
        ),
        fx!(
            "ctl_request_hook",
            CtlRequest,
            CtlRequest::new(CtlCommand::Hook {
                session: SessionId::new(SID),
                token: "0123456789abcdef0123456789abcdef".into(),
                payload: Box::new(hook(
                    "Stop",
                    json!({"stop_hook_active": false, "last_assistant_message": "Done."})
                )),
            })
        ),
        fx!(
            "ctl_request_start",
            CtlRequest,
            CtlRequest::new(CtlCommand::Start {
                ticket: "SHOP-142".into(),
                project: Some(ProjectId::new("shop"))
            })
        ),
        fx!("ctl_response", CtlResponse, CtlResponse::ok(json!({"focused": true}))),
        fx!(
            "hook_session_start",
            HookPayload,
            hook("SessionStart", json!({"source": "startup", "model": "claude-opus"}))
        ),
        fx!(
            "hook_user_prompt_submit",
            HookPayload,
            hook("UserPromptSubmit", json!({"prompt": "Fix the login rate limit"}))
        ),
        fx!(
            "hook_permission_request",
            HookPayload,
            hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {"command": "cargo test"}}))
        ),
        fx!(
            "hook_notification_permission",
            HookPayload,
            hook(
                "Notification",
                json!({"notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"})
            )
        ),
        fx!(
            "hook_notification_idle",
            HookPayload,
            hook(
                "Notification",
                json!({"notification_type": "idle_prompt", "message": "Claude is waiting for your input"})
            )
        ),
        fx!(
            "hook_post_tool_use_edit",
            HookPayload,
            hook(
                "PostToolUse",
                json!({"tool_name": "Edit", "tool_input": {"file_path": "/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login/src/login.rs", "old_string": "a", "new_string": "b"}, "tool_response": {"success": true}})
            )
        ),
        fx!(
            "hook_stop",
            HookPayload,
            hook(
                "Stop",
                json!({"stop_hook_active": false, "last_assistant_message": "Done. All tests pass."})
            )
        ),
        fx!("hook_stop_failure", HookPayload, hook("StopFailure", json!({"error": "rate_limit"}))),
        fx!("hook_session_end", HookPayload, hook("SessionEnd", json!({"reason": "prompt_input_exit"}))),
        fx!("hook_statusline", HookPayload, statusline(true)),
        fx!("hook_statusline_api_key", HookPayload, statusline(false)),
        fx!(
            "notification",
            Notification,
            Notification {
                title: "Shop: SHOP-142 claude".into(),
                body: Some("I need permission to run `cargo test`.".into()),
                urgency: Urgency::Critical,
                project_id: Some(ProjectId::new("shop")),
                session_id: Some(SessionId::new(SID)),
            }
        ),
        fx!(
            "terminal_stats",
            TerminalStats,
            TerminalStats {
                sessions: vec![SessionTermStats {
                    id: SessionId::new(SID),
                    bytes_in: 1_048_576,
                    history_lines: 500,
                    cols: 120,
                    rows: 40,
                    inflight: 0,
                    attached: true,
                    memory_bytes: 1_440_000,
                }],
                total_memory_bytes: 1_440_000,
                reader_threads: 1,
            }
        ),
        fx!(
            "terminal_limits",
            TerminalLimits,
            TerminalLimits {
                scrollback: ScrollbackSettings::default(),
                memory_cap_mb: 160,
                view_scrollback: 1000,
                keyboard_protocol: KeyboardProtocol::Kitty,
                history_log: true,
                history_log_mb: 16,
                history_log_total_mb: 512
            }
        ),
        fx!(
            "history_hit",
            HistoryHit,
            HistoryHit { session_id: SessionId::new(SID), line: "error[E0308]: mismatched types".into() }
        ),
        fx!(
            "login_env",
            LoginEnv,
            LoginEnv {
                vars: BTreeMap::from([("PATH".to_owned(), "/opt/homebrew/bin:/usr/bin:/bin".to_owned())]),
                source: LoginEnvSource::LoginInteractive,
                shell: Some(PathBuf::from("/bin/zsh")),
            }
        ),
        fx!(
            "proxied_request",
            ProxiedRequest,
            ProxiedRequest {
                url: "https://api.acme.com/v1/burndown".into(),
                method: "GET".into(),
                headers: BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]),
                body: None,
                body_base64: false,
                timeout_ms: Some(10_000),
            }
        ),
        fx!(
            "proxied_response",
            ProxiedResponse,
            ProxiedResponse {
                status: 200,
                headers: BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]),
                body: "{\"points\":[]}".into(),
                body_base64: false,
            }
        ),
        fx!(
            "settings_diff",
            SettingsDiff,
            SettingsDiff {
                layers: vec![Layer::Project],
                paths: vec!["worktree.setup".into()],
                requires_restart: vec![],
            }
        ),
        fx!(
            "plugin_grant",
            PluginGrant,
            PluginGrant {
                permission: "tickets.read".into(),
                granted_at: TS.into(),
                manifest_sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(),
            }
        ),
        fx!(
            "account_config",
            AccountConfig,
            AccountConfig {
                kind: AccountKind::Jira,
                base_url: Some("https://acme.atlassian.net".into()),
                flavor: JiraFlavor::Auto,
                auth: None,
                email: Some("me@acme.com".into()),
                user: None,
                secret: Some(crate::secret::SecretRef::new("keyring:jira-acme")),
                text_format: TextFormat::Textile,
                poll_secs: None,
                web_url: None,
                plugin: None,
            }
        ),
        fx!("session_templates_default", Vec<SessionTemplate>, default_session_templates()),
        fx!("editor_presets_default", Vec<EditorPreset>, default_editor_presets()),
        fx!(
            "review_query",
            ReviewQuery,
            ReviewQuery { kind: ReviewKind::Authored, include_team: true, include_drafts: false }
        ),
        fx!("work_source_review", WorkSource, WorkSource::Review { review: review_ref() }),
        fx!(
            "pane_ref",
            PaneRef,
            PaneRef {
                project_id: ProjectId::new("shop"),
                tab_id: TabId::new("tab-1"),
                pane_id: PaneId::new("pane-2")
            }
        ),
        fx!(
            "pr_create",
            PrCreate,
            PrCreate {
                repo: "acme/shop-api".into(),
                head: "feat/SHOP-142-rate-limit-login".into(),
                base: "main".into(),
                title: "SHOP-142: Rate-limit login".into(),
                body: "https://acme.atlassian.net/browse/SHOP-142".into(),
                draft: false,
            }
        ),
        fx!(
            "ctl_command_new",
            CtlCommand,
            CtlCommand::New {
                template: "claude+editor".into(),
                cwd: Some(PathBuf::from("/home/ada/code/shop-api")),
                project: Some(ProjectId::new("shop")),
            }
        ),
        fx!(
            "ctl_response_error",
            CtlResponse,
            CtlResponse::err(KeltaError::invalid("only custom.* events can be emitted"))
        ),
    ];
    for (name, ev) in ui_event_samples() {
        v.push(Fixture { name, type_name: "UiEvent", value: val(&ev), roundtrip: rt::<UiEvent> });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_unique() {
        let mut names: Vec<_> = all().iter().map(|f| f.name).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn samples_round_trip() {
        for f in all() {
            assert!(!f.value.is_null(), "{} serialized to null", f.name);
            let back = (f.roundtrip)(&f.value).unwrap_or_else(|e| panic!("{}: {e}", f.name));
            assert_eq!(back, f.value, "{} does not round-trip", f.name);
        }
    }
}
