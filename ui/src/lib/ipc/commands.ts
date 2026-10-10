// Typed wrappers for every Tauri command of ARCHITECTURE §6 (scaffold-owned, frozen).
//
// Conventions:
// - Every command takes ONE argument object whose keys are the snake_case names of the Rust
//   command parameters (`#[tauri::command(rename_all = "snake_case")]`). Optional Rust `Option<T>`
//   parameters may be omitted or `null`.
// - Every wrapper rejects with an `IpcError` (carrying the Rust `KeltaError`).
// - `COMMAND_NAMES` must match `apps/desktop/src-tauri/src/commands/names.rs` (checked by a test).

import type {
  AccountId,
  AccountTestResult,
  AppInfo,
  Assignee,
  AttachInfo,
  ClipboardKind,
  Column,
  Cursor,
  Diagnostics,
  EditorTarget,
  EffectiveSettings,
  FinishOpts,
  GitStatus,
  HistoryHit,
  JsonValue,
  Layer,
  LayerDoc,
  Layout,
  LayoutSaveResult,
  PerfSnapshot,
  Placement,
  PluginId,
  PluginInfo,
  PluginInstallPreview,
  PluginMethod,
  PrDraft,
  ProjectDraft,
  ProjectId,
  ProjectInfo,
  ProjectPatch,
  ReviewDetail,
  ReviewKind,
  ReviewPage,
  ReviewRef,
  Scope,
  ScreenInstanceId,
  ScreenOpenResult,
  SecretBackendStatus,
  SessionId,
  SessionInfo,
  SpawnRequest,
  StartWorkPlan,
  SubscribeResult,
  TemplateCtx,
  TerminalPalette,
  Ticket,
  TicketDetail,
  TicketItem,
  TicketPage,
  TicketRef,
  ToolCheck,
  ToolHandle,
  ToolId,
  ToolInfo,
  ToolInstanceId,
  Transition,
  TriggerInfo,
  TriggerRun,
  TrustInfo,
  UiEvent,
  ValidationIssue,
  WorkItem,
  WorkItemId,
  WorkSource,
} from '$lib/gen';

import { getTransport, toIpcError, type IpcChannel } from './transport';

type NoArgs = Record<string, never>;

/** Layer selector shared by the settings commands. */
export interface LayerArgs {
  layer: Layer;
  project_id?: ProjectId | null;
  repo_id?: string | null;
}

/**
 * Command catalogue: name → argument object and result. `channel` arguments are passed as
 * `IpcChannel` handles (see `eventsSubscribe` / `sessionAttach`).
 */
