import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import type { Component } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { settingsSections, type SettingsSectionId, type SettingsSectionProps } from '$app/registry';
import * as ipc from '$lib/ipc/commands';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { dispatch } from '$lib/actions';
import { settings, ui } from '$lib/stores';

import schemaJson from '../../../../schema/settings.schema.json';
import { COPY, COPY_DONE, SECTION_COPY } from './copy';
import { joinPath, splitPath } from './lib/paths';
import { enumOptions, fieldKind, leafFields, nodeAt, type SchemaNode } from './lib/schema';
import { SECTION_ROOTS, SECTION_ROWS } from './lib/sections';
import { takeSourceRequest } from './actions';
import SectionForm from './SectionForm.svelte';
import Claude from './sections/Claude.svelte';
import Keys from './sections/Keys.svelte';
import Notifications from './sections/Notifications.svelte';
import Terminal from './sections/Terminal.svelte';
import Worktree from './sections/Worktree.svelte';

const schema = schemaJson as unknown as SchemaNode;
const onOs = (p: 'MacIntel' | 'Linux x86_64') => vi.spyOn(navigator, 'platform', 'get').mockReturnValue(p);

afterEach(() => vi.restoreAllMocks());

async function renderForm(sectionId: SettingsSectionId) {
  const { transport } = createMockTransport();
  setTransport(transport);
  const r = render(SectionForm, { sectionId, layer: 'global' });
  await vi.waitFor(() => expect(r.container.querySelector('[data-path]')).toBeTruthy());
  return r;
}

describe('settings copy table', () => {
  it('each entry names a key of its own section', () => {
    for (const [section, table] of Object.entries(SECTION_COPY)) {
      for (const path of Object.keys(table)) {
        expect(nodeAt(schema, splitPath(path)), path).toBeTruthy();
        expect(SECTION_ROOTS[section as SettingsSectionId], path).toContain(splitPath(path)[0]);
      }
    }
  });

  it('option labels name real enum values', () => {
    for (const [path, entry] of Object.entries(COPY)) {
      if (!entry.options) continue;
      const values = enumOptions(nodeAt(schema, splitPath(path))!).map((o) => o.value);
      for (const v of Object.keys(entry.options)) expect(values, `${path}.${v}`).toContain(v);
    }
  });

  it('enum options take their label from the table', () => {
    const labels = enumOptions(nodeAt(schema, ['keys', 'linux_terminal'])!, 'keys.linux_terminal').map(
      (o) => o.label,
    );
    expect(labels).toEqual(['Terminal first', 'Kelta first']);
  });

  it('platform hides the keys of the other OS', () => {
    const paths = () => leafFields(schema, ['terminal', 'keys']).map((f) => f.path);
    onOs('MacIntel');
    expect(paths()).toContain('terminal.option_as_meta');
    expect(paths()).not.toContain('terminal.primary_selection');
    expect(paths()).not.toContain('keys.linux_terminal');
    onOs('Linux x86_64');
    expect(paths()).not.toContain('terminal.option_as_meta');
    expect(paths()).toContain('terminal.primary_selection');
    expect(paths()).toContain('keys.linux_terminal');
  });

  it('the form leaves out the keys of the other OS', async () => {
    onOs('MacIntel');
    const { container, unmount } = await renderForm('terminal');
    expect(container.querySelector('[data-path="terminal.primary_selection"]')).toBeNull();
    expect(container.querySelector('[data-path="terminal.option_as_meta"]')).toBeTruthy();
    unmount();
  });

  it('leafFields orders keys by the table across roots and drops a root of the other OS', () => {
    const paths = () => leafFields(schema, SECTION_ROOTS.worktree!).map((f) => f.path);
    expect(paths().slice(0, 4)).toEqual([
      'work.on_start.assign_me',
      'work.plan_preview',
      'work.finish_first',
      'work.default_template',
    ]);
    const saved = COPY.tickets!; // not inject(): its afterEach would delete the real entry
    COPY.tickets = { ...saved, platform: 'linux' };
    try {
      onOs('MacIntel');
      expect(paths()).not.toContain('tickets.wip_limit');
    } finally {
      COPY.tickets = saved;
    }
  });

  it('Terminal is grouped by task, scrollback outside Advanced (#470)', async () => {
    onOs('MacIntel');
    const { container, unmount } = await renderForm('terminal');
    const heads = [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent);
    expect(heads).toEqual([
      'Text',
      'Cursor',
      'Clipboard',
      'Keyboard',
      'Scrollback & history',
      'Shell',
      'Sound',
    ]);
    for (const p of ['terminal.scrollback', 'terminal.osc52'])
      expect(container.querySelector(`[data-path="${p}"]`)!.closest('[data-testid="advanced"]')).toBeNull();
    unmount();
  });

  it('every key of a finished section has a label (COPY_DONE)', () => {
    for (const os of ['MacIntel', 'Linux x86_64'] as const) {
      onOs(os);
      const missing = new Set<string>();
      for (const s of COPY_DONE) {
        const roots = SECTION_ROOTS[s] ?? [];
        // mirrors SectionForm: a lone root group shows its children, not itself
        const min = roots.length === 1 && fieldKind(schema.properties![roots[0]!]!) === 'group' ? 2 : 1;
        for (const leaf of leafFields(schema, roots))
          for (let i = min; i <= leaf.segs.length; i++) {
            const p = joinPath(leaf.segs.slice(0, i));
            if (!COPY[p]?.label) missing.add(p);
          }
      }
      expect([...missing]).toEqual([]);
    }
  });
});

