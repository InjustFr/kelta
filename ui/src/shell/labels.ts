// Display labels for sessions and panes (headers, palette, status bar).

import { msg, t } from '$lib/i18n/index.svelte';
import type { PaneContent, SessionInfo, SessionKind, SessionStatus } from '$lib/gen';
import { effectiveChords } from '$lib/keys/manager';
import { defaultTitle } from '$lib/layout';
import { settings } from '$lib/stores';
import type { LampLevel } from '$lib/stores/reducers';

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

/** The user's first effective chord for an action (bindings override ⊕ platform default). */
export function chordFor(actionId: string): string | undefined {
  return effectiveChords(actionId, settings.value()?.keys ?? null)[0];
}
