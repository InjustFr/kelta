// In-memory IPC mock (VITE_IPC=mock): implements every command of ARCHITECTURE §6 over the JSON
// fixtures in ./mock/fixtures plus the typed kelta-proto samples. Used by `pnpm dev:mock`, the
// Playwright e2e suites and vitest. Never part of the production bundle (dynamic import in main.ts).
//
// Test hooks: `createMockTransport()` returns `{ transport, controls }`; in the browser the active
// controls are exposed as `window.__keltaMock` (fail the next call, emit events, read the call log).

import * as samples from '$lib/gen/fixtures';
import type {
  ClaudeRun,
  Column,
  EffectiveSettings,
  FinishMergedReport,
  FinishOpts,
  GitStatus,
  JsonValue,
  KeltaError,
  Layer,
  LocalTags,
  NextUp,
  OutsideSession,
  Layout,
  Person,
  ProjectInfo,
  ReviewDetail,
  ReviewItem,
  ReviewNote,
  ReviewNotes,
  Scope,
  ScreenInstanceId,
  SessionInfo,
  SessionKind,
  Settings,
  Source,
  SourceHit,
  StatusCategory,
  Ticket,
  TicketDetail,
  TicketItem,
  TicketRef,
  ToolInfo,
  Transition,
  UiEvent,
  WorkflowInfo,
  UpdateStatus,
  WorkItem,
  WorkStepStatus,
} from '$lib/gen';
import { FRAME_DATA, FRAME_EXIT, FRAME_SNAPSHOT } from '$lib/gen/constants';
import { emptyLayout, layoutSessions, openPane } from '$lib/layout';

import type { CommandArgs, CommandName, CommandResult } from './commands';
import layoutsJson from './mock/fixtures/layouts.json';
import projectsJson from './mock/fixtures/projects.json';
import reviewsJson from './mock/fixtures/reviews.json';
import sourcesJson from './mock/fixtures/sources.json';
import sessionsJson from './mock/fixtures/sessions.json';
import ticketsJson from './mock/fixtures/tickets.json';
import toolsJson from './mock/fixtures/tools.json';
import workJson from './mock/fixtures/work_items.json';
import workStatusJson from './mock/fixtures/work_status.json';
import type { IpcChannel, IpcTransport } from './transport';

// JSON imports widen string-literal unions; the fixtures are validated by mock.test.ts.
const FIXTURES = {
  projects: projectsJson as unknown as ProjectInfo[],
  sessions: sessionsJson as unknown as SessionInfo[],
  layouts: layoutsJson as unknown as Record<string, Layout>,
  tickets: ticketsJson as unknown as TicketItem[],
  reviews: reviewsJson as unknown as ReviewItem[],
  work: workJson as unknown as WorkItem[],
  git: workStatusJson as Record<string, GitStatus>,
  tools: toolsJson as unknown as ToolInfo[],
  /** `tracker_sources` hits per account; absent account = provider without discovery. */
  sources: sourcesJson as unknown as Record<string, SourceHit[]>,
};

export const MOCK_FIXTURES: Readonly<typeof FIXTURES> = FIXTURES;

export interface MockCall {
  cmd: string;
  args: unknown;
}

export interface MockControls {
  /** Every invoke, in order. */
  readonly calls: MockCall[];
  /** Makes the next call of `cmd` reject with `error` (once). */
  failNext(cmd: CommandName, error: Partial<KeltaError> & { code: KeltaError['code'] }): void;
  /** Makes every call of `cmd` reject until `clearFailures()`. */
  failAlways(cmd: CommandName, error: Partial<KeltaError> & { code: KeltaError['code'] }): void;
  clearFailures(): void;
  /** Parks the next call of `cmd` until the returned function runs (with an error: it rejects). */
  holdNext(cmd: CommandName): (error?: Partial<KeltaError> & { code: KeltaError['code'] }) => void;
  /** Pushes a UiEvent to every `events_subscribe` channel. */
  emit(event: UiEvent): void;
  /** Sends raw output to an attached session (Data frame). */
  output(sessionId: string, text: string): void;
  /** Resets the in-memory state to the fixtures. */
  reset(): void;
  /** Direct access to the in-memory state (tests). */
  readonly state: MockState;
}

export interface MockState {
  projects: ProjectInfo[];
  sessions: SessionInfo[];
  layouts: Record<string, Layout>;
  tickets: TicketItem[];
  reviews: ReviewItem[];
  work: WorkItem[];
  /** `work_status` per work item (tests set it to drive phases). */
  git: Record<string, GitStatus>;
  tools: ToolInfo[];
  settings: EffectiveSettings;
  approved: Set<string>;
  comments: Record<string, string[]>;
  /** Commits on the remote branch the item does not have (suggestions, Update branch), per item. */
  remoteNew: Record<string, number>;
  /** Pending (draft) review line comments per `repo#number`. */
  pending: Record<string, number>;
  plugins: (typeof samples.pluginInfo)[];
  /** Review notes per work item. */
  notes: Record<string, ReviewNote[]>;
  /** Next up list and seen tickets (#145). */
  nextUp: NextUp;
  tags: LocalTags;
  /** Saved Claude workflows (`workflows_list`, #233). */
  workflows: WorkflowInfo[];
  /** Claude `Workflow` runs (`claude_runs_list`, #272); `workflow_run` adds one. */
  claudeRuns: ClaudeRun[];
  /** Claude sessions Kelta did not start (`outside_list`, #345). */
  outside: OutsideSession[];
  /** `app_update_status`; `app_update_check` stamps `checked_at`. */
  update: UpdateStatus;
}

interface MockOptions {
  /** Artificial latency per call in ms (one-shot timer per call). Default 0. */
  latencyMs?: number;
  /** Keeps the `global` layout across reloads, as the core's DB does (the app; unit tests start fresh). */
  keepGlobalLayout?: boolean;
}