export interface Commands {
  // ---- app ---------------------------------------------------------------------------------
  app_info: { args: NoArgs; result: AppInfo };
  app_ready: { args: { t_ms: number }; result: null };
  events_subscribe: { args: { channel: IpcChannel<UiEvent> }; result: SubscribeResult };
  open_external: { args: { url: string }; result: null };
  perf_snapshot: { args: NoArgs; result: PerfSnapshot };
  diagnostics_run: { args: NoArgs; result: Diagnostics };
  clipboard_read: { args: { kind: ClipboardKind }; result: string };
  clipboard_write: { args: { kind: ClipboardKind; text: string }; result: null };
  notify_test: { args: NoArgs; result: null };
  // ---- settings ----------------------------------------------------------------------------
  settings_schema: { args: NoArgs; result: JsonValue };
  settings_effective: { args: { project_id?: ProjectId | null }; result: EffectiveSettings };
  settings_layer_get: { args: LayerArgs; result: LayerDoc };
  settings_set: { args: LayerArgs & { path: string; value: JsonValue }; result: EffectiveSettings };
  settings_reset: { args: LayerArgs & { path: string }; result: EffectiveSettings };
  settings_validate: { args: { layer: Layer; text: string }; result: ValidationIssue[] };
  settings_write_raw: { args: LayerArgs & { text: string }; result: EffectiveSettings };
  settings_open_file: { args: LayerArgs; result: SessionInfo };
  repo_trust: { args: { project_id: ProjectId; repo_id: string; trust: boolean }; result: TrustInfo };
  secret_set: { args: { secret_ref: string; value: string }; result: null };
  secret_delete: { args: { secret_ref: string }; result: null };
  secret_backends_status: { args: NoArgs; result: SecretBackendStatus[] };
  secret_unlock: { args: { passphrase: string; create: boolean }; result: null };
  account_test: { args: { account_id: AccountId }; result: AccountTestResult };
  // ---- projects ----------------------------------------------------------------------------
  project_list: { args: NoArgs; result: ProjectInfo[] };
  project_detect: { args: { path: string }; result: ProjectDraft };
  project_create: { args: { draft: ProjectDraft }; result: ProjectInfo };
  project_update: { args: { id: ProjectId; patch: ProjectPatch }; result: ProjectInfo };
  project_remove: { args: { id: ProjectId; kill_sessions: boolean }; result: null };
  project_open: { args: { id: ProjectId }; result: ProjectInfo };
  project_close: { args: { id: ProjectId; kill_sessions: boolean }; result: ProjectInfo };
  project_activate: { args: { id: ProjectId }; result: ProjectInfo };
  project_reorder: { args: { ids: ProjectId[] }; result: null };
  // ---- layout ------------------------------------------------------------------------------
  layout_get: { args: { project_id: ProjectId }; result: Layout };
  layout_save: { args: { layout: Layout }; result: LayoutSaveResult };
  // ---- sessions ----------------------------------------------------------------------------
  session_spawn: { args: { req: SpawnRequest }; result: SessionInfo };
  session_spawn_template: {
    args: { project_id: ProjectId; template_id: string; ctx: TemplateCtx; placement: Placement };
    result: SessionInfo[];
  };
  session_attach: {
    args: {
      id: SessionId;
      cols: number;
      rows: number;
      channel: IpcChannel<ArrayBuffer | Uint8Array | number[]>;
    };
    result: AttachInfo;
  };
  session_detach: { args: { id: SessionId; generation: number }; result: null };
  /** Binary: raw bytes body + `x-kelta-session-id` header (see `sessionWrite`). */
  session_write: { args: { id: SessionId; data: Uint8Array }; result: null };
  session_resize: { args: { id: SessionId; cols: number; rows: number }; result: null };
  session_ack: { args: { id: SessionId; generation: number; bytes: number }; result: null };
  session_kill: { args: { id: SessionId; force: boolean }; result: null };
  session_restart: { args: { id: SessionId }; result: SessionInfo };
  session_rename: { args: { id: SessionId; name: string }; result: SessionInfo };
  session_list: { args: { project_id?: ProjectId | null }; result: SessionInfo[] };
  session_mark_seen: { args: { id: SessionId }; result: null };
  session_link: {
    args: { id: SessionId; work_item_id?: WorkItemId | null; ticket?: TicketRef | null };
    result: SessionInfo;
  };
  session_text_tail: { args: { id: SessionId; max_lines: number }; result: string };
  session_history_search: {
    args: { project_id: ProjectId; session_id?: SessionId | null; query: string; limit: number };
    result: HistoryHit[];
  };
  terminal_set_palette: { args: { palette: TerminalPalette }; result: null };
  // ---- tickets -----------------------------------------------------------------------------
  tracker_list: {
    args: { scope: Scope; view_id?: string | null; cursor?: Cursor | null; refresh: boolean };
    result: TicketPage;
  };
  tracker_get: { args: { ticket: TicketRef }; result: TicketDetail };
  tracker_columns: { args: { project_id: ProjectId }; result: Column[] };
  tracker_transitions: { args: { ticket: TicketRef }; result: Transition[] };
  tracker_transition: {
    args: { ticket: TicketRef; transition_id: string; fields?: JsonValue | null };
    result: Ticket;
  };
  tracker_move: { args: { ticket: TicketRef; column_id: string }; result: Ticket };
  tracker_comment: { args: { ticket: TicketRef; markdown: string }; result: null };
  tracker_assign: { args: { ticket: TicketRef; assignee: Assignee }; result: Ticket };
  tracker_search: { args: { scope: Scope; text: string }; result: TicketItem[] };
  // ---- reviews -----------------------------------------------------------------------------
  review_list: { args: { scope: Scope; kind: ReviewKind; refresh: boolean }; result: ReviewPage };
  review_get: { args: { review: ReviewRef }; result: ReviewDetail };
  review_approve: { args: { review: ReviewRef; head_sha: string }; result: null };
  review_comment: { args: { review: ReviewRef; body: string }; result: null };
  review_request_changes: { args: { review: ReviewRef; body: string }; result: null };
  // ---- work --------------------------------------------------------------------------------
  work_plan: { args: { project_id: ProjectId; source: WorkSource }; result: StartWorkPlan };
  work_start: { args: { plan: StartWorkPlan }; result: WorkItem };
  work_list: { args: { project_id?: ProjectId | null }; result: WorkItem[] };
  work_resume: { args: { id: WorkItemId }; result: WorkItem };
  /** `step`: a saga step id to re-run, or `skip:<step>` to mark it skipped and continue. */
  work_retry_step: { args: { id: WorkItemId; step: string }; result: WorkItem };
  work_create_pr: { args: { id: WorkItemId; draft: PrDraft }; result: WorkItem };
  work_finish: { args: { id: WorkItemId; opts: FinishOpts }; result: WorkItem };
  work_status: { args: { id: WorkItemId }; result: GitStatus };
  work_link: { args: { id: WorkItemId; ticket: TicketRef; apply_side_effects: boolean }; result: WorkItem };
  /** Every unfinished item (one fetch per repo, 5 min floor). */
  work_status_all: { args: NoArgs; result: Record<WorkItemId, GitStatus> };
  /** Spawns the review diff session; the UI places it zoomed in the work tab. */
  work_diff: { args: { id: WorkItemId }; result: SessionInfo };
  work_mark_reviewed: { args: { id: WorkItemId }; result: WorkItem };
  editor_open: { args: { target: EditorTarget; path: string; line?: number | null }; result: null };
  editor_send_selection: { args: { editor_session: SessionId; claude_session: SessionId }; result: null };
  // ---- tools / plugins / triggers ----------------------------------------------------------
  tool_list: { args: { project_id: ProjectId }; result: ToolInfo[] };
  tool_check: { args: { tool_id: ToolId }; result: ToolCheck };
  tool_open: {
    args: { project_id: ProjectId; tool_id: ToolId; ctx: TemplateCtx; placement: Placement };
    result: ToolHandle;
  };
  tool_close: { args: { instance_id: ToolInstanceId }; result: null };
  plugin_list: { args: NoArgs; result: PluginInfo[] };
  plugin_inspect: { args: { source: string }; result: PluginInstallPreview };
  plugin_install: { args: { source: string; sha256: string; grant: string[] }; result: PluginInfo };
  plugin_uninstall: { args: { id: PluginId }; result: null };
  plugin_enable: { args: { id: PluginId; enabled: boolean }; result: null };
  plugin_grant: { args: { id: PluginId; permissions: string[] }; result: PluginInfo };
  plugin_screen_open: {
    args: { plugin_id: PluginId; screen_id: string; project_id?: ProjectId | null; params: JsonValue };
    result: ScreenOpenResult;
  };
  plugin_screen_close: { args: { instance_id: ScreenInstanceId }; result: null };
  plugin_call: {
    args: { instance_id: ScreenInstanceId; method: PluginMethod; params: JsonValue };
    result: JsonValue;
  };
  command_run: { args: { command_id: string; ctx: TemplateCtx }; result: null };
  trigger_list: { args: { project_id?: ProjectId | null }; result: TriggerInfo[] };
  trigger_test: { args: { trigger_id: string; payload: JsonValue }; result: TriggerRun };
  trigger_log: { args: { limit: number }; result: TriggerRun[] };
}

