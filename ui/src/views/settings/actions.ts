// settings.open (Mod+,): shows Settings over the window (#410), or closes it when it is showing.
// Optional args: { section?: string } to land on a given section, { toml?: projectId } to land on
// that project's raw TOML (the Edit TOML mode, for what the forms cannot edit), { source_id } to
// bring that source's card into view in Settings → Sources ({ source_id, anchor: 'steps' }: its Steps editor), { key: 'tickets.wip_limit' } to show that key. Old section ids (`window`) land on the section that took their keys.
// toggleSetting (⌘K, #478) flips an on/off key in All projects; its toast's Undo is settings.restore.

import { t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import type { JsonValue } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { toasts, sessions, ui } from '$lib/stores';

import { activateProject } from '../../shell/nav';

import { SettingsEditor } from './lib/editor.svelte';

let tomlRequest: string | null = null;

let sourceRequest: { id: string; steps: boolean } | null = null;

let keyRequest: string | null = null;

/** Settings key the Settings pane searches for, once (⌘K's settings results). */
export function takeKeyRequest(): string | null {
  const key = keyRequest;
  keyRequest = null;
  return key;
}

const sourceListeners = new Set<() => void>();

/** A mounted Sources section hears of a request made while it is already showing. */
export function onSourceRequest(fn: () => void): () => void {
  sourceListeners.add(fn);
  return () => void sourceListeners.delete(fn);
}

/** Source whose card (or, with `steps`, its Steps editor) the next Sources section shows, once. */
export function takeSourceRequest(): { id: string; steps: boolean } | null {
  const req = sourceRequest;
  sourceRequest = null;
  return req;
}

/** Project whose TOML the next Settings pane opens on, once. */
export function takeTomlRequest(): string | null {
  const id = tomlRequest;
  tomlRequest = null;
  return id;
}

/** Section ids folded into another: an old settings.open target lands where its keys are now. */
const MOVED: Record<string, string> = { window: 'general' };

registerAction('settings.open', (args) => {
  const asked = typeof args?.section === 'string' ? args.section : null;
  const section = asked === null ? null : (MOVED[asked] ?? asked);
  // The pane's content only carries a section, so the TOML and source targets travel beside it.
  tomlRequest = typeof args?.toml === 'string' ? args.toml : null;
  sourceRequest =
    typeof args?.source_id === 'string' ? { id: args.source_id, steps: args.anchor === 'steps' } : null;
  keyRequest = typeof args?.key === 'string' ? args.key : null;
  const wasOpen = ui.settings !== null;
  if (wasOpen && section === null && tomlRequest === null && sourceRequest === null && keyRequest === null) {
    ui.settings = null; // ⌘, again closes it
    return;
  }
  // A new object re-runs an open pane's section effect, which also takes the TOML request.
  ui.settings = { section };
  if (wasOpen && sourceRequest !== null) sourceListeners.forEach((fn) => fn());
});

// The startup offer of Kelta's MCP in Claude Code (toast: Install / Never; dismissing = Not now).
registerAction('claude.mcp_install', async () => {
  await ipc.claudeMcpInstall();
  toasts.info(t("Kelta's ticket tools are installed: new Claude sessions list them"));
});
registerAction('claude.mcp_never', async () => {
  await ipc.settingsSet({ layer: 'global', path: 'claude.mcp_offer', value: false });
});
// The startup offer of Kelta's hook relay (#345): sessions started outside Kelta show in Now.
registerAction('claude.hooks_install', async () => {
  await ipc.claudeHooksInstall();
  toasts.info(t('Installed: Claude sessions you start from now on show in the Queue'));
});
registerAction('claude.hooks_never', async () => {
  await ipc.settingsSet({ layer: 'global', path: 'claude.hooks_offer', value: false });
});

/** ⌘K's switch (#478): flips `path` in All projects as the Settings form writes it, with Undo. */
export async function toggleSetting(path: string, label: string): Promise<void> {
  const editor = new SettingsEditor(); // the global layer
  await editor.refreshDoc();
  const before = editor.layerValue(path) ?? null; // unset there: Undo removes the key
  const on = editor.valueOf(path) !== true;
  if (!(await editor.set(path, on))) {
    toasts.error(editor.errors[path] ?? '', label);
    return;
  }
  toasts.push({
    level: 'info',
    text: on ? t('{setting} turned on', { setting: label }) : t('{setting} turned off', { setting: label }),
    action: { label: t('Undo'), command: 'settings.restore', args: { path, value: before } },
  });
}

// Undo of toggleSetting: `value` back at `path` in All projects; null resets the key.
registerAction('settings.restore', async (args) => {
  const path = typeof args?.path === 'string' ? args.path : '';
  const editor = new SettingsEditor();
  if (!(await editor.set(path, (args?.value ?? null) as JsonValue)))
    toasts.error(editor.errors[path] ?? '', t('Undo'));
});

// "Set up with Claude" (#343), from the onboarding and the palette: core installs Kelta's MCP when
// needed and opens the setup session in Home.
registerAction('claude.setup', async () => {
  try {
    const info = await ipc.claudeSetup();
    sessions.upsert(info);
    await activateProject(info.project_id);
  } catch (err) {
    toasts.error(err, t('Could not start the setup with Claude'));
  }
});