describe('Settings → Notifications (#472)', () => {
  const EVENTS = [
    'notifications.claude_blocked',
    'notifications.claude_done',
    'notifications.review_requested',
    'notifications.pr_approved',
    'notifications.pr_changes_requested',
    'notifications.ci_failed_mine',
  ];
  const mount = async (seed: Record<string, boolean> = {}) => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    for (const [k, value] of Object.entries(seed))
      await ipc.settingsSet({ layer: 'global', path: `notifications.${k}`, value });
    await settings.load();
    const r = render(Notifications, { layer: 'global', projectId: null, repoId: null });
    await waitFor(() => expect(r.container.querySelector('[data-path]')).toBeTruthy());
    // the editor's first load must land before a click, or it overwrites the write
    await waitFor(() => expect(controls.calls.some((c) => c.cmd === 'settings_layer_get')).toBe(true));
    return r;
  };
  const row = (c: HTMLElement, p: string) => c.querySelector(`[data-path="${p}"]`)!;

  it('the switch comes first, events under Tell me when, then Quiet time; the test row follows the switches', async () => {
    const { container, unmount } = await mount();
    const fields = [...container.querySelectorAll('[data-testid="field"]')].filter(
      (f) => !f.closest('[data-testid="advanced"]'),
    );
    expect(fields.map((f) => f.getAttribute('data-path'))).toEqual([
      'notifications.enabled',
      ...EVENTS,
      'notifications.quiet_hours',
      'notifications.only_when_unfocused',
      'notifications.focus_allow_blocked',
    ]);
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(['Tell me when', 'Quiet time']);
    expect(row(container, 'notifications.enabled').closest('[data-testid="copy-group"]')).toBeNull();
    expect(row(container, 'notifications.bell_background').closest('[data-testid="advanced"]')).toBeTruthy();
    const test = container.querySelector('[data-testid="notification-test"]')!;
    expect(fields.at(-1)!.compareDocumentPosition(test) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    unmount();
  });

  it('the events are disabled, still shown, while notifications are off', async () => {
    const { container, unmount } = await mount({ claude_done: true });
    const reset = () => row(container, 'notifications.claude_done').querySelector('[data-testid="reset"]');
    const sw = (p: string) =>
      within(row(container, p) as HTMLElement).getByRole('switch') as HTMLInputElement;
    const master = () => screen.getByRole('switch', { name: 'Desktop notifications' }) as HTMLInputElement;
    await fireEvent.click(master());
    await waitFor(() => expect(sw('notifications.claude_blocked').disabled).toBe(true));
    for (const p of EVENTS) expect(sw(p).disabled).toBe(true);
    expect(reset()).toBeNull(); // no Reset on a row another setting disabled (#482)
    for (const i of row(container, 'notifications.quiet_hours').querySelectorAll('input[type="time"]'))
      expect((i as HTMLInputElement).disabled).toBe(true);
    await waitFor(() => expect(master().disabled).toBe(false)); // never disabled by its own dependents
    await fireEvent.click(master());
    await waitFor(() => expect(sw('notifications.claude_blocked').disabled).toBe(false));
    await waitFor(() => expect(reset()).toBeTruthy());
    unmount();
  });
});

describe('Settings → Terminal (#482)', () => {
  it('the disk caps are disabled, still shown, while history is off', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await ipc.settingsSet({ layer: 'global', path: 'terminal.history_log', value: false });
    await settings.load();
    const { container, unmount } = render(Terminal, { layer: 'global', projectId: null, repoId: null });
    for (const p of ['terminal.history_log_mb', 'terminal.history_log_total_mb']) {
      await waitFor(() => expect(container.querySelector(`[data-path="${p}"]`), p).toBeTruthy());
      expect(container.querySelector<HTMLInputElement>(`[data-path="${p}"] input`)!.disabled, p).toBe(true);
    }
    unmount();
  });
});