export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Commands[K]['args'];
export type CommandResult<K extends CommandName> = Commands[K]['result'];

/** Every command, in ARCHITECTURE §6 catalogue order (mirrors `names.rs`). */
export const COMMAND_NAMES = [
  'app_info',
  'app_ready',
  'events_subscribe',
  'open_external',
  'perf_snapshot',
  'diagnostics_run',
  'clipboard_read',
  'clipboard_write',
  'notify_test',
  'settings_schema',
  'settings_effective',
  'settings_layer_get',
  'settings_set',
  'settings_reset',
  'settings_validate',
  'settings_write_raw',
  'settings_open_file',
  'repo_trust',
  'secret_set',
  'secret_delete',
  'secret_backends_status',
  'secret_unlock',
  'account_test',
  'project_list',
  'project_detect',
  'project_create',
  'project_update',
  'project_remove',
  'project_open',
  'project_close',
  'project_activate',
  'project_reorder',
  'layout_get',
  'layout_save',
  'session_spawn',
  'session_spawn_template',
  'session_attach',
  'session_detach',
  'session_write',
  'session_resize',
  'session_ack',
  'session_kill',
  'session_restart',
  'session_rename',
  'session_list',
  'session_mark_seen',
  'session_link',
  'session_text_tail',
  'session_history_search',
  'terminal_set_palette',
  'tracker_list',
  'tracker_get',
  'tracker_columns',
  'tracker_transitions',
  'tracker_transition',
  'tracker_move',
  'tracker_comment',
  'tracker_assign',
  'tracker_search',
  'review_list',
  'review_get',
  'review_approve',
  'review_comment',
  'review_request_changes',
  'work_plan',
  'work_start',
  'work_list',
  'work_resume',
  'work_retry_step',
  'work_create_pr',
  'work_finish',
  'work_status',
  'work_link',
  'work_status_all',
  'work_diff',
  'work_mark_reviewed',
  'editor_open',
  'editor_send_selection',
  'tool_list',
  'tool_check',
  'tool_open',
  'tool_close',
  'plugin_list',
  'plugin_inspect',
  'plugin_install',
  'plugin_uninstall',
  'plugin_enable',
  'plugin_grant',
  'plugin_screen_open',
  'plugin_screen_close',
  'plugin_call',
  'command_run',
  'trigger_list',
  'trigger_test',
  'trigger_log',
] as const satisfies readonly CommandName[];

