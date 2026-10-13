import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { healthProblems, type HealthInput } from './lib/health.svelte';
import './actions';
import Health from './sections/Health.svelte';

const fine: HealthInput = {
  hooks: true,
  mcp: true,
  hooksOffer: true,
  mcpOffer: true,
  accounts: { gh: { status: 'ok', detail: null, since: 1 } },
  sources: [],
  restart: [],
  restartSince: null,
  update: null,
  themeBelowFloor: false,
};

describe('healthProblems', () => {
  it('is empty when everything works', () => {
    expect(healthProblems(fine)).toEqual([]);
  });

  it('lists one row per problem kind with its fix', () => {
    const problems = healthProblems({
      ...fine,
      hooks: false,
      mcp: false,
      accounts: {
        jira: { status: 'needs_auth', detail: '401', since: 10 },
        gl: { status: 'offline', detail: null, since: 20 },
        busy: { status: 'rate_limited', detail: null, since: 30 },
      },
      sources: [{ id: 'board', label: 'My board', error: 'boom', since: 40 }],
      restart: ['window.decorations'],
      restartSince: 50,
      update: '9.9.9',
      themeBelowFloor: true,
    });
    expect(problems.map((p) => [p.id, p.fix.label, p.fix.action, p.fix.args?.section ?? null])).toEqual([
      ['claude-hooks', 'Install hooks', 'claude.hooks_install', null],
      ['claude-mcp', 'Install MCP', 'claude.mcp_install', null],
      ['account:jira', 'Reconnect account', 'settings.open', 'accounts'],
      ['account:gl', 'Open account', 'settings.open', 'accounts'],
      ['source:board', 'Open source', 'settings.open', 'sources'],
      ['restart', 'Restart', 'app.restart', null],
      ['update', 'Restart', 'app.restart', null],
      ['theme', 'Open theme', 'settings.open', 'general'],
    ]);
    const source = problems.find((p) => p.id === 'source:board')!;
    expect(source).toMatchObject({ what: 'boom', where: 'My board', since: 40 });
    expect(source.fix.args).toEqual({ section: 'sources', source_id: 'board' });
    expect(problems.find((p) => p.id === 'restart')!.what).toContain('window.decorations');
  });

  it('skips Claude Code when it is missing, not read yet or declined with Never', () => {
    expect(healthProblems({ ...fine, hooks: null, mcp: undefined })).toEqual([]);
    expect(healthProblems({ ...fine, hooks: false, mcp: false, hooksOffer: false, mcpOffer: false })).toEqual(
      [],
    );
  });
});

describe('Health section', () => {
  it('lists the mock\'s missing Claude installs, then says everything works once fixed', async () => {
    render(Health, { layer: 'global', projectId: null, repoId: null });
    await fireEvent.click(await screen.findByRole('button', { name: 'Install hooks' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Install MCP' }));
    expect(await screen.findByText('Everything works.')).toBeTruthy();
  });
});
