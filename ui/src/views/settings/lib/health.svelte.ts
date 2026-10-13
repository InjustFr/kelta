// Settings › Health (#411): what is broken right now, one row per problem with the action that fixes
// it or opens the setting that does. Banners and pane headers still show each problem where it happens.

import { t } from '$lib/i18n/index.svelte';
import * as ipc from '$lib/ipc/commands';
import { settings, tickets, type AccountState } from '$lib/stores';
import { resolveTheme } from '$lib/theme';
import { BELOW_FLOOR } from '$lib/theme/floor';

import { updates } from '../../../shell/updates.svelte';

export interface Problem {
  id: string;
  what: string;
  where: string;
  /** `Date.now()` the problem started (or the data was last good), when known. */
  since: number | null;
  fix: { label: string; action: string; args?: Record<string, unknown> };
}

export interface HealthInput {
  /** `claude_hooks_status` / `claude_mcp_status`: null = Claude Code not found, undefined = not read yet. */
  hooks: boolean | null | undefined;
  mcp: boolean | null | undefined;
  /** `claude.hooks_offer` / `claude.mcp_offer`: false = the user said Never. */
  hooksOffer: boolean;
  mcpOffer: boolean;
  accounts: Record<string, AccountState>;
  /** Ticket sources whose last load failed or served cached tickets. */
  sources: { id: string; label: string; error: string | null; since: number | null }[];
  restart: string[];
  restartSince: number | null;
  /** Version installed and waiting for a restart. */
  update: string | null;
  themeBelowFloor: boolean;
}

const open = (section: string, more: Record<string, unknown> = {}) => ({
  action: 'settings.open',
  args: { section, ...more },
});

export function healthProblems(h: HealthInput): Problem[] {
  const out: Problem[] = [];
  if (h.hooks === false && h.hooksOffer) {
    out.push({
      id: 'claude-hooks',
      what: t("Kelta's hooks are not installed in Claude Code: sessions started in another terminal do not show in the Queue"),
      where: t('Claude'),
      since: null,
      fix: { label: t('Install hooks'), action: 'claude.hooks_install' },
    });
  }
  if (h.mcp === false && h.mcpOffer) {
    out.push({
      id: 'claude-mcp',
      what: t("Kelta's ticket tools are not installed in Claude Code"),
      where: t('Claude'),
      since: null,
      fix: { label: t('Install MCP'), action: 'claude.mcp_install' },
    });
  }
  // rate_limited clears by itself; ok is not a problem.
  for (const [id, a] of Object.entries(h.accounts)) {
    if (a.status === 'ok' || a.status === 'rate_limited') continue;
    const auth = a.status === 'needs_auth';
    out.push({
      id: `account:${id}`,
      what: auth
        ? t('Account {id} needs authentication', { id })
        : a.status === 'offline'
          ? t('Offline: {id} unreachable', { id })
          : t('Account {id}: {message}', { id, message: a.detail ?? t('error') }),
      where: t('Accounts'),
      since: a.since,
      fix: { label: auth ? t('Reconnect account') : t('Open account'), ...open('accounts') },
    });
  }
  for (const s of h.sources) {
    out.push({
      id: `source:${s.id}`,
      what: s.error ?? t('Showing cached tickets: the last refresh failed'),
      where: s.label,
      since: s.since,
      fix: { label: t('Open source'), ...open('sources', { source_id: s.id }) },
    });
  }
  const restart = { label: t('Restart'), action: 'app.restart' };
  if (h.restart.length > 0) {
    out.push({
      id: 'restart',
      what: t('Restart Kelta to apply: {keys}', { keys: h.restart.join(', ') }),
      where: t('Settings'),
      since: h.restartSince,
      fix: restart,
    });
  }
  if (h.update) {
    out.push({
      id: 'update',
      what: t('Kelta {version} is installed. Restart to use it.', { version: h.update }),
      where: t('Updates'),
      since: null,
      fix: restart,
    });
  }
  if (h.themeBelowFloor) {
    out.push({
      id: 'theme',
      what: t(BELOW_FLOOR),
      where: t('Theme'),
      since: null,
      fix: { label: t('Open theme'), ...open('general') },
    });
  }
  return out;
}

class Health {
  hooks = $state<boolean | null | undefined>(undefined);
  mcp = $state<boolean | null | undefined>(undefined);

  /** Reads what no UiEvent reports: Claude Code's install states and the update status. */
  async refresh(): Promise<void> {
    const [hooks, mcp] = await Promise.all([
      ipc.claudeHooksStatus().catch(() => null),
      ipc.claudeMcpStatus().catch(() => null),
      updates.refresh().catch(() => {}),
    ]);
    this.hooks = hooks;
    this.mcp = mcp;
  }

  get problems(): Problem[] {
    const v = settings.value();
    const labels = new Map((v?.sources ?? []).map((s) => [s.id, s.label]));
    const sources = new Map<string, HealthInput['sources'][number]>();
    for (const l of Object.values(tickets.lists)) {
      if (!l.sourceId || (!l.error && !l.stale)) continue;
      const label = labels.get(l.sourceId) ?? l.sourceId;
      sources.set(l.sourceId, { id: l.sourceId, label, error: l.error?.message ?? null, since: l.fetchedAt });
    }
    const state = updates.status?.state;
    return healthProblems({
      hooks: this.hooks,
      mcp: this.mcp,
      hooksOffer: v?.claude.hooks_offer ?? true,
      mcpOffer: v?.claude.mcp_offer ?? true,
      accounts: settings.accounts,
      sources: [...sources.values()],
      restart: settings.pendingRestart,
      restartSince: settings.restartSince,
      update: state?.type === 'ready' ? state.version : null,
      themeBelowFloor: resolveTheme(v, settings.theme).notes.includes(BELOW_FLOOR),
    });
  }
}

export const health = new Health();
