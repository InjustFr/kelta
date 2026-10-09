// Display labels for sessions and panes (headers, palette, status bar).

import type { PaneContent, SessionInfo, SessionKind, SessionStatus } from '$lib/gen';
import { defaultTitle } from '$lib/layout';

const STATUS_LABELS: Record<SessionStatus, string> = {
  starting: 'Starting',
  running: 'Running',
  working: 'Working',
  needs_input: 'Needs input',
  waiting_user: 'Waiting for you',
  done: 'Done',
  error: 'Error',
  exited: 'Exited',
  unknown: '',
};

export function statusLabel(status: SessionStatus): string {
  return STATUS_LABELS[status];
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
  if (content.kind === 'terminal') return session?.title?.trim() || session?.name || 'Terminal';
  return defaultTitle(content);
}