type Handler<K extends CommandName> = (args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>>;
type Handlers = { [K in CommandName]: Handler<K> };

const clone = <T>(v: T): T => structuredClone(v);

function err(code: KeltaError['code'], message: string, detail: JsonValue | null = null): KeltaError {
  return { code, message, detail, retry_after_ms: null };
}

/** The mock's signed-in user (`Assignee::Me`, `who: mine`). */
const ME = { id: 'u-ada', name: 'Ada Lovelace', login: 'ada', avatar_url: null };
const MOCK_USERS = [
  ME,
  { id: 'u-grace', name: 'Grace Hopper', login: 'grace', avatar_url: null },
  { id: 'u-linus', name: 'Linus Torvalds', login: 'linus', avatar_url: null },
];
const MOCK_PRIORITIES = ['Highest', 'High', 'Medium', 'Low', 'Lowest'];
const MOCK_WORKFLOWS: WorkflowInfo[] = [
  {
    name: 'kelta-ticket-pass',
    description: 'Implement, review and merge one ticket',
    path: '/Users/ada/.claude/workflows/kelta-ticket-pass.js',
    source: 'home',
  },
  {
    name: 'triage',
    description: 'Reproduce and label a bug',
    path: '/Users/ada/code/shop/.claude/workflows/triage.js',
    source: 'repo',
  },
];

const CATEGORY_NAMES: Record<StatusCategory, string> = {
  todo: 'To do',
  in_progress: 'In progress',
  in_review: 'In review',
  done: 'Done',
  unknown: 'Unknown',
};

function sampleNotes(id: string): ReviewNote[] {
  const note = (
    n: number,
    path: string,
    line: number,
    body: string,
    st: ReviewNote['state'],
  ): ReviewNote => ({
    id: n,
    work_item_id: id,
    path,
    line_start: line,
    line_end: line + 2,
    body,
    source: 'nvim',
    ext_ref: null,
    state: st,
    sent_at: st === 'open' ? null : '2026-01-01T10:00:00Z',
  });
  return [
    note(1, 'src/checkout/rate_limit.rs', 10, 'use the existing backoff helper', 'open'),
    note(2, 'src/checkout/mod.rs', 3, 'name this after the ticket', 'untouched'),
  ];
}

/** The fixtures' `status_since` are written as of this instant; shifted to now so ages stay 3/10/16/25 days. */
const FIXTURE_NOW = Date.parse('2026-10-10T12:00:00Z');

function rebaseAges(items: TicketItem[]): TicketItem[] {
  const shift = Date.now() - FIXTURE_NOW;
  for (const { ticket: t } of items)
    if (t.status_since) t.status_since = new Date(Date.parse(t.status_since) + shift).toISOString();
  return items;
}

/** Default settings whose `[[sources]]` are the fixture projects' boards (as the core migrated them),
 *  plus a GitLab board feeding no project (the project-less fixture tickets come from it). */
function settingsWithSources(projects: ProjectInfo[]): JsonValue {
  const sources: Source[] = projects.flatMap((p) =>
    p.tracker
      ? [
          {
            id: p.id,
            label: `${p.name} board`,
            projects: [p.id],
            tracker: clone(p.tracker),
            routes: [],
            status_colors: {},
          },
        ]
      : [],
  );
  const base = projects.find((p) => p.tracker)?.tracker;
  if (base) {
    const view = { ...base.views[0]!, id: 'ops', label: 'Ops issues', jql: null, account: null };
    const tracker = { ...clone(base), account: 'gitlab-corp', views: [view], columns: [] };
    sources.push({ id: 'ops', label: 'Ops board', projects: [], tracker, routes: [], status_colors: {} });
  }
  return { ...clone(samples.settingsDefault), sources } as unknown as JsonValue;
}

function freshState(): MockState {
  return {
    projects: clone(FIXTURES.projects),
    sessions: clone(FIXTURES.sessions),
    layouts: clone(FIXTURES.layouts),
    tickets: rebaseAges(clone(FIXTURES.tickets)),
    reviews: clone(FIXTURES.reviews),
    work: clone(FIXTURES.work),
    git: clone(FIXTURES.git),
    tools: clone(FIXTURES.tools),
    settings: { value: settingsWithSources(FIXTURES.projects), sources: {} },
    approved: new Set(),
    comments: {},
    remoteNew: {},
    pending: { 'acme/shop-web#101': 2 },
    plugins: [clone(samples.pluginInfo)],
    notes: Object.fromEntries(FIXTURES.work.slice(0, 1).map((w) => [w.id, sampleNotes(w.id)])),
    tags: { tags: [], tickets: {}, prs: {} },
    // Every fixture ticket seen but the last: one `New` ticket to groom.
    nextUp: { items: [], seen: FIXTURES.tickets.slice(0, -1).map((t) => refKey(t.ticket.ref)) },
    workflows: clone(MOCK_WORKFLOWS),
    claudeRuns: [],
    outside: [],
    update: { ...clone(samples.updateStatus), state: { type: 'up_to_date' } },
  };
}

const prKey = (r: { repo: string; number: number }): string => `${r.repo}#${r.number}`;
const refKey = (r: TicketRef): string => `${r.account}:${r.key}`;
const sameRef = (a: TicketRef, b: TicketRef): boolean => a.account === b.account && a.key === b.key;
/** Fixture items whose worktree has uncommitted changes (the failed billing item, the merged wip). */
const dirtyWork = (w: WorkItem): boolean => w.id === FIXTURES.work[1]?.id || w.id === FIXTURES.work[7]?.id;
const FINISH_MERGED: FinishOpts = {
  remove_worktree: true,
  delete_branch: true,
  force: false,
  transition_to: null,
};

/** Realistic descriptions for the items the README screenshots show; other fixtures keep the filler. */
const BLURBS: Record<string, string> = {
  '4567': 'Invoices with several lines round each line, the ledger rounds the total: they differ by a cent.',
  'acme/shop-api#311':
    'Product images are fetched from S3 on every page view. Cache them at the edge for a day.',
};
const MOCK_BODY = 'Mock body rendered from the in-memory fixtures.';

function ticketDetailFor({ ticket: t, prs, caps }: TicketItem, comments: string[]): TicketDetail {
  // Headings, lists, a rule and a task list, as tracker descriptions carry them. No `#`: the title is the only h1.
  const blurb = BLURBS[t.ref.key] ?? MOCK_BODY;
  const body = `Ticket **${t.ref.key}** (${t.title}).\n\n${blurb}\n\n## Acceptance\n\n- [x] Shown in the detail\n- [ ] Covered by a test\n\n---\n\n### Notes\n\n1. First\n2. Second`;
  return {
    ticket: t,
    body_md: body,
    body_html:
      `<p>Ticket <strong>${t.ref.key}</strong> (${escapeHtml(t.title)}).</p><p>${escapeHtml(blurb)} <a href="${t.url}">Open in tracker</a></p>` +
      '<h2>Acceptance</h2><ul><li><input disabled="" type="checkbox" checked="">Shown in the detail</li><li><input disabled="" type="checkbox">Covered by a test</li></ul>' +
      '<hr><h3>Notes</h3><ol><li>First</li><li>Second</li></ol>',
    body_format: 'markdown',
    comments: [
      {
        author: { id: 'u-bob', name: 'Bob Martin', login: 'bob', avatar_url: null },
        created_at: '2026-10-08T09:12:00Z',
        body_html: '<p>Can we keep the existing behaviour behind a flag?</p>',
      },
      ...comments.map((c, i) => ({
        author: { id: 'u-ada', name: 'Ada Lovelace', login: 'ada', avatar_url: null },
        created_at: new Date(Date.UTC(2026, 9, 9, 12, i)).toISOString(),
        body_html: `<p>${escapeHtml(c)}</p>`,
      })),
    ],
    // Jira-like: the list's parent opens as a ticket.
    parent: t.parent ? { account: t.ref.account, key: t.parent.key, id: t.parent.key } : null,
    children: [],
    prs,
    caps,
  };
}

/** The assignees and reporters of a scope's tickets, as the core's `TicketPage.people`. */
function teamPeople(items: TicketItem[]): Person[] {
  const people = new Map<string, Person>();
  for (const t of items) {
    for (const u of [t.ticket.assignee, t.ticket.reporter]) {
      if (!u) continue;
      const me = u.id === ME.id;
      const key = me ? 'me' : `${t.ticket.ref.account}:${u.id}`;
      people.set(key, {
        key,
        name: me ? 'You' : u.name,
        avatar_url: u.avatar_url,
        ids: [{ account: t.ticket.ref.account, id: u.id, login: u.login, code_host: false }],
        matched: me,
        me,
      });
    }
  }
  return clone([...people.values()]);
}

function escapeHtml(s: string): string {
  return s.replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c,
  );
}

function defaultColumns(): Column[] {
  const cats: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done'];
  return cats.map((category, order) => ({
    id: category,
    name: CATEGORY_NAMES[category],
    category,
    order,
    match_names: [],
  }));
}

// Native status names per tracker, so transition sets differ across accounts (multi-select `m`).
const NATIVE_STATUSES: Record<string, [StatusCategory, string][]> = {
  'redmine-corp': [
    ['todo', 'New'],
    ['in_progress', 'In Progress'],
    ['done', 'Resolved'],
  ],
  'jira-acme': [
    ['todo', CATEGORY_NAMES.todo],
    ['in_progress', CATEGORY_NAMES.in_progress],
    ['in_review', CATEGORY_NAMES.in_review],
    ['done', CATEGORY_NAMES.done],
    ['in_progress', 'Blocked'],
  ],
};

function transitionsFor(t: Ticket): Transition[] {
  const cats: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done'];
  const native =
    NATIVE_STATUSES[t.ref.account] ?? cats.map((c): [StatusCategory, string] => [c, CATEGORY_NAMES[c]]);
  return native
    .filter(
      ([c, name]) =>
        name !== t.status.name &&
        (c !== t.status.category || name === 'Blocked' || t.status.name === 'Blocked'),
    )
    .map(([category, name]) => {
      const id = name === 'Blocked' ? 'to-blocked' : `to-${category}`;
      return {
        id,
        name,
        to: { id: `st-${id.slice(3)}`, name, category },
        needs_fields: category === 'done' && t.ref.account === 'jira-acme',
      };
    });
}

function setPath(root: JsonValue, path: string, value: JsonValue | undefined): JsonValue {
  const parts = path.split('.').filter(Boolean);
  const out = clone(root) as Record<string, JsonValue>;
  let node: Record<string, JsonValue> = out;
  parts.forEach((part, i) => {
    if (i === parts.length - 1) {
      if (value === undefined) delete node[part];
      else node[part] = value;
      return;
    }
    const next = node[part];
    if (typeof next !== 'object' || next === null || Array.isArray(next)) node[part] = {};
    node = node[part] as Record<string, JsonValue>;
  });
  return out;
}

/** Core's effective value carries serde defaults for the keys a `[[sources]]` entry omits (#281). */
const withSourceDefaults = (s: Partial<Source>): Source => ({
  id: '',
  label: '',
  projects: [],
  routes: [],
  status_colors: {},
  ...s,
  tracker: {
    account: '',
    views: [],
    columns: [],
    status_map: { start: null, review: null, done: null },
    repo_rules: [],
    ...s.tracker,
  },
});

function getPath(root: JsonValue, path: string): JsonValue | undefined {
  let node: JsonValue | undefined = root;
  for (const part of path.split('.').filter(Boolean)) {
    if (typeof node !== 'object' || node === null || Array.isArray(node)) return undefined;
    node = (node as Record<string, JsonValue>)[part];
  }
  return node;
}

let nextUuid = 0x100;
function uuid(): string {
  nextUuid += 1;
  return `0199a6b2-0000-7000-8000-${nextUuid.toString(16).padStart(12, '0')}`;
}

function kindLabel(kind: SessionKind): string {
  switch (kind.type) {
    case 'editor':
      return kind.adapter;
    case 'tool':
      return kind.tool_id;
    default:
      return kind.type;
  }
}

/** The `value` of the last `settings_set` call (tests: the last `sources` list written). */
export function lastSettingsValue<T>(mock: MockControls): T | undefined {
  return (mock.calls.filter((c) => c.cmd === 'settings_set').at(-1)?.args as { value: T } | undefined)?.value;
}

const GLOBAL_LAYOUT_KEY = 'kelta.mock.layout.global';

