// In-memory IPC mock (VITE_IPC=mock): implements every command of ARCHITECTURE §6 over the JSON
// fixtures in ./mock/fixtures plus the typed kelta-proto samples. Used by `pnpm dev:mock`, the
// Playwright e2e suites and vitest. Never part of the production bundle (dynamic import in main.ts).
//
// Test hooks: `createMockTransport()` returns `{ transport, controls }`; in the browser the active
// controls are exposed as `window.__keltaMock` (fail the next call, emit events, read the call log).

import * as samples from '$lib/gen/fixtures';
import type {
  Column,
  EffectiveSettings,
  JsonValue,
  KeltaError,
  Layer,
  Layout,
  ProjectInfo,
  ReviewDetail,
  ReviewItem,
  ScreenInstanceId,
  SessionInfo,
  SessionKind,
  StatusCategory,
  Ticket,
  TicketDetail,
  TicketItem,
  TicketRef,
  ToolInfo,
  Transition,
  UiEvent,
  WorkItem,
  WorkStepStatus,
} from '$lib/gen';
import { FRAME_DATA, FRAME_EXIT, FRAME_SNAPSHOT } from '$lib/gen/constants';
import { emptyLayout, layoutSessions, openPane } from '$lib/layout';

import type { CommandArgs, CommandName, CommandResult } from './commands';
import layoutsJson from './mock/fixtures/layouts.json';
import projectsJson from './mock/fixtures/projects.json';
import reviewsJson from './mock/fixtures/reviews.json';
import sessionsJson from './mock/fixtures/sessions.json';
import ticketsJson from './mock/fixtures/tickets.json';
import toolsJson from './mock/fixtures/tools.json';
import workJson from './mock/fixtures/work_items.json';
import type { IpcChannel, IpcTransport } from './transport';

// JSON imports widen string-literal unions; the fixtures are validated by mock.test.ts.
const FIXTURES = {
  projects: projectsJson as unknown as ProjectInfo[],
  sessions: sessionsJson as unknown as SessionInfo[],
  layouts: layoutsJson as unknown as Record<string, Layout>,
  tickets: ticketsJson as unknown as TicketItem[],
  reviews: reviewsJson as unknown as ReviewItem[],
  work: workJson as unknown as WorkItem[],
  tools: toolsJson as unknown as ToolInfo[],
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
  tools: ToolInfo[];
  settings: EffectiveSettings;
  approved: Set<string>;
  comments: Record<string, string[]>;
  plugins: (typeof samples.pluginInfo)[];
}

interface MockOptions {
  /** Artificial latency per call in ms (one-shot timer per call). Default 0. */
  latencyMs?: number;
}

