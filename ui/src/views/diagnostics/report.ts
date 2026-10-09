// Pure helpers of the Diagnostics pane and the onboarding checklist.

import type { AppInfo, Check, CheckStatus, Diagnostics } from '$lib/gen';

export const STATUS_ICON: Record<CheckStatus, string> = {
  ok: 'circle-check',
  warn: 'triangle-alert',
  fail: 'circle-x',
};

export const STATUS_TONE: Record<CheckStatus, 'ok' | 'warn' | 'danger'> = {
  ok: 'ok',
  warn: 'warn',
  fail: 'danger',
};

const RANK: Record<CheckStatus, number> = { fail: 0, warn: 1, ok: 2 };

/** Failing checks first, then warnings, then the rest in their original order. */
export function sortChecks(checks: readonly Check[]): Check[] {
  return checks
    .map((c, i) => ({ c, i }))
    .sort((a, b) => RANK[a.c.status] - RANK[b.c.status] || a.i - b.i)
    .map((x) => x.c);
}

export function counts(checks: readonly Check[]): Record<CheckStatus, number> {
  const out: Record<CheckStatus, number> = { ok: 0, warn: 0, fail: 0 };
  for (const c of checks) out[c.status] += 1;
  return out;
}

/** Plain-text report for bug reports. Contains no secrets: only versions, paths and check output. */
export function reportText(info: AppInfo | null, diag: Diagnostics | null): string {
  const lines: string[] = [];
  if (info) {
    lines.push(`Kelta ${info.version} on ${info.platform}/${info.arch}`);
    lines.push(`config: ${info.config_dir}`);
    lines.push(`data: ${info.data_dir}`);
    lines.push(`runtime: ${info.runtime_dir}`);
    if (info.claude)
      lines.push(`claude: ${info.claude.version} (${info.claude.path})${info.claude.ok ? '' : ' too old'}`);
    if (info.safe_graphics) lines.push('safe graphics: on');
    lines.push('');
  }
  for (const c of diag?.checks ?? []) {
    lines.push(`[${c.status.toUpperCase()}] ${c.label}: ${c.detail}`);
    if (c.fix) lines.push(`    fix: ${c.fix}`);
  }
  return lines.join('\n');
}

/** The first-run checklist needs attention when any check failed or warned. */
export function needsAttention(diag: Diagnostics | null): boolean {
  return !!diag && diag.checks.some((c) => c.status !== 'ok');
}
