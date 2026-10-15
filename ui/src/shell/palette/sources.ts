// Palette items (SPEC §1): actions, projects, sessions across projects, tools, plugin commands,
// settings sections and keys and pull requests are built synchronously from the stores; tickets come
// from `tracker_search`. Every item carries its own `run`.

import { msg, t } from '$lib/i18n/index.svelte';
import { dispatch, hasAction } from '$lib/actions';
import { settingsSections } from '$app/registry';
import type { CommandDef, ProjectId, SessionInfo, TemplateCtx, TicketItem } from '$lib/gen';
import { ACTIONS } from '$lib/gen/actions';
import { effectiveChords } from '$lib/keys/manager';
import { paneSession } from '$lib/layout';
import { trackerAssign } from '$lib/ipc/commands';
import { plugins, projects, reviews, sessions, settings, tickets, toasts, ui, work } from '$lib/stores';
import { attentionRank, lampOf, type LampLevel } from '$lib/stores/reducers';
import { currentPlatform, isIconName } from '$lib/ui';

import { reviewCtx, ticketCtx, workCtx } from '../../views/plugins/actions';
import { helpOf, keywordsOf, labelOf, leafFields, type SchemaNode } from '../../views/settings/lib/schema';
import { SECTION_ROOTS } from '../../views/settings/lib/sections';
import { configWorkflows, pickWorkflow, runWorkflowOn } from '../../views/tickets/workflow.svelte';
import { focusedWorkItem } from '../../views/work/actions';
import { phaseNow, sessionLabel, unfinishedWork, workTitle } from '../../views/work/live';
import { goToWork } from '../../views/work/nav';
import { startAnotherPr, startWorkOnTicket, ticketWork } from '../../views/work/startWork';
import { workKey } from '../../views/work/phase';
import { recentPlaces } from '../jumplist';
import { sessionIcon, sessionKindName, statusLabel, toolIcon } from '../labels';
import { activateProject, focusedPane, openContent, railTools, revealSession } from '../nav';

export type PaletteGroup =
  | 'Recent'
  | 'Work items'
  | 'Sessions'
  | 'Projects'
  | 'Actions'
  | 'Tools'
  | 'Commands'
  | 'Tickets'
  | 'Pull requests'
  | 'Settings';

export interface PaletteItem {
  id: string;
  group: PaletteGroup;
  label: string;
  detail?: string;
  /** Extra words matched by the query, not shown. */
  keywords?: string;
  kbd?: string;
  icon: string;
  lamp?: LampLevel;
  run: () => void | Promise<void>;
}

/** Display order of the groups when the query is empty (headers are translated where shown). */
export const GROUP_ORDER: readonly PaletteGroup[] = [
  msg('Recent'),
  msg('Work items'),
  msg('Sessions'),
  msg('Projects'),
  msg('Actions'),
  msg('Tools'),
  msg('Commands'),
  msg('Tickets'),
  msg('Pull requests'),
  msg('Settings'),
];

/** Query prefixes that narrow the palette to their groups (`# 311`, `!311`, `>split`, `,wip`). */
const PREFIXES: Record<string, readonly PaletteGroup[]> = {
  '#': ['Tickets'],
  '!': ['Pull requests'],
  '>': ['Actions', 'Commands', 'Tools'],
  ',': ['Settings'],
};

/** The groups a query's prefix narrows to (null: every group) and the text left to match. */
export function parseQuery(query: string): { groups: readonly PaletteGroup[] | null; text: string } {
  const q = query.trimStart();
  const groups = PREFIXES[q[0] ?? ''];
  return groups ? { groups, text: q.slice(1).trim() } : { groups: null, text: q.trim() };
}

/** Text matched against the query. */
export function itemText(item: PaletteItem): string {
  return `${item.label} ${item.detail ?? ''} ${item.keywords ?? ''}`;
}

/** Detail segments are separated by space, not middle dots (DESIGN §3). */
const SEP = '\u2002\u2002';

function projectName(id: ProjectId): string {
  return projects.byId(id)?.name ?? id;
}

function sessionDetail(s: SessionInfo): string {
  const kind = sessionKindName(s.kind);
  // The kind only adds information when the session was renamed ("claude" titled "claude" says it twice).
  return [projectName(s.project_id), kind === s.name ? null : kind, statusLabel(s.status)]
    .filter(Boolean)
    .join(SEP);
}

