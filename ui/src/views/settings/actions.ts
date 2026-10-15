// settings.open (Mod+,): shows Settings over the window (#410), or closes it when it is showing.
// Optional args: { section?: string } to land on a given section, { toml?: projectId } to land on
// that project's raw TOML (the Edit TOML mode, for what the forms cannot edit), { source_id } to
// bring that source's card into view in Settings → Sources ({ source_id, anchor: 'steps' }: its Steps editor), { key: 'tickets.wip_limit' } to show that key.

import { t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import * as ipc from '$lib/ipc/commands';
import { toasts, sessions, ui } from '$lib/stores';

import { activateProject } from '../../shell/nav';

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

registerAction('settings.open', (args) => {
  const section = typeof args?.section === 'string' ? args.section : null;
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
