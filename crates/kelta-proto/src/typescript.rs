//! TypeScript export (ts-rs) used by `xtask codegen`. Generated in memory so `codegen --check`
//! can compare without touching the tree. Large integers (`u64`/`i64`) map to `number`.

use std::collections::BTreeMap;

use ts_rs::{Config, TS, TypeVisitor};

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
use crate::secret::*;
use crate::settings::*;
use crate::term::*;
use crate::tracker::*;

pub fn config() -> Config {
    Config::new().with_large_int("number")
}

struct Collector<'a> {
    cfg: &'a Config,
    out: BTreeMap<String, String>,
    error: Option<String>,
}

impl TypeVisitor for Collector<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if self.error.is_some() {
            return;
        }
        let Some(path) = T::output_path() else { return };
        let key = path.to_string_lossy().replace('\\', "/");
        if self.out.contains_key(&key) {
            return;
        }
        match T::export_to_string(self.cfg) {
            Ok(s) => {
                self.out.insert(key, s);
            }
            Err(e) => {
                self.error = Some(format!("{key}: {e}"));
                return;
            }
        }
        T::visit_dependencies(self);
    }
}

macro_rules! roots {
    ($v:expr; $($t:ty),* $(,)?) => {
        $( $v.visit::<$t>(); )*
    };
}

/// Every exported type as `relative path → file contents`.
pub fn export_all() -> Result<BTreeMap<String, String>, String> {
    let cfg = config();
    let mut v = Collector { cfg: &cfg, out: BTreeMap::new(), error: None };
    roots!(v;
        // errors / ids
        KeltaError, ErrorCode, ProjectId, SessionId, WorkItemId, AccountId, TabId, PaneId,
        ToolInstanceId, ScreenInstanceId, PluginId, ToolId,
        // model
        ProjectInfo, RepoInfo, AttentionSummary, ProjectDraft, RepoDraft, CodeHostHint, TrackerHint,
        ProjectPatch, SessionKind, SessionStatus, Attention, Lifecycle, StatusSource, RestorePolicy,
        CloseOnExit, SpawnRequest, SessionInfo, ClaudeMeta, EditorMeta, AttachInfo, StatusChange,
        Scope, Layout, Tab, SplitDir, LayoutNode, TicketsMode, PaneContent, Placement,
        OpenPaneRequest, PaneRef, WorkKind, WorkState, StepStatus, WorkStepStatus, WorkItem,
        WorkSource, BranchChoice, BranchExists, ClaudePlan, SideEffects, StartWorkPlan, FinishOpts,
        FinishMergedReport, SkippedItem, ShipOrigin, GitStatus, EditorTarget, TemplateCtx,
        // tracker / codehost
        TrackerKind, TrackerCaps, User, TicketRef, StatusCategory, Status, Ticket, BodyFormat,
        Comment, TicketDetail, Transition, Column, Cursor, Page<Ticket>, Assignee, AccountError,
        TicketItem, TicketPage, CodeHostKind, ReviewRef, CiState, ReviewDecision, MyReviewState,
        ReviewKind, Review, ReviewQuery, Reviewer, CiCheck, FileChange, ReviewDetail, PrState, PrCreate,
        PrDraft, ReviewItem, ReviewPage,
        // ext
        ToolKind, EmbedMode, WebLifecycle, Ready, StopSpec, WebStart, ToolDef, ToolSource,
        ToolInfo, ToolCheck, ToolHandle, Matcher, TriggerDef, Urgency, RunStdin, RunShow,
        ActionDef, CommandWhen, CommandDef, TriggerOrigin, TriggerInfo, TriggerRun,
        BlockingOutcome, PlatformName, ScreenScope, ScreenPlacement, ScreenDef, KeybindingDef,
        ActionButtonDef, SettingsContribution, Contributes, PluginManifest, Permission,
        PluginMethod, PluginInfo, PermissionInfo, PluginInstallPreview, ScreenOpenResult,
        CallOrigin, ProxiedRequest, ProxiedResponse, PluginGrant,
        // settings
        Layer, Settings, ProjectConfig, RepoConfig, CodeHostBinding, TrackerBinding, TrackerView,
        ColumnSpec, StatusMap, RepoRule, RepoMatch, ProjectV2Ref, TransitionTarget, AccountConfig,
        SessionTemplate, TemplateNode, EditorPreset, ClaudeProfile, LinuxGraphics,
        EffectiveSettings, LayerDoc, ValidationIssue, TrustInfo, SettingsDiff,
        // secrets
        SecretRef, SecretBackendStatus,
        // events
        ToastLevel, ToastAction, Toast, Notification, AccountStatus, UiEvent, TriggerChain,
        BusEvent,
        // ctl / hooks
        CtlCommand, CtlRequest, CtlResponse, HookPayload,
        // terminal
        KillSignal, ClipboardKind, TerminalPalette, TerminalLimits, SessionTermStats,
        TerminalStats, LoginEnvSource, LoginEnv,
        // ipc
        ToolVersion, AppInfo, ProcRole, ProcMem, SessionMem, PerfSnapshot, CheckStatus, Check,
        Diagnostics, WindowState, SubscribeResult, LayoutSaveResult, AccountTestResult,
    );
    if let Some(e) = v.error {
        return Err(e);
    }
    Ok(v.out)
}