export function sessionItems(): PaletteItem[] {
  return [...sessions.all]
    .sort(
      (a, b) =>
        attentionRank(b.attention) - attentionRank(a.attention) || a.created_at.localeCompare(b.created_at),
    )
    .map((s) => ({
      id: `session:${s.id}`,
      group: 'Sessions' as const,
      label: sessionLabel(s),
      detail: sessionDetail(s),
      icon: sessionIcon(s.kind),
      lamp: lampOf(s.attention, s.status === 'working'),
      run: () => void revealSession(s.id),
    }));
}

/** Every unfinished work item: `[lamp] KEY title`, the phase as meta; matches key, title, branch. */
export function workItems(): PaletteItem[] {
  return unfinishedWork().map((w) => {
    const phase = phaseNow(w);
    return {
      id: `work:${w.id}`,
      group: 'Work items' as const,
      label: `${workKey(w)} ${workTitle(w)}`,
      detail: [phase.label, w.branch].join(SEP),
      icon: 'git-branch',
      lamp: phase.lamp === 'none' ? undefined : phase.lamp,
      // work_resume recreates a closed tab and resumes Claude.
      run: () => void goToWork(w),
    };
  });
}

export function projectItems(): PaletteItem[] {
  return projects.list.map((p) => ({
    id: `project:${p.id}`,
    group: 'Projects' as const,
    label: p.name,
    detail: p.builtin ? t('Home') : p.open ? t('open project') : t('closed project'),
    icon: p.builtin ? 'house' : 'folder',
    run: () => void activateProject(p.id),
  }));
}

export function actionItems(): PaletteItem[] {
  const keys = settings.value()?.keys ?? null;
  const platform = currentPlatform();
  return ACTIONS.filter(
    (a) =>
      a.context !== 'external' &&
      !a.id.startsWith('project.goto.') &&
      !a.id.startsWith('tool.goto.') &&
      hasAction(a.id),
  ).map((a) => ({
    id: `action:${a.id}`,
    group: 'Actions' as const,
    label: t(a.label),
    kbd: effectiveChords(a.id, keys, platform)[0],
    icon: 'command',
    run: () => void dispatch(a.id),
  }));
}

export function toolItems(): PaletteItem[] {
  return railTools().flatMap((tool) => {
    const mark = toolIcon(tool);
    const item = {
      id: `tool:${tool.id}`,
      group: 'Tools' as const,
      label: t('Open {name}', { name: tool.label }),
      detail: tool.installed === false ? t('not installed') : (tool.description ?? undefined),
      kbd: tool.keybinding ?? undefined,
      icon: isIconName(mark) ? mark : 'wrench',
      run: () => void dispatch('tools.open', { tool_id: tool.id }),
    };
    if (!tool.needs_project) return [item];
    const forProject = {
      ...item,
      id: `tool-for:${tool.id}`,
      label: t('Open {name} for project…', { name: tool.label }),
      kbd: undefined,
      run: () => void dispatch('tools.open', { tool_id: tool.id, pick_project: true }),
    };
    return [item, forProject];
  });
}

/**
 * The ctx a command runs with, `null` when it does not apply. Ticket/review/work commands need their
 * subject: a ticket/review detail pane or the focused work item; none on lists or in a global view
 * (the focused pane would be the hidden project's).
 */
function commandCtx(cmd: CommandDef): Partial<TemplateCtx> | null {
  if (!cmd.enabled) return null;
  const pane = focusedPane();
  const content = ui.view ? null : pane?.content;
  switch (cmd.when) {
    case 'terminal':
      return pane !== null && paneSession(pane) !== null ? {} : null;
    case 'ticket':
      if (content?.kind !== 'ticket_detail') return null;
      return ticketCtx(content.ticket, work.forTicket(content.ticket)?.id ?? null, [], projects.activeId);
    case 'review':
      return content?.kind === 'review_detail' ? reviewCtx(content.review, projects.activeId) : null;
    case 'work': {
      const w = focusedWorkItem();
      return w ? workCtx(w) : null;
    }
    default:
      return {};
  }
}