type Handler<K extends CommandName> = (args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>>;
type Handlers = { [K in CommandName]: Handler<K> };

const clone = <T>(v: T): T => structuredClone(v);

function err(code: KeltaError['code'], message: string, detail: JsonValue | null = null): KeltaError {
  return { code, message, detail, retry_after_ms: null };
}

const CATEGORY_NAMES: Record<StatusCategory, string> = {
  todo: 'To do',
  in_progress: 'In progress',
  in_review: 'In review',
  done: 'Done',
  unknown: 'Unknown',
};

function freshState(): MockState {
  return {
    projects: clone(FIXTURES.projects),
    sessions: clone(FIXTURES.sessions),
    layouts: clone(FIXTURES.layouts),
    tickets: clone(FIXTURES.tickets),
    reviews: clone(FIXTURES.reviews),
    work: clone(FIXTURES.work),
    tools: clone(FIXTURES.tools),
    settings: { value: clone(samples.settingsDefault) as unknown as JsonValue, sources: {} },
    approved: new Set(),
    comments: {},
    plugins: [clone(samples.pluginInfo)],
  };
}

const refKey = (r: TicketRef): string => `${r.account}:${r.key}`;
const sameRef = (a: TicketRef, b: TicketRef): boolean => a.account === b.account && a.key === b.key;

function ticketDetailFor(t: Ticket, comments: string[]): TicketDetail {
  const body = `Ticket **${t.ref.key}** (${t.title}).\n\nMock body rendered from the in-memory fixtures.`;
  return {
    ticket: t,
    body_md: body,
    body_html: `<p>Ticket <strong>${t.ref.key}</strong> (${escapeHtml(t.title)}).</p><p>Mock body rendered from the in-memory fixtures. <a href="${t.url}">Open in tracker</a></p>`,
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
    parent: null,
  };
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

function transitionsFor(t: Ticket): Transition[] {
  const cats: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done'];
  return cats
    .filter((c) => c !== t.status.category)
    .map((category) => ({
      id: `to-${category}`,
      name: CATEGORY_NAMES[category],
      to: { id: `st-${category}`, name: CATEGORY_NAMES[category], category },
      needs_fields: category === 'done' && t.ref.account === 'jira-acme',
    }));
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

export function createMockTransport(options: MockOptions = {}): {
  transport: IpcTransport;
  controls: MockControls;
} {
  let state = freshState();
  const calls: MockCall[] = [];
  const failOnce = new Map<string, KeltaError>();
  const failAlways = new Map<string, KeltaError>();
  const eventChannels = new Set<IpcChannel<UiEvent>>();
  const attached = new Map<string, { generation: number; channel: IpcChannel<Uint8Array> }>();
  let generation = 0;
  const screens = new Map<ScreenInstanceId, string>();

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
    state.settings = {
      value: setPath(state.settings.value, path, value),
      sources: { ...state.settings.sources },
    };
    if (value === undefined) delete state.settings.sources[path];
    else state.settings.sources[path] = layer;
    emit({ type: 'settings.changed', layers: [layer], paths: [path], requires_restart: [] });
    return clone(state.settings);
  };

  const handlers: Handlers = {
    // ---- app -------------------------------------------------------------------------------
    app_info: () => clone(samples.appInfo),
    app_ready: () => null,
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
        : [{ path: '', message: 'expected `key = value`', line: 1, col: 1 }],
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
      project(project_id);
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
    session_spawn_template: ({ project_id, template_id, placement }) => {
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
          tab_title: i === 0 ? template_id : null,
          work_item_id: null,
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
    terminal_set_palette: () => null,
    // ---- tickets ---------------------------------------------------------------------------
    tracker_list: ({ scope, view_id, cursor }) => {
      const items =
        scope.kind === 'all' ? state.tickets : state.tickets.filter((t) => t.project_ids.includes(scope.id));
      const filtered =
        view_id === 'sprint'
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
      };
    },
    tracker_get: ({ ticket }) => {
      const item = ticketItem(ticket);
      return ticketDetailFor(clone(item.ticket), state.comments[refKey(ticket)] ?? []);
    },
    tracker_columns: ({ project_id }) => {
      const p = project(project_id);
      if (!p.tracker) throw err('invalid_argument', `project ${project_id} has no tracker`);
      if (p.tracker.columns.length === 0) return defaultColumns();
      return p.tracker.columns.map((c, order) => ({
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
      if (t.needs_fields && !fields) {
        throw err('needs_fields', `${t.name} requires fields`, {
          fields: [{ id: 'resolution', name: 'Resolution', required: true }],
        });
      }
      return updateTicket(ticket, t.to.category, t.to.name);
    },
    tracker_move: ({ ticket, column_id }) => {
      const item = ticketItem(ticket);
      const projectId = item.project_ids[0];
      const columns = projectId ? handlers.tracker_columns({ project_id: projectId }) : defaultColumns();
      const col = (columns as Column[]).find((c) => c.id === column_id);
      if (!col) throw err('not_found', `column ${column_id} not found`);
      if (col.category === item.ticket.status.category) return clone(item.ticket);
      return updateTicket(ticket, col.category, col.name);
    },
    tracker_comment: ({ ticket, markdown }) => {
      ticketItem(ticket);
      const key = refKey(ticket);
      state.comments[key] = [...(state.comments[key] ?? []), markdown];
      return null;
    },
    tracker_assign: ({ ticket, assignee }) => {
      const item = ticketItem(ticket);
      item.ticket.assignee =
        assignee.kind === 'none'
          ? null
          : assignee.kind === 'me'
            ? { id: 'u-ada', name: 'Ada Lovelace', login: 'ada', avatar_url: null }
            : { id: assignee.id, name: assignee.id, login: null, avatar_url: null };
      return clone(item.ticket);
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
    // ---- reviews ---------------------------------------------------------------------------
    review_list: ({ scope, kind }) => ({
      items: clone(
        state.reviews.filter(
          (r) => r.review.kind === kind && (scope.kind === 'all' || r.project_ids.includes(scope.id)),
        ),
      ),
      stale: false,
      errors: [],
    }),
    review_get: ({ review }) => {
      const item = reviewItem(review);
      const detail: ReviewDetail = {
        ...clone(samples.reviewDetail),
        review: clone(item.review),
        body_html: `<p>${escapeHtml(item.review.title)}</p><p>Mock description. <a href="${item.review.url}">View on host</a></p>`,
      };
      return detail;
    },
    review_approve: ({ review, head_sha }) => {
      const item = reviewItem(review);
      if (item.review.head_sha !== head_sha) {
        throw err('conflict', 'PR changed, refresh', { head_sha: item.review.head_sha });
      }
      item.review.my_state = 'approved';
      state.approved.add(`${review.repo}#${review.number}`);
      emit({ type: 'reviews.changed', scope: { kind: 'all' }, new_keys: [] });
      return null;
    },
    review_comment: ({ review }) => {
      const item = reviewItem(review);
      if (!item.review.my_state) item.review.my_state = 'commented';
      return null;
    },
    review_request_changes: ({ review }) => {
      reviewItem(review).review.my_state = 'changes_requested';
      return null;
    },
    // ---- work ------------------------------------------------------------------------------
    work_plan: ({ project_id, source }) => {
      const p = project(project_id);
      const plan = clone(samples.startWorkPlan);
      const repo = p.repos.find((r) => r.primary) ?? p.repos[0];
      let slug: string;
      if (source.kind === 'ticket') {
        const t = ticketItem(source.ticket);
        slug = `${t.ticket.ref.key.replace(/^#/, 'gh-')}-${t.ticket.title}`;
      } else if (source.kind === 'review') {
        slug = `pr-${source.review.number}`;
      } else {
        slug = source.name;
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
      return {
        ...plan,
        project_id,
        source,
        repo_id: repo?.id ?? plan.repo_id,
        repo_choices: p.repos.map((r) => r.id),
        branch: source.kind === 'review' ? `kelta/${slug}` : `feat/${slug}`,
        worktree_path: `${repo?.path ?? '/tmp'}.worktrees/${slug}`,
        existing: existing ? existing.id : null,
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
      };
      const spawned = handlers.session_spawn_template({
        project_id: plan.project_id,
        template_id: plan.template_id,
        ctx: {
          repo_id: plan.repo_id,
          cwd: plan.worktree_path,
          session_id: null,
          work_item_id: item.id,
          ticket: item.ticket,
          review: item.review,
          extra: {},
        },
        placement: 'new_tab',
      }) as SessionInfo[];
      item.session_ids = spawned.map((s) => s.id);
      for (const s of spawned) {
        const live = state.sessions.find((x) => x.id === s.id);
        if (live) live.work_item_id = item.id;
      }
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
      void draft;
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_finish: ({ id, opts }) => {
      const w = work(id);
      if (w.id === FIXTURES.work[1]?.id && !opts.force) {
        throw err('dirty', 'worktree has uncommitted changes', {
          files: ['src/invoice.rs', 'tests/rounding.rs'],
        });
      }
      w.state = { kind: 'finished' };
      for (const sid of w.session_ids) {
        if (state.sessions.some((s) => s.id === sid)) {
          state.sessions = state.sessions.filter((s) => s.id !== sid);
          emit({ type: 'session.removed', id: sid });
        }
      }
      emit({ type: 'work.updated', work: clone(w) });
      return clone(w);
    },
    work_status: ({ id }) => {
      const w = work(id);
      return w.state.kind === 'failed'
        ? { ahead: 0, behind: 3, dirty: true, unpushed: false }
        : clone(samples.gitStatus);
    },
    editor_open: () => null,
    editor_send_selection: ({ editor_session, claude_session }) => {
      session(editor_session);
      session(claude_session);
      return null;
    },
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
    tool_open: ({ project_id, tool_id, placement }) => {
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
      const next = openPane(layoutOf(project_id), {
        content: { kind: 'terminal', session_id: s.id },
        placement,
        focus: true,
        tab_title: t.label,
        work_item_id: null,
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
    clearFailures: () => {
      failOnce.clear();
      failAlways.clear();
    },
    emit,
    output: (sessionId, text) => sendFrame(sessionId, FRAME_DATA, new TextEncoder().encode(text)),
    reset: () => {
      state = freshState();
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