export function createMockTransport(options: MockOptions = {}): {
  transport: IpcTransport;
  controls: MockControls;
} {
  let state = freshState();
  const keep = options.keepGlobalLayout === true;
  try {
    const saved = keep ? sessionStorage.getItem(GLOBAL_LAYOUT_KEY) : null;
    if (saved) state.layouts.global = JSON.parse(saved) as Layout;
  } catch {
    // no storage: a reload starts without the global layout, like a fresh profile
  }
  const calls: MockCall[] = [];
  const failOnce = new Map<string, KeltaError>();
  const held = new Map<string, Promise<KeltaError | null>>();
  const failAlways = new Map<string, KeltaError>();
  const eventChannels = new Set<IpcChannel<UiEvent>>();
  const attached = new Map<string, { generation: number; channel: IpcChannel<Uint8Array> }>();
  let generation = 0;
  const screens = new Map<ScreenInstanceId, string>();
  // Claude Code found, Kelta's MCP not registered yet.
  let claudeMcp: boolean | null = false;
  let claudeHooks: boolean | null = false;

  const emit = (event: UiEvent): void => {
    for (const ch of eventChannels) {
      queueMicrotask(() => ch.onmessage(clone(event)));
    }
  };

  const frame = (tag: number, payload: Uint8Array): Uint8Array => {
    const out = new Uint8Array(payload.length + 1);
    out[0] = tag;
    out.set(payload, 1);
    return out;
  };

  const sendFrame = (sessionId: string, tag: number, payload: Uint8Array): void => {
    const a = attached.get(sessionId);
    if (!a) return;
    const f = frame(tag, payload);
    queueMicrotask(() => a.channel.onmessage(f));
  };

  const sourcesNow = (): Source[] => (state.settings.value as unknown as Settings).sources;
  /** `ProjectInfo.tracker` is the first source feeding the project (core `project_binding`). */
  const syncProjectTrackers = (): void => {
    for (const p of state.projects) {
      const tracker = sourcesNow().find((s) => s.projects.includes(p.id))?.tracker ?? null;
      if (JSON.stringify(tracker) === JSON.stringify(p.tracker)) continue;
      p.tracker = clone(tracker);
      emit({ type: 'project.updated', project: clone(p) });
    }
  };
  const source = (id: string): Source => {
    const s = sourcesNow().find((x) => x.id === id);
    if (!s) throw err('not_found', `source ${id}`);
    return s;
  };

  const project = (id: string): ProjectInfo => {
    const p = state.projects.find((x) => x.id === id);
    if (!p) throw err('not_found', `project not found: ${id}`);
    return p;
  };
  const session = (id: string): SessionInfo => {
    const s = state.sessions.find((x) => x.id === id);
    if (!s) throw err('not_found', `session not found: ${id}`);
    return s;
  };
  const work = (id: string): WorkItem => {
    const w = state.work.find((x) => x.id === id);
    if (!w) throw err('not_found', `work item not found: ${id}`);
    return w;
  };
  const notesOf = (id: string): ReviewNotes => ({
    worktree: work(id).worktree,
    notes: clone(state.notes[id] ?? []),
    since: null,
  });
  // A recorded rebase awaiting its force push is "diverged"; `remoteNew` drives "Remote has new commits".
  const gitOf = (w: WorkItem): GitStatus => {
    const base = clone(state.git[w.id] ?? samples.gitStatus);
    const pending = !!w.rebase && w.rebase.total === 0 && !!w.rebase.remote_sha;
    return {
      ...base,
      unpushed: base.unpushed || pending,
      diverged: pending,
      remote_new: state.remoteNew[w.id] ?? 0,
    };
  };
  const ticketItem = (ref: TicketRef): TicketItem => {
    const t = state.tickets.find((x) => sameRef(x.ticket.ref, ref));
    if (!t) throw err('not_found', `ticket not found: ${ref.key}`);
    return t;
  };
  const reviewItem = (r: { account: string; repo: string; number: number }): ReviewItem => {
    const found = state.reviews.find(
      (x) =>
        x.review.ref.account === r.account &&
        x.review.ref.repo === r.repo &&
        x.review.ref.number === r.number,
    );
    if (!found) throw err('not_found', `review not found: ${r.repo}#${r.number}`);
    return found;
  };
  const updateSession = (s: SessionInfo, patch: Partial<SessionInfo>): SessionInfo => {
    Object.assign(s, patch);
    emit({ type: 'session.updated', session: clone(s) });
    return clone(s);
  };
  const layoutOf = (projectId: string): Layout => {
    const existing = state.layouts[projectId];
    if (existing) return existing;
    const fresh = emptyLayout(projectId);
    state.layouts[projectId] = fresh;
    return fresh;
  };
  const backendLayoutChange = (projectId: string, next: Layout): void => {
    const saved = { ...next, rev: next.rev + 1 };
    state.layouts[projectId] = saved;
    emit({ type: 'layout.changed', project_id: projectId, layout: clone(saved) });
  };
  const updateTicket = (ref: TicketRef, category: StatusCategory, name?: string): Ticket => {
    const item = ticketItem(ref);
    item.ticket = {
      ...item.ticket,
      status: { id: `st-${category}`, name: name ?? CATEGORY_NAMES[category], category },
      updated_at: new Date().toISOString(),
      status_since: new Date().toISOString(),
    };
    const scopes = item.project_ids.map((id) => ({ kind: 'project' as const, id }));
    for (const scope of [...scopes, { kind: 'all' as const }]) emit({ type: 'tickets.changed', scope });
    return clone(item.ticket);
  };
  const spawn = (
    projectId: string,
    kind: SessionKind,
    name: string | null,
    cwd: string | null,
  ): SessionInfo => {
    project(projectId);
    const s: SessionInfo = {
      id: uuid(),
      project_id: projectId,
      kind,
      name: name ?? kindLabel(kind),
      title: null,
      cwd: cwd ?? project(projectId).repos[0]?.path ?? '/Users/ada',
      status: 'running',
      status_source: kind.type === 'claude' ? 'hook' : 'none',
      attention: 'none',
      seen: true,
      lifecycle: 'live',
      pid: 5000 + state.sessions.length,
      exit_code: null,
      work_item_id: null,
      claude:
        kind.type === 'claude'
          ? { session_uuid: uuid(), model: null, preview: null, files_touched: [], hooks_active: true }
          : null,
      editor: kind.type === 'editor' ? { adapter: kind.adapter, socket: null } : null,
      cols: 120,
      rows: 40,
      created_at: new Date().toISOString(),
    };
    state.sessions.push(s);
    emit({ type: 'session.updated', session: clone(s) });
    return clone(s);
  };
  const settingsWith = (layer: Layer, path: string, value: JsonValue | undefined): EffectiveSettings => {
    const next = setPath(state.settings.value, path, value) as unknown as Settings;
    if (path.split('.')[0] === 'sources') next.sources = (next.sources ?? []).map(withSourceDefaults);
    state.settings = { value: next as unknown as JsonValue, sources: { ...state.settings.sources } };
    if (value === undefined) delete state.settings.sources[path];
    else state.settings.sources[path] = layer;
    if (path.split('.')[0] === 'sources') syncProjectTrackers();
    emit({ type: 'settings.changed', layers: [layer], paths: [path], requires_restart: [] });
    return clone(state.settings);
  };

  const handlers: Handlers = {
    // ---- app -------------------------------------------------------------------------------
    app_info: () => clone(samples.appInfo),
    app_ready: () => null,
    bench_mark: () => null,
    events_subscribe: ({ channel }) => {
      eventChannels.add(channel);
      return { sub_id: eventChannels.size };
    },
    open_external: ({ url }) => {
      if (!/^(https?|mailto):/i.test(url)) throw err('invalid_argument', `refusing to open ${url}`);
      return null;
    },
    perf_snapshot: () => clone(samples.perfSnapshot),
    diagnostics_run: () => clone(samples.diagnostics),
    clipboard_read: () => 'mock clipboard text',
    clipboard_write: () => null,
    notify_test: () => {
      emit({ type: 'toast', toast: { level: 'info', text: 'Test notification sent', action: null } });
      return null;
    },
    claude_mcp_status: () => claudeMcp,
    claude_permission_mode: () => 'auto',
    claude_hooks_status: () => claudeHooks,
    claude_hooks_install: () => {
      claudeHooks = true;
      return null;
    },
    claude_hooks_remove: () => {
      claudeHooks = false;
      return null;
    },
    outside_list: () => clone(state.outside),
    // No window control in the browser: Peek with a note.
    outside_jump: () => 'Terminal',
    claude_mcp_install: () => {
      claudeMcp = true;
      return null;
    },
    claude_setup: () => {
      claudeMcp = true;
      const s = spawn('home', { type: 'claude' }, 'Set up Kelta', null);
      const { layout } = openPane(layoutOf('home'), {
        content: { kind: 'terminal', session_id: s.id },
        placement: 'new_tab',
        focus: true,
        tab_title: 'Set up Kelta',
        work_item_id: null,
      });
      backendLayoutChange('home', layout);
      return s;
    },
    claude_mcp_remove: () => {
      claudeMcp = false;
      return null;
    },
    app_update_status: () => clone(state.update),
    app_update_check: () => {
      state.update.checked_at = new Date().toISOString();
      return clone(state.update);
    },
    app_restart: () => null,
    app_quit: () => null,
    app_close: () => null,
    window_detach: () => null,
    // ---- settings --------------------------------------------------------------------------
    settings_schema: async () => {
      const schema = await import('../../../../schema/settings.schema.json');
      return clone(schema.default) as unknown as JsonValue;
    },
    settings_effective: () => clone(state.settings),
    settings_layer_get: ({ layer, project_id }) => ({
      ...clone(samples.layerDoc),
      path:
        layer === 'project' && project_id
          ? `/Users/ada/.config/kelta/projects/${project_id}.toml`
          : '/Users/ada/.config/kelta/config.toml',
    }),
    settings_set: ({ layer, path, value }) => settingsWith(layer, path, value),
    settings_reset: ({ layer, path }) => {
      const def = getPath(samples.settingsDefault as unknown as JsonValue, path);
      const next = settingsWith(layer, path, def);
      delete state.settings.sources[path];
      return { ...next, sources: { ...state.settings.sources } };
    },
    settings_validate: ({ text }) =>
      text.includes('=') || text.trim() === ''
        ? []
        : [{ path: '', message: 'expected `key = value`', line: 1, col: 1, severity: 'error' as const }],
    settings_write_raw: () => clone(state.settings),
    settings_open_file: ({ project_id }) =>
      spawn(
        project_id ?? 'home',
        { type: 'editor', adapter: 'nvim' },
        'config.toml',
        '/Users/ada/.config/kelta',
      ),
    repo_trust: ({ trust }) => ({ ...clone(samples.trustInfo), trusted: trust }),
    secret_set: () => null,
    secret_delete: () => null,
    secret_backends_status: () => clone(samples.secretBackendsStatus),
    secret_unlock: () => null,
    oauth_device_start: ({ kind }) => ({
      user_code: 'WDJB-MJHT',
      verification_uri:
        kind === 'github' ? 'https://github.com/login/device' : 'https://gitlab.com/oauth/device',
      expires_in: 900,
    }),
    oauth_device_finish: () => null,
    oauth_device_cancel: () => null,
    account_test: ({ account_id }) =>
      account_id.startsWith('broken')
        ? { ok: false, user: null, error: err('needs_auth', '401 Unauthorized') }
        : clone(samples.accountTestResult),
    // ---- projects --------------------------------------------------------------------------
    project_list: () => clone(state.projects),
    project_detect: ({ path }) => {
      const name = path.split('/').filter(Boolean).pop() ?? 'project';
      const draft = clone(samples.projectDraft);
      return {
        ...draft,
        suggested_id: name
          .toLowerCase()
          .replace(/[^a-z0-9-]/g, '-')
          .slice(0, 40),
        name,
      };
    },
    project_create: ({ draft }) => {
      if (state.projects.some((p) => p.id === draft.suggested_id))
        throw err('conflict', `project ${draft.suggested_id} exists`);
      const p: ProjectInfo = {
        id: draft.suggested_id,
        name: draft.name,
        color: draft.color,
        icon: draft.icon,
        repos: draft.repos.map((r) => ({
          id: r.id,
          path: r.path,
          primary: r.primary,
          remote: r.remote,
          base: r.base,
          code_host: r.code_host,
          exists: true,
        })),
        tracker: draft.tracker,
        open: true,
        active: false,
        attention: { level: 'none', needs_input: 0 },
        builtin: false,
      };
      state.projects.push(p);
      emit({ type: 'project.updated', project: clone(p) });
      if (draft.tracker) {
        const s = { id: p.id, label: p.name, projects: [p.id], tracker: draft.tracker, routes: [] };
        settingsWith('global', 'sources', [...sourcesNow(), s] as unknown as JsonValue);
      }
      return clone(p);
    },
    project_update: ({ id, patch }) => {
      const p = project(id);
      if (patch.name !== null) p.name = patch.name;
      if (patch.color !== null) p.color = patch.color;
      if (patch.icon !== null) p.icon = patch.icon;
      if (patch.tracker !== null) p.tracker = patch.tracker;
      if (patch.remove_tracker) p.tracker = null;
      emit({ type: 'project.updated', project: clone(p) });
      return clone(p);
    },
    project_remove: ({ id, kill_sessions }) => {
      const p = project(id);
      if (p.builtin) throw err('invalid_argument', 'the Home project cannot be removed');
      state.projects = state.projects.filter((x) => x.id !== id);
      if (kill_sessions) {
        for (const s of state.sessions.filter((x) => x.project_id === id))
          emit({ type: 'session.removed', id: s.id });
        state.sessions = state.sessions.filter((x) => x.project_id !== id);
      }
      emit({ type: 'project.removed', id });
      return null;
    },
    project_open: ({ id }) => {
      const p = project(id);
      p.open = true;
      emit({ type: 'project.updated', project: clone(p) });
      return clone(p);
    },
    project_close: ({ id, kill_sessions }) => {
      const p = project(id);
      p.open = false;
      p.active = false;
      if (kill_sessions) {
        for (const s of state.sessions.filter((x) => x.project_id === id))
          emit({ type: 'session.removed', id: s.id });
        state.sessions = state.sessions.filter((x) => x.project_id !== id);
      }
      emit({ type: 'project.updated', project: clone(p) });
      return clone(p);
    },
    project_activate: ({ id }) => {
      const target = project(id);
      for (const p of state.projects) {
        const was = p.active;
        p.active = p.id === id;
        if (p.active) p.open = true;
        if (was !== p.active) emit({ type: 'project.updated', project: clone(p) });
      }
      return clone(target);
    },
    project_reorder: ({ ids }) => {
      const order = new Map(ids.map((id, i) => [id, i]));
      state.projects.sort((a, b) => (order.get(a.id) ?? 1e9) - (order.get(b.id) ?? 1e9));
      return null;
    },
    // ---- layout ----------------------------------------------------------------------------
    layout_get: ({ project_id }) => {
      if (project_id !== 'global') project(project_id);
      return clone(layoutOf(project_id));
    },
    layout_save: ({ layout }) => {
      const current = layoutOf(layout.project_id);
      if (layout.rev !== current.rev)
        throw err('conflict', `stale layout rev ${layout.rev} (current ${current.rev})`, {
          rev: current.rev,
        });
      const rev = current.rev + 1;
      state.layouts[layout.project_id] = { ...clone(layout), rev };
      if (keep && layout.project_id === 'global')
        try {
          sessionStorage.setItem(GLOBAL_LAYOUT_KEY, JSON.stringify(state.layouts.global));
        } catch {
          // no storage: kept until reload
        }
      return { rev };
    },
    // ---- sessions --------------------------------------------------------------------------
    session_spawn: ({ req }) => {
      const s = spawn(req.project_id, req.kind, req.name, req.cwd);
      if (req.work_item_id) {
        const live = session(s.id);
        live.work_item_id = req.work_item_id;
        return clone(live);
      }
      return s;
    },
    session_spawn_template: ({ project_id, template_id, placement, ctx }) => {
      const item = ctx.work_item_id ? state.work.find((w) => w.id === ctx.work_item_id) : undefined;
      const tabTitle = item
        ? item.ticket
          ? `${item.ticket.key} ${ticketItem(item.ticket).ticket.title}`
          : item.title
            ? `wip ${item.title}`
            : item.branch
        : template_id;
      const kinds: SessionKind[] =
        template_id === 'claude+editor'
          ? [{ type: 'claude' }, { type: 'editor', adapter: 'nvim' }]
          : template_id === 'claude'
            ? [{ type: 'claude' }]
            : template_id === 'nvim' || template_id === 'editor'
              ? [{ type: 'editor', adapter: 'nvim' }]
              : [{ type: 'shell' }];
      const spawned = kinds.map((k) => spawn(project_id, k, null, null));
      let layout = layoutOf(project_id);
      spawned.forEach((s, i) => {
        layout = openPane(layout, {
          content: { kind: 'terminal', session_id: s.id },
          placement: i === 0 ? placement : 'split_right',
          focus: i === 0,
          tab_title: i === 0 ? tabTitle : null,
          work_item_id: item?.id ?? null,
        }).layout;
      });
      backendLayoutChange(project_id, layout);
      return spawned;
    },
    session_attach: ({ id, cols, rows, channel }) => {
      const s = session(id);
      generation += 1;
      attached.set(id, { generation, channel: channel as unknown as IpcChannel<Uint8Array> });
      if (s.lifecycle === 'dormant')
        updateSession(s, { lifecycle: 'live', status: 'running', pid: 6000 + generation });
      if (s.cols !== cols || s.rows !== rows) Object.assign(s, { cols, rows });
      const text =
        `\x1b[2J\x1b[H\x1b[1;36mkelta mock\x1b[0m session \x1b[1m${s.name}\x1b[0m (${kindLabel(s.kind)})\r\n` +
        `cwd ${s.cwd}\r\nstatus ${s.status}\r\n$ `;
      sendFrame(id, FRAME_SNAPSHOT, new TextEncoder().encode(text));
      if (s.lifecycle === 'exited') {
        const code = new Uint8Array(4);
        new DataView(code.buffer).setInt32(0, s.exit_code ?? -1, true);
        sendFrame(id, FRAME_EXIT, code);
      }
      return { generation, cols, rows };
    },
    session_detach: ({ id, generation: gen }) => {
      const a = attached.get(id);
      if (a && a.generation === gen) attached.delete(id);
      return null;
    },
    session_write: ({ id, data }) => {
      session(id);
      // Echo input back like a cooked tty.
      const text = new TextDecoder().decode(data).replace(/\r/g, '\r\n');
      sendFrame(id, FRAME_DATA, new TextEncoder().encode(text));
      return null;
    },
    session_resize: ({ id, cols, rows }) => {
      Object.assign(session(id), { cols, rows });
      return null;
    },
    session_ack: () => null,
    session_kill: ({ id }) => {
      session(id);
      state.sessions = state.sessions.filter((s) => s.id !== id);
      attached.delete(id);
      emit({ type: 'session.removed', id });
      return null;
    },
    session_restart: ({ id }) =>
      updateSession(session(id), {
        lifecycle: 'live',
        status: 'running',
        exit_code: null,
        pid: 7000 + Math.floor(Math.random() * 1000),
      }),
    session_rename: ({ id, name }) => updateSession(session(id), { name }),
    session_list: ({ project_id }) =>
      clone(project_id ? state.sessions.filter((s) => s.project_id === project_id) : state.sessions),
    session_mark_seen: ({ id }) => {
      const s = session(id);
      if (!s.seen || s.attention === 'done' || s.attention === 'activity') {
        updateSession(s, { seen: true, attention: s.attention === 'needs_input' ? 'needs_input' : 'none' });
      }
      return null;
    },
    session_link: ({ id, work_item_id }) =>
      updateSession(session(id), { work_item_id: work_item_id ?? null }),
    session_text_tail: ({ id, max_lines }) => {
      const s = session(id);
      return Array.from({ length: Math.min(max_lines, 5) }, (_, i) => `${s.name} line ${i + 1}`).join('\n');
    },
    session_history_search: ({ project_id, session_id, query, limit }) =>
      state.sessions
        .filter((s) => s.project_id === project_id && (!session_id || s.id === session_id))
        .map((s) => ({ session_id: s.id, line: `${s.name}: ${query}` }))
        .slice(0, limit),
    claude_runs_list: ({ id }) => clone(state.claudeRuns.filter((r) => r.session_id === id)),
    // Any run and agent: e2e specs emit runs the state does not hold.
    claude_run_tail: ({ agent_id }) => [
      { kind: 'text', text: `${agent_id}: reading the code` },
      { kind: 'tool', text: 'Bash: Run the tests' },
      { kind: 'result', text: 'ok. 12 passed' },
    ],
    terminal_set_palette: () => null,
    // ---- tickets ---------------------------------------------------------------------------
    tracker_list: ({ scope: asked, source_id, view_id: askedView, cursor, who }) => {
      // core `list_scope`: the source when it is in the scope and has views, else the scope; the
      // view when a listed source has it
      const within = (sc: Scope) =>
        sourcesNow().filter((s) =>
          sc.kind === 'all' ? true : sc.kind === 'source' ? s.id === sc.id : s.projects.includes(sc.id),
        );
      const narrowed = within(asked).find((s) => s.id === source_id && s.tracker.views.length > 0);
      const scope: Scope = narrowed ? { kind: 'source', id: narrowed.id } : asked;
      const inScope = within(scope);
      const view_id =
        (!source_id || narrowed) && inScope.some((s) => s.tracker.views.some((v) => v.id === askedView))
          ? (askedView ?? null)
          : null;
      // the sources of the scope that feed one of the ticket's projects; the view the core would
      // query (`view_id` inside the first one, else its first view)
      const scoped = state.tickets
        .map((t) => {
          // a project-less ticket comes from the sources feeding no project
          const from = inScope.filter((s) =>
            t.project_ids.length ? s.projects.some((p) => t.project_ids.includes(p)) : !s.projects.length,
          );
          const views = from[0]?.tracker.views ?? [];
          return { item: t, from, view: views.find((v) => v.id === view_id) ?? views[0] };
        })
        .filter(({ from }) => from.length > 0);
      const items = scoped
        .filter(({ item, view }) => {
          const w = who ?? view?.who;
          return w === 'mine'
            ? item.ticket.assignee?.id === ME.id
            : w === 'unassigned'
              ? !item.ticket.assignee
              : true;
        })
        .map(({ item, view, from }) => ({
          ...item,
          view_ids: view ? [view.id] : [],
          source_ids: from.map((s) => s.id),
        }));
      // `view_id` null = the union of the views (WP2), which keeps done tickets
      const filtered =
        view_id === 'sprint' || view_id == null
          ? items
          : items.filter((t) => t.ticket.status.category !== 'done' || view_id === 'all');
      const pageSize = 50;
      const offset = cursor && cursor.kind === 'offset' ? cursor.value : 0;
      const page = filtered.slice(offset, offset + pageSize);
      return {
        items: clone(page),
        next: offset + pageSize < filtered.length ? { kind: 'offset', value: offset + pageSize } : null,
        stale: false,
        errors: [],
        people: teamPeople(scoped.map((s) => s.item)),
        scope,
        view_id,
      };
    },
    tracker_get: ({ ticket }) => {
      const item = ticketItem(ticket);
      const detail = ticketDetailFor(clone(item), state.comments[refKey(ticket)] ?? []);
      // One parent with a sub-task, for the detail's Sub-tasks section (TICKETS.md T7).
      if (ticket.key === 'SHOP-142')
        detail.children = [clone(ticketItem({ ...ticket, key: 'SHOP-160', id: '10160' }))];
      return detail;
    },
    tracker_columns: ({ source_id }) => {
      const { tracker } = source(source_id);
      if (tracker.columns.length === 0) return defaultColumns();
      return tracker.columns.map((c, order) => ({
        id: c.id,
        name: c.label,
        category: c.categories[0] ?? 'unknown',
        order,
        match_names: c.names,
      }));
    },
    tracker_transitions: ({ ticket }) => transitionsFor(ticketItem(ticket).ticket),
    tracker_transition: ({ ticket, transition_id, fields }) => {
      const t = transitionsFor(ticketItem(ticket).ticket).find((x) => x.id === transition_id);
      if (!t) throw err('not_found', `transition ${transition_id} not available`);
      // The tracker refuses this one (workflow rule): the StatusPicker's error path.
      if (ticket.account === 'redmine-corp' && ticket.key === '4567' && t.to.category === 'done') {
        throw err('conflict', 'Redmine: status transition not allowed (422)');
      }
      if (t.needs_fields && !fields) {
        throw err('needs_fields', `${t.name} requires fields`, {
          fields: [{ id: 'resolution', name: 'Resolution', required: true }],
        });
      }
      return updateTicket(ticket, t.to.category, t.to.name);
    },
    tracker_move: ({ ticket, column_id, source_id }) => {
      const item = ticketItem(ticket);
      const sourceId =
        source_id ?? sourcesNow().find((s) => s.projects.includes(item.project_ids[0] ?? ''))?.id;
      const columns = sourceId ? handlers.tracker_columns({ source_id: sourceId }) : defaultColumns();
      const col = (columns as Column[]).find((c) => c.id === column_id);
      if (!col) throw err('not_found', `column ${column_id} not found`);
      if (col.category === item.ticket.status.category) return clone(item.ticket);
      return updateTicket(ticket, col.category, col.name);
    },
    tracker_comment: ({ ticket, markdown }) => {
      if (!ticketItem(ticket).caps.comment) throw err('unsupported', `${ticket.account} cannot comment`);
      const key = refKey(ticket);
      state.comments[key] = [...(state.comments[key] ?? []), markdown];
      return null;
    },
    tracker_assign: ({ ticket, assignee }) => {
      const item = ticketItem(ticket);
      if (!item.caps.assign) throw err('unsupported', `${ticket.account} cannot assign`);
      item.ticket.assignee =
        assignee.kind === 'none'
          ? null
          : assignee.kind === 'me'
            ? { ...ME }
            : { id: assignee.id, name: assignee.id, login: null, avatar_url: null };
      return clone(item.ticket);
    },
    tracker_assignable_users: ({ ticket, query }) => {
      if (!ticketItem(ticket).caps.assign) throw err('unsupported', `${ticket.account} cannot assign`);
      const q = query.trim().toLowerCase();
      return clone(MOCK_USERS.filter((u) => `${u.name} ${u.login}`.toLowerCase().includes(q)));
    },
    tracker_priorities: ({ ticket }) => {
      if (!ticketItem(ticket).caps.assign)
        throw err('unsupported', `${ticket.account} cannot set priorities`);
      return [...MOCK_PRIORITIES];
    },
    tracker_set_priority: ({ ticket, priority }) => {
      const item = ticketItem(ticket);
      const rank = MOCK_PRIORITIES.indexOf(priority);
      if (rank < 0) throw err('not_found', `no priority named ${priority}`);
      item.ticket.priority = priority;
      item.ticket.priority_rank = rank;
      return clone(item.ticket);
    },
    tracker_set_blocked: ({ ticket, blocked }) => {
      const item = ticketItem(ticket);
      if (!item.caps.block) throw err('unsupported', `${ticket.account} cannot set this blocked marker`);
      item.ticket.blocked = blocked;
      return clone(item.ticket);
    },
    tracker_refine: ({ ticket }) => {
      const t = ticketItem(ticket).ticket;
      return [
        '## Acceptance criteria',
        '',
        `- [ ] ${t.title} works end to end`,
        '- [ ] Covered by a test',
        '',
        '## Open questions',
        '',
        'None',
        '',
        '## Sub-tasks',
        '',
        'None',
      ].join('\n');
    },
    workflows_list: ({ ticket }) => {
      ticketItem(ticket);
      return clone(state.workflows);
    },
    // Like `Core::workflow_run`: claim (`Starting`), then a Claude session in a background tab.
    workflow_run: ({ ticket, workflow }) => {
      const t = ticketItem(ticket);
      if (!state.workflows.some((w) => w.name === workflow)) throw err('not_found', `workflow ${workflow}`);
      const held = state.work.find(
        (w) => w.ticket && sameRef(w.ticket, ticket) && w.state.kind !== 'finished',
      );
      if (held) throw err('conflict', `${ticket.key} is held by work item ${held.id} (started in Kelta)`);
      const projectId = t.project_ids[0] ?? state.projects.find((p) => p.active)?.id ?? 'shop';
      const title = `${workflow} · ${ticket.key}`;
      const s = spawn(projectId, { type: 'claude' }, title, null);
      const item: WorkItem = {
        id: uuid(),
        project_id: projectId,
        kind: 'ticket',
        ticket: { ...ticket }, // a plain copy: the arg can be a UI state proxy
        review: null,
        repo_id: project(projectId).repos[0]?.id ?? 'main',
        worktree: s.cwd,
        branch: 'main',
        base: 'main',
        claude_uuid: null,
        nvim_socket: null,
        session_ids: [],
        tab_id: null,
        pr_url: null,
        state: { kind: 'active' },
        steps: [],
        created_at: new Date().toISOString(),
        title: null,
        pr_title_needs_key: false,
        cost_usd: 0,
        review_due: false,
        claude_replied: false,
        auto_finish: false,
        nvim_kept: 0,
        run: {
          name: workflow,
          run_id: `${workflow}-${s.id.slice(0, 8)}`,
          session_id: s.id,
          phase: 'Starting',
          state: 'running',
          note: null,
          sha: null,
        },
      };
      state.work.push(item);
      emit({ type: 'work.updated', work: clone(item) });
      // What the hooks then report: the run, its first agent spawned (#272).
      const wf: ClaudeRun = {
        run_id: `wf_${s.id.slice(0, 8)}-000`,
        session_id: s.id,
        name: workflow,
        status: 'running',
        phases: [{ title: 'Pick', agents: [{ id: `a${s.id.slice(0, 8)}`, label: 'pick', done: false }] }],
        duration_ms: null,
        total_tokens: null,
        total_tool_calls: null,
        result: null,
        logs: [],
      };
      state.claudeRuns.push(wf);
      emit({ type: 'workflow.updated', run: clone(wf) });
      const content = { kind: 'terminal' as const, session_id: s.id };
      const req = {
        content,
        placement: 'new_tab' as const,
        focus: false,
        tab_title: title,
        work_item_id: null,
      };
      backendLayoutChange(projectId, openPane(layoutOf(projectId), req).layout);
      return s;
    },
    tracker_search: ({ scope, text }) => {
      const q = text.toLowerCase();
      return clone(
        state.tickets.filter(
          (t) =>
            (scope.kind === 'all' || t.project_ids.includes(scope.id)) &&
            (t.ticket.ref.key.toLowerCase().includes(q) || t.ticket.title.toLowerCase().includes(q)),
        ),
      );
    },
    tracker_sources: ({ account_id, query }) => {
      const hits = FIXTURES.sources[account_id];
      if (!hits) throw err('unsupported', `${account_id} cannot list sources`);
      const q = query.trim().toLowerCase();
      return clone(hits.filter((h) => h.label.toLowerCase().includes(q)));
    },
    next_up_list: () => clone(state.nextUp),
    next_up_put: ({ item }) => {
      const key = refKey(item.ticket);
      const rest = state.nextUp.items.filter((i) => refKey(i.ticket) !== key);
      state.nextUp.items = [...rest, { ...item, ticket: { ...item.ticket } }]; // args may be $state proxies
      return null;
    },
    next_up_remove: ({ ticket }) => {
      state.nextUp.items = state.nextUp.items.filter((i) => refKey(i.ticket) !== refKey(ticket));
      return null;
    },
    ticket_seen: ({ tickets }) => {
      for (const t of tickets) if (!state.nextUp.seen.includes(refKey(t))) state.nextUp.seen.push(refKey(t));
      return null;
    },
    tags_get: () => clone(state.tags),
    tags_set: ({ tags }) => {
      state.tags = JSON.parse(JSON.stringify(tags)) as LocalTags; // args may be $state proxies
      return clone(state.tags);
    },
    // ---- reviews ---------------------------------------------------------------------------
    // Team: every PR of the scope, as the core lists the source-fed repos' open PRs.
    review_list: ({ scope, kind }) => ({
      items: clone(
        state.reviews.filter(
          (r) =>
            (kind === 'team' ? r.project_ids.length > 0 : r.review.kind === kind) &&
            (scope.kind === 'all' || r.project_ids.includes(scope.id)),
        ),
      ).map((r) =>
        kind === 'team' ? { ...r, review: { ...r.review, kind, my_state: null, reviewed_head: null } } : r,
      ),
      stale: false,
      errors: [],
    }),
    review_get: ({ review }) => {
      const item = reviewItem(review);
      const detail: ReviewDetail = {
        ...clone(samples.reviewDetail),
        review: clone(item.review),
        pending_comments: state.pending[prKey(review)] ?? 0,
        body_html: `<p>${escapeHtml(item.review.title)}</p><p>${escapeHtml(BLURBS[prKey(review)] ?? 'Mock description.')} <a href="${item.review.url}">View on host</a></p>`,
      };
      return detail;
    },
    review_approve: ({ review, head_sha }) => {
      const item = reviewItem(review);
      if (item.review.head_sha !== head_sha) {
        throw err('conflict', 'PR changed, refresh', { head_sha: item.review.head_sha });
      }
      item.review.my_state = 'approved';
      item.review.reviewed_head = head_sha;
      delete state.pending[prKey(review)];
      state.approved.add(prKey(review));
      emit({ type: 'reviews.changed', scope: { kind: 'all' }, new_keys: [] });
      return null;
    },
    review_comment: ({ review }) => {
      const item = reviewItem(review);
      if (!item.review.my_state || item.review.my_state === 'pending') item.review.my_state = 'commented';
      item.review.reviewed_head = item.review.head_sha;
      delete state.pending[prKey(review)];
      return null;
    },
    review_request_changes: ({ review }) => {
      const item = reviewItem(review);
      item.review.my_state = 'changes_requested';
      item.review.reviewed_head = item.review.head_sha;
      delete state.pending[prKey(review)];
      return null;
    },
    review_nudge: ({ review }) => {
      const item = reviewItem(review);
      const last = item.review.nudged_at ? Date.parse(item.review.nudged_at) : 0;
      if (Date.now() - last < 24 * 3600_000) throw err('invalid_argument', 'already nudged in the last 24 h');
      item.review.nudged_at = new Date().toISOString();
      emit({ type: 'reviews.changed', scope: { kind: 'all' }, new_keys: [] });
      return null;
    },
    review_set_draft: ({ review, draft }) => {
      reviewItem(review).review.draft = draft;
      emit({ type: 'reviews.changed', scope: { kind: 'all' }, new_keys: [] });
      return null;
    },
    // ---- work ------------------------------------------------------------------------------
    work_plan: ({ project_id: asked, source }) => {
      // Simplified SPEC §3.1: the existing item, the caller's pick, a route, the last pick for the
      // source, the source's projects (a guess when several), the active one (a guess). The mock's
      // source of a ticket is its account; the projects whose tracker uses it are the ones it feeds.
      const choices = state.projects.filter((x) => x.open && x.id !== 'home' && x.repos.length > 0);
      const ref = source.kind === 'ticket' ? source.ticket : null;
      const owner = ref
        ? state.work.find((w) => w.ticket && sameRef(w.ticket, ref) && w.state.kind !== 'finished')
        : undefined;
      const fed = ref ? choices.filter((x) => x.tracker?.account === ref.account) : [];
      const t = ref ? ticketItem(ref).ticket : null;
      const has = (l: string | null) => !l || !!t?.labels.some((x) => x.toLowerCase() === l.toLowerCase());
      const route = fed
        .flatMap((x) => (x.tracker?.repo_rules ?? []).map((r) => ({ project: x.id, ...r })))
        .find(({ match: m }) => {
          const kindOk = !m.type || t?.kind?.toLowerCase() === m.type.toLowerCase();
          return !!(m.component || m.label || m.type) && has(m.component) && has(m.label) && kindOk;
        });
      const last = ref
        ? state.work
            .filter((w) => (w.ticket_source ?? w.ticket?.account) === ref.account)
            .filter((w) => choices.some((x) => x.id === w.project_id))
            .sort((a, b) => b.created_at.localeCompare(a.created_at))[0]?.project_id
        : undefined;
      const usual = asked ?? route?.project ?? last ?? (fed.length === 1 ? fed[0]?.id : undefined);
      const guessed = source.kind === 'ticket' && !owner && !usual;
      const project_id =
        owner?.project_id ??
        usual ??
        (fed[0] ?? choices.find((x) => x.active) ?? choices[0] ?? state.projects[0])?.id ??
        'home';
      const p = project(project_id);
      const plan = clone(samples.startWorkPlan);
      const repo =
        p.repos.find((r) => route?.project === project_id && r.id === route.repo) ??
        p.repos.find((r) => r.primary) ??
        p.repos[0];
      let slug: string;
      if (source.kind === 'ticket') {
        const t = ticketItem(source.ticket);
        slug = `${t.ticket.ref.key.replace(/^#/, 'gh-')}-${t.ticket.title}`;
      } else if (source.kind === 'review') {
        slug = `pr-${source.review.number}`;
      } else {
        slug = source.name || (source.task ?? '').split('\n').find((l) => l.trim() !== '') || '';
      }
      slug = slug
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-|-$/g, '')
        .slice(0, 40);
      const existing =
        source.kind === 'ticket'
          ? state.work.find(
              (w) => w.ticket && sameRef(w.ticket, source.ticket) && w.state.kind !== 'finished',
            )
          : undefined;
      // Scratch items (FLOW §4.3): wip/{slug}, `{task}` prompt, never adopt an existing branch.
      const scratch = source.kind === 'branch' && source.task != null;
      const branch =
        source.kind === 'review'
          ? `kelta/${slug}`
          : source.kind === 'branch'
            ? source.name || `wip/${slug}`
            : `feat/${slug}`;
      if (scratch && slug === '') throw err('invalid_argument', 'describe the task or name the branch');
      if (scratch && state.work.some((w) => w.branch === branch && w.state.kind !== 'finished')) {
        throw err(
          'conflict',
          `Branch ${branch} has a work item. Edit the branch name or the task's first line.`,
        );
      }
      const repoPick = (source.kind === 'branch' && p.repos.find((r) => r.id === source.repo)) || repo;
      return {
        ...plan,
        project_id,
        source,
        repo_id: existing?.repo_id ?? repoPick?.id ?? plan.repo_id,
        repo_choices: p.repos.map((r) => r.id),
        project_choices:
          source.kind === 'branch'
            ? []
            : [...fed, ...choices.filter((x) => !fed.includes(x))].map((x) => x.id),
        project_guessed: guessed,
        ticket_source: ref && fed.length ? ref.account : null,
        // An existing item shows its own branch and worktree, not a new plan's (B5).
        branch: existing?.branch ?? branch,
        worktree_path:
          existing?.worktree ?? `${repoPick?.path ?? '/tmp'}.worktrees/${branch.replace(/\//g, '-')}`,
        existing: existing ? existing.id : null,
        claude: scratch ? { ...plan.claude, prompt: source.task ?? '' } : plan.claude,
        side_effects:
          source.kind === 'branch'
            ? { ...plan.side_effects, assign_me: false, transition_to: null, comment: null }
            : plan.side_effects,
      };
    },
    work_start: ({ plan }) => {
      const names = [
        'before_start',
        'fetch_ticket',
        'fetch_base',
        'worktree',
        'include_files',
        'claude_files',
      ];
      const rest = ['layout', 'setup', 'editor', 'claude', 'tracker_side_effects', 'persist'];
      const now = new Date().toISOString();
      const steps: WorkStepStatus[] = [...names, ...rest].map((step) => ({
        step,
        status: 'done',
        detail: null,
        updated_at: now,
      }));
      const item: WorkItem = {
        id: uuid(),
        project_id: plan.project_id,
        kind: plan.source.kind,
        ticket: plan.source.kind === 'ticket' ? plan.source.ticket : null,
        review: plan.source.kind === 'review' ? plan.source.review : null,
        repo_id: plan.repo_id,
        worktree: plan.worktree_path,
        branch: plan.branch,
        base: plan.base,
        claude_uuid: uuid(),
        nvim_socket: null,
        session_ids: [],
        tab_id: null,
        pr_url: null,
        state: { kind: 'active' },
        steps,
        created_at: now,
        title:
          plan.source.kind === 'branch'
            ? ((plan.source.task ?? '')
                .split('\n')
                .map((l) => l.trim())
                .find((l) => l !== '')
                ?.slice(0, 72) ?? null)
            : null,
        pr_title_needs_key: false,
        cost_usd: 0,
        review_due: false,
        claude_replied: false,
        auto_finish: false,
        nvim_kept: 0,
        ticket_source: plan.ticket_source ?? null,
      };
      // Like the saga: sessions run in the worktree, bound to the item, in a new active tab that
      // carries the item (B5); the UI brings the project to the front once work_start returns.
      const kinds: SessionKind[] =
        plan.template_id === 'claude'
          ? [{ type: 'claude' }]
          : [{ type: 'claude' }, { type: 'editor', adapter: 'nvim' }];
      const spawned = kinds.map((k) => spawn(plan.project_id, k, null, plan.worktree_path));
      item.session_ids = spawned.map((s) => s.id);
      for (const s of spawned) updateSession(session(s.id), { work_item_id: item.id });
      const key = item.ticket?.key ?? (item.review ? `#${item.review.number}` : item.branch);
      const title =
        plan.source.kind === 'ticket'
          ? `${key} ${ticketItem(plan.source.ticket).ticket.title}`
          : item.title
            ? `wip ${item.title}`
            : key;
      let layout = layoutOf(plan.project_id);
      spawned.forEach((s, i) => {
        layout = openPane(layout, {
          content: { kind: 'terminal', session_id: s.id },
          placement: i === 0 ? 'new_tab' : 'split_right',
          focus: i === 0,
          tab_title: i === 0 ? title : null,
          work_item_id: item.id,
        }).layout;
      });
      backendLayoutChange(plan.project_id, layout);
      state.git[item.id] = {
        ahead: 0,
        behind: 0,
        dirty: false,
        unpushed: false,
        files: 0,
        insertions: 0,
        deletions: 0,
        missing: false,
      };
      state.work.push(item);
      emit({ type: 'work.updated', work: clone(item) });
      return clone(item);
    },
    work_list: ({ project_id }) =>
      clone(project_id ? state.work.filter((w) => w.project_id === project_id) : state.work),
    work_resume: ({ id }) => {
      const w = work(id);
      for (const sid of w.session_ids) {
        const s = state.sessions.find((x) => x.id === sid);
        if (s && s.lifecycle === 'dormant') updateSession(s, { lifecycle: 'live', status: 'running' });
      }
      // A closed work tab is recreated around the item's sessions.
      const current = layoutOf(w.project_id);
      if (!current.tabs.some((t) => t.work_item_id === w.id) && w.session_ids.length > 0) {
        let l = current;
        w.session_ids.forEach((sid, i) => {
          l = openPane(l, {
            content: { kind: 'terminal', session_id: sid },
            placement: i === 0 ? 'new_tab' : 'split_right',
            focus: i === 0,
            tab_title: i === 0 ? (w.ticket?.key ?? w.branch) : null,
            work_item_id: w.id,
          }).layout;
        });
        backendLayoutChange(w.project_id, l);
      }
      return clone(w);
    },
    work_retry_step: ({ id, step }) => {
      const w = work(id);
      const now = new Date().toISOString();
      // `skip:<step>` marks the step skipped and continues the saga.
      const skipped = step.startsWith('skip:') ? step.slice(5) : null;
      w.steps = w.steps.map((s) =>
        s.step === skipped
          ? { ...s, status: 'skipped', detail: 'skipped by user', updated_at: now }
          : s.step === step || (s.status === 'pending' && w.state.kind === 'failed')
            ? { ...s, status: 'done', detail: null, updated_at: now }
            : s,
      );
      w.state = { kind: w.pr_url ? 'pr_open' : 'active' };
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_create_pr: ({ id, draft }) => {
      const w = work(id);
      w.pr_url = w.pr_url ?? `https://github.com/acme/mock/pull/${100 + state.work.indexOf(w)}`;
      w.state = { kind: 'pr_open' };
      w.review_due = false; // a UI Ship counts as review
      w.pr_title_needs_key = false;
      void draft;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_pr_draft: ({ id }) => {
      const w = work(id);
      const t = w.ticket && state.tickets.find((x) => sameRef(x.ticket.ref, w.ticket!))?.ticket;
      const pr = (state.settings.value as { work?: { pr?: { draft?: boolean } } }).work?.pr;
      return {
        title: t ? `${t.ref.key}: ${t.title}` : (w.title ?? w.branch),
        body: t ? `${t.url}` : '',
        draft: pr?.draft ?? false,
      };
    },
    work_finish_merged: ({ ids }) => {
      const report: FinishMergedReport = { finished: [], skipped: [] };
      for (const w of state.work.filter((x) => x.state.kind === 'merged' && ids.includes(x.id))) {
        const why = w.state.kind === 'merged' ? w.state.detail : null;
        if (why || dirtyWork(w)) {
          report.skipped.push({ id: w.id, reason: why ?? `${w.worktree} has uncommitted changes` });
        } else {
          report.finished.push(handlers.work_finish({ id: w.id, opts: FINISH_MERGED }) as WorkItem);
        }
      }
      return report;
    },
    work_check_prs: () => null,
    work_finish: ({ id, opts }) => {
      const w = work(id);
      if (dirtyWork(w) && !opts.force) {
        throw err('dirty', 'worktree has uncommitted changes', {
          files: ['src/invoice.rs', 'tests/rounding.rs'],
        });
      }
      w.state = { kind: 'finished' };
      w.review_due = false;
      w.claude_replied = false;
      for (const sid of w.session_ids) {
        if (state.sessions.some((s) => s.id === sid)) {
          state.sessions = state.sessions.filter((s) => s.id !== sid);
          emit({ type: 'session.removed', id: sid });
        }
      }
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_link: ({ id, ticket, apply_side_effects }) => {
      const w = work(id);
      if (w.kind !== 'branch')
        throw err('conflict', w.review ? 'Review checkout: read-only' : 'work item already has a ticket');
      const t = ticketItem(ticket);
      w.kind = 'ticket';
      w.ticket = clone(t.ticket.ref);
      w.pr_title_needs_key = w.pr_url !== null;
      if (apply_side_effects)
        t.ticket.status = {
          ...t.ticket.status,
          name: w.pr_url ? 'In Review' : 'In Progress',
          category: 'in_progress',
        };
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_create_ticket: ({ id, view_id, title, apply_side_effects }) => {
      const w = work(id);
      if (w.kind !== 'branch') throw err('conflict', 'work item already has a ticket');
      const views = state.projects.find((p) => p.id === w.project_id)?.tracker?.views ?? [];
      if (!views.some((v) => v.id === view_id)) throw err('not_found', `tracker source ${view_id}`);
      if (!title.trim()) throw err('invalid_argument', 'the ticket needs a title');
      const base = clone(state.tickets[0]!);
      const key = `SHOP-${900 + state.tickets.length}`;
      base.ticket = {
        ...base.ticket,
        ref: { ...base.ticket.ref, key, id: key },
        title: title.trim(),
        assignee: null,
      };
      base.project_ids = [w.project_id];
      base.work_item_id = null;
      state.tickets.push(base);
      return handlers.work_link({ id, ticket: base.ticket.ref, apply_side_effects });
    },
    work_status: ({ id }) => gitOf(work(id)),
    work_status_all: () =>
      Object.fromEntries(state.work.filter((w) => w.state.kind !== 'finished').map((w) => [w.id, gitOf(w)])),
    work_diff: ({ id }) => {
      const w = work(id);
      const s = spawn(w.project_id, { type: 'editor', adapter: 'nvim' }, 'diff', w.worktree);
      return updateSession(session(s.id), { work_item_id: w.id });
    },
    work_send: ({ id, prompt, files, threads, feedback_at }) => {
      const w = work(id);
      if (!prompt.trim()) throw err('invalid_argument', 'the prompt is empty');
      const claude = state.sessions.find((s) => w.session_ids.includes(s.id) && s.kind.type === 'claude');
      void files;
      // Never refused: held while Claude works, copied when Kelta cannot tell it is idle.
      const live = claude?.lifecycle === 'live';
      if (live && claude.status_source !== 'hook') {
        const text =
          "This Claude isn't connected to Kelta: the feedback is copied, paste it when Claude is ready.";
        emit({ type: 'toast', toast: { level: 'info', text, action: null } });
      } else if (live && (claude.status === 'working' || claude.status === 'needs_input')) {
        const text = 'Claude is working: it gets your message when it stops.';
        emit({ type: 'toast', toast: { level: 'info', text, action: null } });
      } else if (claude) {
        updateSession(claude, { lifecycle: 'live', status: 'working', status_source: 'hook' });
      }
      if (threads) w.sent_threads = [...threads];
      if (feedback_at && !(Date.parse(w.feedback_at ?? '') >= Date.parse(feedback_at)))
        w.feedback_at = feedback_at;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_feedback: ({ id }) => {
      if (!work(id).pr_url) throw err('invalid_argument', 'this work item has no pull request');
      return clone(samples.feedback);
    },
    work_rerequest_review: ({ id }) => {
      work(id);
      return clone(samples.feedback.reviewers);
    },
    work_resolve_sent_threads: ({ id }) => {
      const w = work(id);
      if (!w.sent_threads?.length) throw err('invalid_argument', 'no review threads were sent to Claude');
      w.sent_threads = [];
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_rebase: ({ id, op }) => {
      const w = work(id);
      const claude = state.sessions.find((s) => w.session_ids.includes(s.id) && s.kind.type === 'claude');
      const busy =
        claude?.lifecycle === 'live' && (claude.status === 'working' || claude.status === 'needs_input');
      if (busy && op.kind !== 'abort') {
        throw err('conflict', 'Claude is working in this worktree. Rebase when it stops.', {
          reason: 'claude_busy',
        });
      }
      const sha = (n: number): string => `${'abcdef0123456789'.repeat(3)}${n}`.slice(0, 40);
      if (op.kind === 'start' && op.onto === 'remote_branch') {
        delete state.remoteNew[id];
        w.rebase = null;
      } else if (op.kind === 'start') {
        // A pushed item (with a PR) stops on a conflict; an unpushed one rebases cleanly.
        w.rebase = w.pr_url
          ? {
              onto: `origin/${w.base}`,
              pre_head: sha(1),
              remote_sha: sha(1),
              conflicts: ['src/output.rs'],
              step: 1,
              total: 2,
            }
          : null;
      } else if (!w.rebase || w.rebase.total === 0) {
        throw err('conflict', 'No rebase is in progress.');
      } else if (op.kind === 'continue') {
        w.rebase = { ...w.rebase, conflicts: [], step: 0, total: 0 };
      } else {
        w.rebase = null;
      }
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_push: ({ id, force }) => {
      const w = work(id);
      if (force && !(w.rebase && w.rebase.total === 0)) {
        throw err('conflict', 'Force push is only offered to rewrite your own rebased commits.', {
          reason: 'not_diverged',
        });
      }
      w.rebase = null;
      w.pr_title_needs_key = false; // the backend renamed the PR with the ticket key
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_mark_reviewed: ({ id }) => {
      const w = work(id);
      if (w.review_due || w.delta) {
        w.review_due = false;
        w.delta = null;
        emit({ type: 'work.updated', work: clone(w) });
      }
      return clone(w);
    },
    work_set_note: ({ id, note }) => {
      const w = work(id);
      w.next_note = note?.trim() || null;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_left: ({ id }) => {
      const w = work(id);
      w.left_at = new Date().toISOString();
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_start_now: ({ id }) => {
      const w = work(id);
      if (w.state.kind !== 'queued') throw err('conflict', 'work item is not queued');
      w.state = { kind: 'active' };
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_queue_front: ({ id }) => {
      const w = work(id);
      if (w.state.kind !== 'queued') throw err('conflict', 'work item is not queued');
      const pos = Math.min(...state.work.map((x) => (x.state.kind === 'queued' ? x.state.pos : Infinity)));
      w.state = { kind: 'queued', pos: pos - 1 };
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_park: ({ id }) => {
      const w = work(id);
      const mine = state.sessions.filter(
        (s) =>
          s.work_item_id === id &&
          s.lifecycle === 'live' &&
          (s.kind.type === 'claude' || s.kind.type === 'editor'),
      );
      if (mine.some((s) => s.kind.type === 'claude' && s.status === 'working'))
        throw err('conflict', 'Claude is working');
      if (mine.length === 0) throw err('conflict', 'nothing to park');
      for (const s of mine)
        updateSession(s, {
          lifecycle: 'dormant',
          status: 'unknown',
          attention: 'none',
          seen: true,
          pid: null,
        });
      Object.assign(w, { parked_at: new Date().toISOString(), nvim_kept: 0 });
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_notes: ({ id }) => notesOf(id),
    work_note_resolve: ({ id, note }) => {
      const n = (state.notes[id] ?? []).find((x) => x.id === note);
      if (!n) throw err('not_found', `note ${note}`);
      n.state = 'resolved';
      emit({ type: 'work.updated', work: clone(work(id)) });
      return notesOf(id);
    },
    work_notes_send: ({ id }) => {
      const open = (state.notes[id] ?? []).filter((n) => n.state === 'open' || n.state === 'untouched');
      if (open.length === 0) throw err('invalid_argument', 'no open review notes');
      const now = new Date().toISOString();
      for (const n of open) Object.assign(n, { state: 'sent', sent_at: now });
      emit({ type: 'work.updated', work: clone(work(id)) });
      return notesOf(id);
    },
    work_arm_merge: ({ id }) => {
      const w = work(id);
      w.auto_finish = true;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_disarm_merge: ({ id }) => {
      const w = work(id);
      w.auto_finish = false;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    editor_open: () => null,
    editor_send_selection: ({ editor_session, claude_session }) => {
      session(editor_session);
      session(claude_session);
      return null;
    },
    editor_quickfix: () => null,
    fs_exists: ({ paths }) => paths.map((p) => p.startsWith('/')),
    // ---- tools / plugins / triggers --------------------------------------------------------
    tool_list: ({ project_id }) => {
      project(project_id);
      return clone(state.tools);
    },
    tool_check: ({ tool_id }) => {
      const t = state.tools.find((x) => x.id === tool_id);
      if (!t) throw err('not_found', `tool ${tool_id} not found`);
      return t.installed === false
        ? { installed: false, version: null, install_hint: `brew install ${tool_id}` }
        : clone(samples.toolCheck);
    },
    tool_open: ({ project_id, tool_id, placement, ctx }) => {
      const t = state.tools.find((x) => x.id === tool_id);
      if (!t) throw err('not_found', `tool ${tool_id} not found`);
      if (t.kind === 'web') {
        const instance_id = uuid();
        const next = openPane(layoutOf(project_id), {
          content: { kind: 'web', tool_instance_id: instance_id },
          placement,
          focus: true,
          tab_title: t.label,
          work_item_id: null,
        }).layout;
        backendLayoutChange(project_id, next);
        return { kind: 'web', instance_id, url: 'http://127.0.0.1:3011/', embed: 'auto' };
      }
      const s = spawn(project_id, { type: 'tool', tool_id }, t.label, null);
      if (ctx.work_item_id) updateSession(session(s.id), { work_item_id: ctx.work_item_id });
      const next = openPane(layoutOf(project_id), {
        content: { kind: 'terminal', session_id: s.id },
        placement,
        focus: true,
        tab_title: t.label,
        work_item_id: ctx.work_item_id,
      }).layout;
      backendLayoutChange(project_id, next);
      return { kind: 'pty', session_id: s.id };
    },
    tool_close: () => null,
    plugin_list: () => clone(state.plugins),
    plugin_inspect: () => clone(samples.pluginInstallPreview),
    plugin_install: ({ grant }) => {
      const preview = samples.pluginInstallPreview;
      const info = {
        ...clone(samples.pluginInfo),
        id: preview.manifest.id,
        name: preview.manifest.name,
        version: preview.manifest.version,
        granted: grant,
      };
      state.plugins = [...state.plugins.filter((p) => p.id !== info.id), info];
      return clone(info);
    },
    plugin_uninstall: ({ id }) => {
      state.plugins = state.plugins.filter((p) => p.id !== id);
      return null;
    },
    plugin_enable: ({ id, enabled }) => {
      const p = state.plugins.find((x) => x.id === id);
      if (!p) throw err('not_found', `plugin ${id} not found`);
      p.enabled = enabled;
      return null;
    },
    plugin_grant: ({ id, permissions }) => {
      const p = state.plugins.find((x) => x.id === id);
      if (!p) throw err('not_found', `plugin ${id} not found`);
      p.granted = permissions;
      return clone(p);
    },
    plugin_screen_open: ({ plugin_id, screen_id }) => {
      const instance_id = uuid();
      screens.set(instance_id, plugin_id);
      return { instance_id, url: `kelta-plugin://${plugin_id}/${screen_id}.html` };
    },
    plugin_screen_close: ({ instance_id }) => {
      screens.delete(instance_id);
      return null;
    },
    plugin_call: ({ instance_id, method }) => {
      const pluginId = screens.get(instance_id);
      if (!pluginId) throw err('not_found', `screen instance ${instance_id} not found`);
      const plugin = state.plugins.find((p) => p.id === pluginId);
      const granted = new Set(plugin?.granted ?? []);
      const needs: Record<string, string> = {
        'projects.list': 'projects.read',
        'projects.current': 'projects.read',
        'tickets.list': 'tickets.read',
        'sessions.list': 'sessions.read',
      };
      const required = needs[method];
      if (required && !granted.has(required)) {
        throw err('permission_denied', `missing permission ${required}`, { permission: required });
      }
      switch (method) {
        case 'app.info':
          return { version: samples.appInfo.version, platform: samples.appInfo.platform, theme: 'dark' };
        case 'projects.list':
          return clone(state.projects) as unknown as JsonValue;
        case 'sessions.list':
          return clone(state.sessions) as unknown as JsonValue;
        default:
          return null;
      }
    },
    command_run: () => null,
    trigger_list: () => clone(samples.triggerInfos),
    trigger_test: ({ trigger_id }) => ({
      ...clone(
        samples.triggerRuns[0] ?? { ts: '', trigger_id, event: 'test', ok: true, detail: null, depth: 0 },
      ),
      trigger_id,
    }),
    trigger_log: ({ limit }) => clone(samples.triggerRuns).slice(0, limit),
  };

  const transport: IpcTransport = {
    kind: 'mock',
    async invoke<T>(
      cmd: string,
      args?: Record<string, unknown> | Uint8Array,
      opts?: { headers?: Record<string, string> },
    ) {
      // Raw binary body (session_write): bytes + x-kelta-session-id header. `ArrayBuffer.isView`
      // also accepts Uint8Arrays from another realm (jsdom).
      const raw = args !== undefined && ArrayBuffer.isView(args);
      const callArgs: Record<string, unknown> | undefined = raw
        ? {
            id: opts?.headers?.['x-kelta-session-id'] ?? '',
            data: new Uint8Array(args.buffer, args.byteOffset, args.byteLength),
          }
        : (args as Record<string, unknown> | undefined);
      calls.push({ cmd, args: callArgs });
      if (options.latencyMs) {
        // one-shot: simulated IPC latency for this call
        await new Promise((resolve) => setTimeout(resolve, options.latencyMs));
      } else {
        await Promise.resolve();
      }
      const hold = held.get(cmd);
      if (hold) {
        held.delete(cmd);
        const parked = await hold;
        if (parked) throw clone(parked);
      }
      const failure = failOnce.get(cmd) ?? failAlways.get(cmd);
      if (failure) {
        failOnce.delete(cmd);
        throw clone(failure);
      }
      const handler = (handlers as unknown as Record<string, (a: unknown) => unknown>)[cmd];
      if (!handler) throw err('unsupported', `not implemented: ${cmd}`);
      return (await handler(callArgs ?? {})) as T;
    },
    channel<T>(onmessage: (message: T) => void): IpcChannel<T> {
      const ch: IpcChannel<T> = { handle: null, onmessage };
      (ch as { handle: unknown }).handle = ch;
      return ch;
    },
  };

  const toError = (e: Partial<KeltaError> & { code: KeltaError['code'] }): KeltaError => ({
    code: e.code,
    message: e.message ?? `mock failure (${e.code})`,
    detail: e.detail ?? null,
    retry_after_ms: e.retry_after_ms ?? null,
  });

  const controls: MockControls = {
    calls,
    failNext: (cmd, e) => void failOnce.set(cmd, toError(e)),
    failAlways: (cmd, e) => void failAlways.set(cmd, toError(e)),
    holdNext: (cmd) => {
      let release!: (e: KeltaError | null) => void;
      held.set(cmd, new Promise((resolve) => (release = resolve)));
      return (e) => release(e ? toError(e) : null);
    },
    clearFailures: () => {
      failOnce.clear();
      failAlways.clear();
    },
    emit,
    output: (sessionId, text) => sendFrame(sessionId, FRAME_DATA, new TextEncoder().encode(text)),
    reset: () => {
      state = freshState();
      try {
        sessionStorage.removeItem(GLOBAL_LAYOUT_KEY);
      } catch {
        // no storage
      }
      calls.length = 0;
      attached.clear();
    },
    get state() {
      return state;
    },
  };

  return { transport, controls };
}

/** Sessions that appear in a layout but are missing from the session list (fixture sanity). */
export function danglingSessions(state: Pick<MockState, 'layouts' | 'sessions'>): string[] {
  const ids = new Set(state.sessions.map((s) => s.id));
  return Object.values(state.layouts)
    .flatMap((l) => layoutSessions(l))
    .filter((id) => !ids.has(id));
}