/** Commands from config and enabled plugins (`plugin.command.<id>` handled by L8). */
export function commandItems(): PaletteItem[] {
  const out: PaletteItem[] = [];
  const add = (cmd: CommandDef, origin: string): void => {
    const ctx = commandCtx(cmd);
    if (!ctx) return;
    out.push({
      id: `command:${origin}:${cmd.id}`,
      group: 'Commands',
      label: cmd.title,
      detail: origin,
      kbd: cmd.keybinding ?? undefined,
      icon: 'zap',
      run: () => void dispatch(`plugin.command.${cmd.id}`, { ctx }),
    });
  };
  for (const cmd of settings.value()?.commands ?? []) add(cmd, 'config');
  for (const p of plugins.plugins.data ?? []) {
    if (!p.enabled) continue;
    for (const cmd of p.contributes.commands) add(cmd, p.name);
    for (const kind of ['ticket', 'review', 'work'] as const) {
      for (const b of p.contributes[`${kind}_actions`]) {
        add(
          { id: `${p.id}/${b.id}`, title: b.title, do: b.do, when: kind, keybinding: null, enabled: true },
          p.name,
        );
      }
    }
    for (const s of p.contributes.screens) {
      // `<plugin>/open:<screen>` is generated by the backend; global screens open in Home.
      const project = s.scope === 'global' ? (projects.home?.id ?? 'home') : projects.activeId;
      out.push({
        id: `command:${p.name}:${p.id}/open:${s.id}`,
        group: 'Commands',
        label: t('Open {name}', { name: s.title }),
        detail: p.name,
        icon: 'zap',
        run: () =>
          void dispatch(`plugin.command.${p.id}/open:${s.id}`, {
            ctx: project ? { extra: { project_id: project } } : {},
          }),
      });
    }
  }
  return out;
}

/** `[[workflows]]` with `on = "global"` (#355): a run with no ticket, in the active project. */
export function workflowItems(): PaletteItem[] {
  const project = projects.activeId;
  if (!project) return [];
  return configWorkflows('global').map((w) => ({
    id: `workflow:${w.name}`,
    group: 'Commands' as const,
    label: t('Run {workflow}', { workflow: w.label }),
    detail: w.description || undefined,
    icon: 'play',
    run: () => void runWorkflowOn(project, w, null),
  }));
}

/** Every section, then every key of the loaded schema (label and help as in Settings). */
export function settingsItems(): PaletteItem[] {
  const linux = currentPlatform() === 'linux';
  const sections = settingsSections.filter((s) => !s.platform || (s.platform === 'linux') === linux);
  const schema = settings.schema.data as unknown as SchemaNode | null;
  const keys = schema
    ? sections.flatMap((s) =>
        leafFields(schema, SECTION_ROOTS[s.id] ?? []).map((f) => ({
          id: `setting-key:${f.path}`,
          group: 'Settings' as const,
          label: labelOf(f.path),
          detail: [f.path, helpOf(f.path, f.node)].filter(Boolean).join(SEP),
          keywords: keywordsOf(f.path).join(' '),
          icon: s.icon,
          run: () => void dispatch('settings.open', { section: s.id, key: f.path }),
        })),
      )
    : [];
  return [
    ...sections.map((s) => ({
      id: `setting:${s.id}`,
      group: 'Settings' as const,
      label: t('Settings: {section}', { section: t(s.label) }),
      icon: s.icon,
      run: () => void dispatch('settings.open', { section: s.id }),
    })),
    ...keys,
  ];
}

export function reviewItems(): PaletteItem[] {
  const seen: string[] = [];
  const out: PaletteItem[] = [];
  for (const list of Object.values(reviews.lists)) {
    for (const it of list.data?.items ?? []) {
      const r = it.review;
      const key = `${r.ref.account}:${r.ref.repo}#${r.ref.number}`;
      if (seen.includes(key)) continue;
      seen.push(key);
      out.push({
        id: `review:${key}`,
        group: 'Pull requests',
        label: `${r.ref.repo}#${r.ref.number} ${r.title}`,
        detail: r.author.name,
        icon: 'git-pull-request',
        run: () => {
          const project = it.project_ids[0] ?? projects.activeId;
          if (project) {
            void openContent(project, {
              content: { kind: 'review_detail', review: r.ref },
              placement: 'new_tab',
            });
          }
        },
      });
    }
  }
  return out;
}

