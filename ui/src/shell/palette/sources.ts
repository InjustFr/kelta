// Palette items (SPEC §1): actions, projects, sessions across projects, tools, plugin commands,
// settings sections and reviews are built synchronously from the stores; tickets come from
// `tracker_search`. Every item carries its own `run`.

import { dispatch, hasAction } from '$lib/actions';
import { settingsSections } from '$app/registry';
import type { CommandDef, ProjectId, SessionInfo, TicketItem } from '$lib/gen';
import { ACTIONS } from '$lib/gen/actions';
import { effectiveChords } from '$lib/keys/manager';
import { paneSession } from '$lib/layout';
import { plugins, projects, reviews, sessions, settings, tools } from '$lib/stores';
import { attentionRank, lampOf, type LampLevel } from '$lib/stores/reducers';
import { currentPlatform } from '$lib/ui';

import { sessionIcon, sessionKindName, statusLabel } from '../labels';
import { activateProject, focusedPane, openContent, revealSession } from '../nav';

export type PaletteGroup =
  'Sessions' | 'Projects' | 'Actions' | 'Tools' | 'Commands' | 'Tickets' | 'Reviews' | 'Settings';

export interface PaletteItem {
  id: string;
  group: PaletteGroup;
  label: string;
  detail?: string;
  kbd?: string;
  icon: string;
  lamp?: LampLevel;
  run: () => void | Promise<void>;
}

/** Display order of the groups when the query is empty. */
export const GROUP_ORDER: readonly PaletteGroup[] = [
  'Sessions',
  'Projects',
  'Actions',
  'Tools',
  'Commands',
  'Tickets',
  'Reviews',
  'Settings',
];

/** Text matched against the query. */
export function itemText(item: PaletteItem): string {
  return `${item.label} ${item.detail ?? ''}`;
}

/** Detail segments are separated by space, not middle dots (DESIGN §3). */
const SEP = '\u2002\u2002';

function projectName(id: ProjectId): string {
  return projects.byId(id)?.name ?? id;
}

function sessionDetail(s: SessionInfo): string {
  const status = statusLabel(s.status);
  return [projectName(s.project_id), sessionKindName(s.kind), status].filter(Boolean).join(SEP);
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
      label: s.name,
      detail: sessionDetail(s),
      icon: sessionIcon(s.kind),
      lamp: lampOf(s.attention, s.status === 'working'),
      run: () => void revealSession(s.id),
    }));
}

export function projectItems(): PaletteItem[] {
  return projects.list.map((p) => ({
    id: `project:${p.id}`,
    group: 'Projects' as const,
    label: p.name,
    detail: p.builtin ? 'Home' : p.open ? 'open project' : 'closed project',
    icon: p.builtin ? 'house' : 'folder',
    run: () => void activateProject(p.id),
  }));
}

export function actionItems(): PaletteItem[] {
  const keys = settings.value()?.keys ?? null;
  const platform = currentPlatform();
  return ACTIONS.filter(
    (a) => a.context !== 'external' && !a.id.startsWith('project.goto.') && hasAction(a.id),
  ).map((a) => ({
    id: `action:${a.id}`,
    group: 'Actions' as const,
    label: a.label,
    kbd: effectiveChords(a.id, keys, platform)[0],
    icon: 'command',
    run: () => void dispatch(a.id),
  }));
}

export function toolItems(): PaletteItem[] {
  const id = projects.activeId;
  if (!id) return [];
  return tools.list(id).map((t) => ({
    id: `tool:${t.id}`,
    group: 'Tools' as const,
    label: `Open tool: ${t.label}`,
    detail: t.installed === false ? 'not installed' : (t.description ?? undefined),
    kbd: t.keybinding ?? undefined,
    icon: t.icon && t.icon.length > 0 ? t.icon : 'wrench',
    run: () => void dispatch('tools.open', { tool_id: t.id }),
  }));
}

function commandApplies(cmd: CommandDef): boolean {
  if (!cmd.enabled) return false;
  const pane = focusedPane();
  switch (cmd.when) {
    case 'terminal':
      return pane !== null && paneSession(pane) !== null;
    case 'ticket':
      return pane?.content.kind === 'tickets' || pane?.content.kind === 'ticket_detail';
    case 'review':
      return pane?.content.kind === 'reviews' || pane?.content.kind === 'review_detail';
    default:
      return true;
  }
}

/** Commands from config and enabled plugins (`plugin.command.<id>` handled by L8). */
export function commandItems(): PaletteItem[] {
  const out: PaletteItem[] = [];
  const add = (cmd: CommandDef, origin: string): void => {
    if (!commandApplies(cmd)) return;
    out.push({
      id: `command:${origin}:${cmd.id}`,
      group: 'Commands',
      label: cmd.title,
      detail: origin,
      kbd: cmd.keybinding ?? undefined,
      icon: 'zap',
      run: () => void dispatch(`plugin.command.${cmd.id}`),
    });
  };
  for (const cmd of settings.value()?.commands ?? []) add(cmd, 'config');
  for (const p of plugins.plugins.data ?? []) {
    if (!p.enabled) continue;
    for (const cmd of p.contributes.commands) add(cmd, p.name);
  }
  return out;
}

export function settingsItems(): PaletteItem[] {
  const linux = currentPlatform() === 'linux';
  return settingsSections
    .filter((s) => !s.platform || (s.platform === 'linux') === linux)
    .map((s) => ({
      id: `setting:${s.id}`,
      group: 'Settings' as const,
      label: `Settings: ${s.label}`,
      icon: s.icon,
      run: () => void dispatch('settings.open', { section: s.id }),
    }));
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
        group: 'Reviews',
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

export function ticketItems(hits: readonly TicketItem[]): PaletteItem[] {
  return hits.map((hit) => ({
    id: `ticket:${hit.ticket.ref.account}:${hit.ticket.ref.key}`,
    group: 'Tickets' as const,
    label: `${hit.ticket.ref.key} ${hit.ticket.title}`,
    detail: [hit.ticket.status.name, hit.project_ids.map(projectName).join(', ')].filter(Boolean).join(SEP),
    icon: 'ticket',
    run: () => {
      const project = hit.project_ids[0] ?? projects.activeId;
      if (project) {
        void openContent(project, {
          content: { kind: 'ticket_detail', ticket: hit.ticket.ref },
          placement: 'new_tab',
        });
      }
    },
  }));
}

/** Every synchronous item, in display order. */
export function buildItems(): PaletteItem[] {
  return [
    ...sessionItems(),
    ...projectItems(),
    ...actionItems(),
    ...toolItems(),
    ...commandItems(),
    ...reviewItems(),
    ...settingsItems(),
  ];
}
