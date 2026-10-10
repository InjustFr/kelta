// Config warnings (#255): a bad project colour is shown on the colour field and in the raw editor,
// and never disables Save.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { ValidationIssue } from '$lib/gen';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings } from '$lib/stores';

import EditToml from './EditToml.svelte';
import SettingsPane from './SettingsPane.svelte';
import { SettingsEditor } from './lib/editor.svelte';
import Projects from './sections/Projects.svelte';

const COLOR: ValidationIssue = {
  path: 'project.color',
  message: '`color` must be a theme colour or #rrggbb',
  line: 3,
  col: 9,
  severity: 'warning',
};

beforeEach(async () => {
  const { transport } = createMockTransport();
  // what the Rust validator answers for `color = "red"` in a project file
  setTransport({
    ...transport,
    invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array): Promise<T> =>
      cmd === 'settings_validate' ? Promise.resolve([COLOR] as T) : transport.invoke<T>(cmd, args),
  });
  await Promise.all([projects.load(), settings.load()]);
});

describe('config warnings', () => {
  it('the colour field of Settings → Projects shows the warning', async () => {
    render(Projects, { layer: 'global', projectId: null, repoId: null });
    const shop = () => within(document.querySelector<HTMLElement>('[data-project-id="shop"]')!);
    await waitFor(() =>
      expect(shop().getByTestId('color-warning').textContent).toContain('theme colour or #rrggbb'),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Edit Shop' }));
    await waitFor(() => expect(shop().getByTestId('color-warning').getAttribute('role')).toBe('status'));
  });

  it('the Settings banner lists warnings apart from errors', async () => {
    render(SettingsPane, {
      projectId: 'shop',
      tabId: 't1',
      paneId: 'p1',
      visible: true,
      focused: true,
      content: { kind: 'settings', section: 'projects' },
    });
    const banner = await screen.findByTestId('layer-warnings');
    expect(banner.textContent).toContain('project.color');
    expect(screen.queryByTestId('layer-issues')).toBeNull();
  });

  it('the raw editor lists warnings apart and keeps Save enabled', async () => {
    const editor = new SettingsEditor({ layer: 'project', projectId: 'shop' });
    await editor.load();
    render(EditToml, { editor });
    const warnings = await screen.findByTestId('toml-warnings');
    expect(warnings.textContent).toContain('project.color');
    expect(screen.queryByTestId('toml-issues')).toBeNull();
    expect(screen.getByTestId('toml-valid')).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('TOML source'), { target: { value: 'color = "red"\n' } });
    const save = screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement;
    expect(save.disabled).toBe(false);
  });
});