describe('SECTION_ROWS anchors (#482)', () => {
  const anchors = Object.entries(SECTION_ROWS).flatMap(([id, rows]) =>
    (rows ?? []).flatMap((r) => (r.anchor ? [[id, r.anchor] as const] : [])),
  );
  it.each(anchors)('%s draws its %s row', async (id, anchor) => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    const C = (await settingsSections.find((s) => s.id === id)!.load())
      .default as unknown as Component<SettingsSectionProps>;
    const { container, unmount } = render(C, { layer: 'global', projectId: null, repoId: null });
    await waitFor(() => expect(container.querySelector(`[data-testid="${anchor}"]`)).toBeTruthy());
    unmount();
  });
});

describe('Settings → Work items', () => {
  it('stage links, groups in the order work happens, no ticket move outside Advanced, the note opens Sources → Steps', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    await import('./actions');
    const seen = vi.fn();
    Element.prototype.scrollIntoView = seen;
    const { container, unmount } = render(Worktree, { layer: 'global', projectId: null, repoId: null });
    await waitFor(() => expect(container.querySelector('[data-path]')).toBeTruthy());
    const paths = (els: Element[]) => els.map((e) => e.getAttribute('data-path'));
    const fields = [...container.querySelectorAll('[data-testid="field"]')];
    expect(paths(fields.filter((f) => !f.closest('[data-testid="advanced"]')))).toEqual([
      'work.on_start.assign_me',
      'work.plan_preview',
      'work.finish_first',
      'work.default_template',
      'worktree.branch_template',
      'worktree.root',
      'worktree.include',
      'worktree.setup',
      'worktree.setup_blocking',
      'worktree.teardown',
      'work.pr.title_template',
      'work.pr.body_template',
      'work.pr.draft',
      'work.pr.merge_method',
      'work.on_merge.offer_cleanup',
      'tickets.wip_limit',
      'work.return_brief_after_mins',
    ]);
    const stages = ['Starting', 'Branch folder', 'Pull request', 'After merge', 'Limits'];
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(stages);
    const nav = within(container.querySelector('[data-testid="work-stages"]') as HTMLElement);
    expect(nav.getAllByRole('button').map((b) => b.textContent?.trim())).toEqual(stages);
    const advanced = container.querySelector('[data-testid="advanced"]')!;
    for (const p of ['work.on_pr.comment', 'work.on_start.transition_to'])
      expect(advanced.querySelector(`[data-path="${p}"]`), p).toBeTruthy();

    await fireEvent.click(nav.getByRole('button', { name: 'After merge' }));
    expect((seen.mock.contexts[0] as HTMLElement).dataset.group).toBe('After merge');

    await fireEvent.click(screen.getByRole('button', { name: 'Edit steps in Sources' }));
    expect(ui.settings).toEqual({ section: 'sources' });
    expect(takeSourceRequest()).toEqual({ id: settings.value()!.sources[0]!.id, steps: true });
    ui.settings = null;
    unmount();
  });

  it('at the repo layer only the Branch folder group shows, with no stage links or Sources note', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    const { container, unmount } = render(Worktree, { layer: 'repo', projectId: null, repoId: 'r1' });
    await waitFor(() => expect(container.querySelector('[data-path]')).toBeTruthy());
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(['Branch folder']);
    expect(container.querySelector('[data-testid="work-stages"]')).toBeNull();
    expect(container.querySelector('[data-testid="steps-note"]')).toBeNull();
    unmount();
  });
});

describe('Settings → Work items (#482)', () => {
  it('Edit steps opens the board feeding the project, and hides with no boards', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    await import('./actions');
    const first = render(Worktree, { layer: 'global', projectId: 'billing', repoId: null });
    await fireEvent.click(await screen.findByRole('button', { name: 'Edit steps in Sources' }));
    expect(takeSourceRequest()).toEqual({ id: 'billing', steps: true });
    ui.settings = null;
    first.unmount();

    await ipc.settingsSet({ layer: 'global', path: 'sources', value: [] });
    await settings.load();
    const { container, unmount } = render(Worktree, { layer: 'global', projectId: 'billing', repoId: null });
    await waitFor(() => expect(container.querySelector('[data-testid="steps-note"]')).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'Edit steps in Sources' })).toBeNull();
    unmount();
  });
});