// Compile-time check that COMMAND_NAMES covers every key of `Commands`.
type MissingCommands = Exclude<CommandName, (typeof COMMAND_NAMES)[number]>;
const _allCommandsListed: MissingCommands extends never ? true : MissingCommands = true;
void _allCommandsListed;

/** Converts channel handles into the transport-specific values before invoking. */
function encodeArgs(args: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(args)) {
    if (value === undefined) continue;
    out[key] = isChannel(value) ? value.handle : value;
  }
  return out;
}

function isChannel(value: unknown): value is IpcChannel<unknown> {
  return typeof value === 'object' && value !== null && 'handle' in value && 'onmessage' in value;
}

/** Generic typed invoke. Prefer the named wrappers below. */
export async function call<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  try {
    return await getTransport().invoke<CommandResult<K>>(cmd, encodeArgs(args as Record<string, unknown>));
  } catch (err) {
    throw toIpcError(cmd, err);
  }
}

type Wrapper<K extends CommandName> =
  NoArgs extends CommandArgs<K>
    ? (args?: CommandArgs<K>) => Promise<CommandResult<K>>
    : (args: CommandArgs<K>) => Promise<CommandResult<K>>;

function wrap<K extends CommandName>(cmd: K): Wrapper<K> {
  return ((args?: CommandArgs<K>) => call(cmd, (args ?? {}) as CommandArgs<K>)) as Wrapper<K>;
}

// ---- app ------------------------------------------------------------------------------------
export const appInfo = wrap('app_info');
export const appReady = wrap('app_ready');
export const openExternal = wrap('open_external');
export const perfSnapshot = wrap('perf_snapshot');
export const diagnosticsRun = wrap('diagnostics_run');
export const clipboardRead = wrap('clipboard_read');
export const clipboardWrite = wrap('clipboard_write');
export const notifyTest = wrap('notify_test');

/** Subscribes `onEvent` to the UiEvent channel (one subscription per window). */
export async function eventsSubscribe(onEvent: (event: UiEvent) => void): Promise<SubscribeResult> {
  const channel = getTransport().channel<UiEvent>(onEvent);
  return call('events_subscribe', { channel });
}

// ---- settings -------------------------------------------------------------------------------
export const settingsSchema = wrap('settings_schema');
export const settingsEffective = wrap('settings_effective');
export const settingsLayerGet = wrap('settings_layer_get');
export const settingsSet = wrap('settings_set');
export const settingsReset = wrap('settings_reset');
export const settingsValidate = wrap('settings_validate');
export const settingsWriteRaw = wrap('settings_write_raw');
export const settingsOpenFile = wrap('settings_open_file');
export const repoTrust = wrap('repo_trust');
export const secretSet = wrap('secret_set');
export const secretDelete = wrap('secret_delete');
export const secretBackendsStatus = wrap('secret_backends_status');
export const secretUnlock = wrap('secret_unlock');
export const accountTest = wrap('account_test');

// ---- projects -------------------------------------------------------------------------------
export const projectList = wrap('project_list');
export const projectDetect = wrap('project_detect');
export const projectCreate = wrap('project_create');
export const projectUpdate = wrap('project_update');
export const projectRemove = wrap('project_remove');
export const projectOpen = wrap('project_open');
export const projectClose = wrap('project_close');
export const projectActivate = wrap('project_activate');
export const projectReorder = wrap('project_reorder');

// ---- layout ---------------------------------------------------------------------------------
export const layoutGet = wrap('layout_get');
export const layoutSave = wrap('layout_save');