/** Verbs are offered on the best hits only: 4 rows for each of 20 hits would bury everything else. */
const VERB_HITS = 3;

function openTicket(hit: TicketItem): void {
  const project = hit.project_ids[0] ?? projects.activeId;
  if (project)
    void openContent(project, {
      content: { kind: 'ticket_detail', ticket: hit.ticket.ref },
      placement: 'new_tab',
    });
}

function ticketVerbs(hit: TicketItem): PaletteItem[] {
  const { ref, url } = hit.ticket;
  const id = `ticket:${ref.account}:${ref.key}`;
  const project = hit.project_ids[0] ?? projects.activeId;
  const detail = hit.ticket.title;
  const verb = (name: string, label: string, icon: string, run: () => void | Promise<void>): PaletteItem => ({
    id: `${id}:${name}`,
    group: 'Tickets',
    label,
    detail,
    icon,
    run,
  });
  return [
    verb('start', t('Start work on {key}', { key: ref.key }), 'play', () => void startWorkOnTicket(ref)),
    ...(ticketWork(ref).length > 0
      ? [
          verb(
            'another',
            t('Start another PR on {key}', { key: ref.key }),
            'plus',
            () => void startAnotherPr(ref),
          ),
        ]
      : []),
    verb('workflow', t('Run workflow on {key}…', { key: ref.key }), 'play', () => void pickWorkflow(ref)),
    verb('move', t('Move {key} to…', { key: ref.key }), 'arrow-right', () =>
      ui.openSheet('tickets.move', { ticket: hit.ticket, project_id: project }),
    ),
    // A tracker that cannot assign from Kelta (caps) offers no assign verb.
    ...(hit.caps.assign
      ? [
          verb('assign', t('Assign {key} to me', { key: ref.key }), 'user', async () => {
            try {
              tickets.patch(await trackerAssign({ ticket: ref, assignee: { kind: 'me' } }));
              toasts.info(t('{key} assigned to you', { key: ref.key }));
            } catch (err) {
              toasts.error(err, t('Assigning {key}', { key: ref.key }));
            }
          }),
        ]
      : []),
    verb(
      'browser',
      t('Open {key} in browser', { key: ref.key }),
      'external-link',
      () => void dispatch('tickets.open_in_browser', { url }),
    ),
  ];
}

export function ticketItems(hits: readonly TicketItem[]): PaletteItem[] {
  return hits.flatMap((hit, i) => [
    {
      id: `ticket:${hit.ticket.ref.account}:${hit.ticket.ref.key}`,
      group: 'Tickets' as const,
      label: `${hit.ticket.ref.key} ${hit.ticket.title}`,
      detail: [hit.ticket.status.name, hit.project_ids.map(projectName).join(', ')].filter(Boolean).join(SEP),
      icon: 'ticket',
      run: () => openTicket(hit),
    },
    ...(i < VERB_HITS ? ticketVerbs(hit) : []),
  ]);
}

/**
 * The last places visited (jumplist.ts), most recent first, as the items of `base` with the same
 * id; a ticket or pull request no list holds reopens its pane, a finished work item drops out.
 */
export function recentItems(base: readonly PaletteItem[]): PaletteItem[] {
  return recentPlaces().flatMap((r): PaletteItem[] => {
    const item = base.find((i) => i.id === r.id);
    if (item) return [{ ...item, id: `recent:${r.id}`, group: 'Recent' }];
    const { project, content } = r;
    if (!project || (content?.kind !== 'ticket_detail' && content?.kind !== 'review_detail')) return [];
    return [
      {
        id: `recent:${r.id}`,
        group: 'Recent',
        label: r.label,
        icon: content.kind === 'ticket_detail' ? 'ticket' : 'git-pull-request',
        run: () => void openContent(project, { content, placement: 'new_tab' }),
      },
    ];
  });
}

/** Every synchronous item, in display order. */
export function buildItems(): PaletteItem[] {
  return [
    ...workItems(),
    ...sessionItems(),
    ...projectItems(),
    ...actionItems(),
    ...toolItems(),
    ...commandItems(),
    ...workflowItems(),
    ...reviewItems(),
    ...settingsItems(),
  ];
}
