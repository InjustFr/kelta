// Display labels for sessions and panes (headers, palette, status bar).

import { msg, t } from '$lib/i18n/index.svelte';
import type {
  ClaudeRun,
  PaneContent,
  RunClaim,
  SessionId,
  SessionInfo,
  SessionKind,
  SessionStatus,
  ToolInfo,
} from '$lib/gen';
import { effectiveChords } from '$lib/keys/manager';
import { defaultTitle } from '$lib/layout';
import { settings } from '$lib/stores';
import type { LampLevel } from '$lib/stores/reducers';
import { isIconName } from '$lib/ui/icons';

const STATUS_LABELS: Record<SessionStatus, string> = {
  starting: msg('Starting'),
  running: msg('Running'),
  working: msg('Working'),
  needs_input: msg('Needs input'),
  waiting_user: msg('Waiting for you'),
  done: msg('Done'),
  error: msg('Error'),
  exited: msg('Exited'),
  unknown: '',
};

export function statusLabel(status: SessionStatus): string {
  return t(STATUS_LABELS[status]);
}

export function sessionKindName(kind: SessionKind): string {
  switch (kind.type) {
    case 'editor':
      return kind.adapter;
    case 'tool':
      return kind.tool_id;
    default:
      return kind.type;
  }
}

export function sessionIcon(kind: SessionKind): string {
  switch (kind.type) {
    case 'claude':
      return 'bot';
    case 'editor':
      return 'file-code';
    case 'tool':
      return 'wrench';
    case 'setup':
      return 'play';
    default:
      return 'square-terminal';
  }
}

export function paneIcon(content: PaneContent, session: SessionInfo | null): string {
  switch (content.kind) {
    case 'terminal':
      return session ? sessionIcon(session.kind) : 'square-terminal';
    case 'claude_runs':
      return 'list-tree';
    case 'web':
      return 'globe';
    case 'plugin_screen':
      return 'puzzle';
    case 'tickets':
      return content.mode === 'board' ? 'kanban' : 'ticket';
    case 'ticket_detail':
      return 'ticket';
    case 'reviews':
    case 'review_detail':
      return 'git-pull-request';
    case 'inbox':
      return 'inbox';
    case 'work_item':
      return 'git-branch';
    case 'settings':
      return 'settings';
    case 'diagnostics':
      return 'activity';
    case 'welcome':
      return 'house';
    case 'empty':
      return 'square';
  }
}

export function paneTitle(content: PaneContent, session: SessionInfo | null): string {
  if (content.kind === 'terminal') return session?.title?.trim() || session?.name || t('Terminal');
  return t(defaultTitle(content)); // stored English, shown translated
}

const ATTENTION_LABELS: Record<LampLevel, string> = {
  none: '',
  activity: msg('New output'),
  done: msg('Ready to review'),
  working: msg('Working'),
  parked: msg('Parked'),
  error: msg('Error'),
  needs_input: msg('Needs input'),
};
/** Status word shown beside a project/tab/pane lamp. */
export function attentionLabel(level: LampLevel): string {
  return t(ATTENTION_LABELS[level]);
}
const RUN_STATUS: Record<string, string> = {
  running: msg('running'),
  completed: msg('completed'),
  failed: msg('failed'),
  killed: msg('killed'),
  blocked: msg('blocked'),
  done: msg('done'),
};
/** A Claude workflow run status (`completed`, `killed`, ...); an unknown one is shown as is. */
export function runStatusLabel(status: string): string {
  return t(RUN_STATUS[status] ?? status);
}

/** A session's run to show: the last running one, else the last (#272). */
export function currentRun(list: readonly ClaudeRun[] | undefined): ClaudeRun | null {
  return list?.findLast((r) => r.status === 'running') ?? list?.at(-1) ?? null;
}

/** The run's phase with a running agent, or its final status. */
export function runPhase(run: ClaudeRun): string {
  if (run.status !== 'running') return runStatusLabel(run.status);
  const phase = run.phases.findLast((p) => p.agents.some((a) => !a.done)) ?? run.phases.at(-1);
  return phase?.title ?? t('starting');
}

/**
 * A rail tab row's run sub line (#392): its work item's claimed run, else the running runs of its
 * sessions that no live claim names (those show on their ticket), `+N` past the first.
 */
export function runLine(
  claim: RunClaim | null | undefined,
  runs: readonly ClaudeRun[],
  claims: readonly RunClaim[],
): { text: string; sessionId: SessionId } | null {
  if (claim?.session_id && claim.state !== 'done') {
    const phase = claim.state === 'running' && claim.phase ? claim.phase : runStatusLabel(claim.state);
    return { text: `${claim.name ?? t('workflow')} · ${phase}`, sessionId: claim.session_id };
  }
  // One live claim takes one running run of its session with the same workflow name.
  const left = claims
    .filter((c) => c.state === 'running' || c.state === 'blocked')
    .map((c) => `${c.session_id} ${c.name}`);
  const loose = runs.filter((r) => {
    if (r.status !== 'running') return false;
    const i = left.indexOf(`${r.session_id} ${r.name}`);
    if (i >= 0) left.splice(i, 1);
    return i < 0;
  });
  const first = loose[0];
  if (!first) return null;
  const more = loose.length > 1 ? ` +${loose.length - 1}` : '';
  return { text: `${first.name ?? t('workflow')} · ${runPhase(first)}${more}`, sessionId: first.session_id };
}

// Well-known tools without an `icon`, by id or lower-cased label (#377).
const TOOL_ICONS: Record<string, string> = {
  lazydocker: 'container',
  docker: 'container',
  lazygit: 'git-branch',
  gitui: 'git-branch',
  tig: 'git-branch',
  shell: 'terminal',
  terminal: 'terminal',
  bash: 'terminal',
  zsh: 'terminal',
  fish: 'terminal',
  editor: 'file-code',
  nvim: 'file-code',
  neovim: 'file-code',
  vim: 'file-code',
  helix: 'file-code',
  hx: 'file-code',
};

/** A tool's mark: its `icon` (Lucide name or 1-2 characters), a well-known tool's icon, else its label's first two letters. */
export function toolIcon(tool: Pick<ToolInfo, 'id' | 'label' | 'icon'>): string {
  const icon = tool.icon?.trim();
  if (icon && (isIconName(icon) || [...icon].length <= 2)) return icon;
  return TOOL_ICONS[tool.id] ?? TOOL_ICONS[tool.label.toLowerCase()] ?? tool.label.slice(0, 2);
}

/** The user's first effective chord for an action (bindings override ⊕ platform default). */
export function chordFor(actionId: string): string | undefined {
  return effectiveChords(actionId, settings.value()?.keys ?? null)[0];
}