// ---- sessions -------------------------------------------------------------------------------
export const sessionSpawn = wrap('session_spawn');
export const sessionSpawnTemplate = wrap('session_spawn_template');
export const sessionDetach = wrap('session_detach');
export const sessionResize = wrap('session_resize');
export const sessionAck = wrap('session_ack');
export const sessionKill = wrap('session_kill');
export const sessionRestart = wrap('session_restart');
export const sessionRename = wrap('session_rename');
export const sessionList = wrap('session_list');
export const sessionMarkSeen = wrap('session_mark_seen');
export const sessionLink = wrap('session_link');
export const sessionTextTail = wrap('session_text_tail');
export const sessionHistorySearch = wrap('session_history_search');
export const terminalSetPalette = wrap('terminal_set_palette');

/** Normalizes what a Tauri raw channel delivers into bytes. */
export function frameBytes(message: ArrayBuffer | Uint8Array | number[]): Uint8Array {
  if (Array.isArray(message)) return Uint8Array.from(message);
  if (ArrayBuffer.isView(message))
    return new Uint8Array(message.buffer, message.byteOffset, message.byteLength);
  return new Uint8Array(message);
}

/**
 * Attaches a terminal view. `onFrame` receives each binary frame (first byte = tag, ARCH §6.1;
 * constants in `$lib/gen/constants`). Frames of a stale generation must be ignored by the caller.
 */
export async function sessionAttach(
  args: { id: SessionId; cols: number; rows: number },
  onFrame: (frame: Uint8Array) => void,
): Promise<AttachInfo> {
  const channel = getTransport().channel<ArrayBuffer | Uint8Array | number[]>((m) => onFrame(frameBytes(m)));
  return call('session_attach', { ...args, channel });
}

/**
 * Fire-and-forget terminal input: raw body + `x-kelta-session-id` header (S0 contract note 11).
 * Strings are UTF-8 encoded.
 */
export async function sessionWrite(id: SessionId, data: Uint8Array | string): Promise<void> {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  try {
    await getTransport().invoke<null>('session_write', bytes, { headers: { 'x-kelta-session-id': id } });
  } catch (err) {
    throw toIpcError('session_write', err);
  }
}

// ---- tickets --------------------------------------------------------------------------------
export const trackerList = wrap('tracker_list');
export const trackerGet = wrap('tracker_get');
export const trackerColumns = wrap('tracker_columns');
export const trackerTransitions = wrap('tracker_transitions');
export const trackerTransition = wrap('tracker_transition');
export const trackerMove = wrap('tracker_move');
export const trackerComment = wrap('tracker_comment');
export const trackerAssign = wrap('tracker_assign');
export const trackerSearch = wrap('tracker_search');

// ---- reviews --------------------------------------------------------------------------------
export const reviewList = wrap('review_list');
export const reviewGet = wrap('review_get');
export const reviewApprove = wrap('review_approve');
export const reviewComment = wrap('review_comment');
export const reviewRequestChanges = wrap('review_request_changes');

// ---- work -----------------------------------------------------------------------------------
export const workPlan = wrap('work_plan');
export const workStart = wrap('work_start');
export const workList = wrap('work_list');
export const workResume = wrap('work_resume');
export const workRetryStep = wrap('work_retry_step');
export const workCreatePr = wrap('work_create_pr');
export const workFinish = wrap('work_finish');
export const workStatus = wrap('work_status');
export const workLink = wrap('work_link');
export const workStatusAll = wrap('work_status_all');
export const workDiff = wrap('work_diff');
export const workMarkReviewed = wrap('work_mark_reviewed');
export const editorOpen = wrap('editor_open');
export const editorSendSelection = wrap('editor_send_selection');

// ---- tools / plugins / triggers -------------------------------------------------------------
export const toolList = wrap('tool_list');
export const toolCheck = wrap('tool_check');
export const toolOpen = wrap('tool_open');
export const toolClose = wrap('tool_close');
export const pluginList = wrap('plugin_list');
export const pluginInspect = wrap('plugin_inspect');
export const pluginInstall = wrap('plugin_install');
export const pluginUninstall = wrap('plugin_uninstall');
export const pluginEnable = wrap('plugin_enable');
export const pluginGrant = wrap('plugin_grant');
export const pluginScreenOpen = wrap('plugin_screen_open');
export const pluginScreenClose = wrap('plugin_screen_close');
export const pluginCall = wrap('plugin_call');
export const commandRun = wrap('command_run');
export const triggerList = wrap('trigger_list');
export const triggerTest = wrap('trigger_test');
export const triggerLog = wrap('trigger_log');