describe('Settings → Keys (#482)', () => {
  const mount = async (props: SettingsSectionProps) => {
    onOs('MacIntel');
    const created = createMockTransport();
    setTransport(created.transport);
    await settings.load();
    return { ...render(Keys, props), controls: created.controls };
  };

  it('at the repo layer the bindings say why they are read-only', async () => {
    const { container, unmount } = await mount({ layer: 'repo', projectId: 'shop', repoId: 'api' });
    const section = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('[data-testid="keys-section"]');
      if (!el) throw new Error('no keys section yet');
      return el;
    });
    expect(section.textContent).toContain('Not allowed in a repo-local config');
    expect(section.textContent).not.toContain('The same for every project.');
    unmount();
  });

  it('Use it here writes the new shortcut before freeing the other action', async () => {
    const { container, controls, unmount } = await mount({ layer: 'global', projectId: null, repoId: null });
    const detach = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('tr[data-action="pane.detach"]');
      if (!el) throw new Error('no detach row yet');
      return el;
    });
    await waitFor(() => expect(controls.calls.some((c) => c.cmd === 'settings_layer_get')).toBe(true));
    await fireEvent.click(detach.querySelector('[data-testid="record"]')!);
    await fireEvent.keyDown(screen.getByTestId('recorder'), { code: 'KeyD', key: 'd', metaKey: true });
    controls.failNext('settings_set', { code: 'internal' });
    await fireEvent.click(await screen.findByRole('button', { name: 'Use it here' }));
    await waitFor(() => expect(controls.calls.filter((c) => c.cmd === 'settings_set').length).toBe(1));
    const sets = controls.calls.filter((c) => c.cmd === 'settings_set');
    expect(JSON.stringify(sets[0]!.args)).toContain('pane.detach');
    expect(JSON.stringify(sets)).not.toContain('pane.split_right');
    unmount();
  });
});

describe('Settings → General', () => {
  it('Window keys sit under Appearance and Startup & quit; settings.open { section: "window" } lands on General', async () => {
    const { container, unmount } = await renderForm('general');
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(['Appearance', 'Startup & quit', 'Updates']);
    for (const [group, path] of [
      ['Appearance', 'window.decorations'],
      ['Startup & quit', 'window.close_behavior'],
      ['Startup & quit', 'window.restore_geometry'],
    ])
      expect(container.querySelector(`[data-group="${group}"] [data-path="${path}"]`), path).toBeTruthy();
    unmount();

    await import('./actions');
    await dispatch('settings.open', { section: 'window' });
    expect(ui.settings).toEqual({ section: 'general' });
    ui.settings = null;
  });
});

describe('Settings → Claude', () => {
  it('fields follow the table order under its three groups, its own rows inside theirs, launch options under Advanced', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    const { container, unmount } = render(Claude, { layer: 'global', projectId: null, repoId: null });
    await waitFor(() => expect(container.querySelector('[data-testid="claude-mcp"]')).toBeTruthy());
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] > header > h3')].map((h) => h.textContent),
    ).toEqual(['Sessions & cost', 'Profiles', 'Kelta in Claude Code']);
    const first = container.querySelector('[data-group="Sessions & cost"]')!;
    expect(
      [...first.querySelectorAll('[data-testid="field"]')].map((e) => e.getAttribute('data-path')),
    ).toEqual(['claude.max_live', 'claude.budget_usd', 'claude.auto_park_after_mins']);
    for (const [group, id] of [
      ['Profiles', 'claude-permission-mode'],
      ['Kelta in Claude Code', 'claude-mcp'],
      ['Kelta in Claude Code', 'claude-hooks'],
    ])
      expect(container.querySelector(`[data-group="${group}"] [data-testid="${id}"]`), id).toBeTruthy();
    const advanced = container.querySelector('[data-testid="advanced"]')!;
    for (const p of ['binary', 'extra_args', 'extra_hooks', 'terminal_command'])
      expect(advanced.querySelector(`[data-path="claude.${p}"]`), p).toBeTruthy();
    unmount();
  });

  it('at the repo layer its own rows stay hidden: they change Claude Code for every project (#482)', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    const { container, unmount } = render(Claude, { layer: 'repo', projectId: 'shop', repoId: 'api' });
    await waitFor(() =>
      expect(container.querySelector('[data-path="claude.append_system_prompt"]')).toBeTruthy(),
    );
    expect(container.querySelector('[data-testid="copy-group"]')).toBeNull();
    for (const id of ['claude-mcp', 'claude-hooks', 'claude-permission-mode'])
      expect(container.querySelector(`[data-testid="${id}"]`), id).toBeNull();
    unmount();
  });
});
