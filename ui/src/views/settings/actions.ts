// settings.open (Mod+,): opens a Settings tab in the active project, or focuses the existing one.
// Optional args: { section?: string } to land on a given section, { toml?: projectId } to land on
// that project's raw TOML (the Edit TOML mode, for what the forms cannot edit), { source_id } to
// bring that source's card into view in Settings → Sources.

import { t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import * as ipc from '$lib/ipc/commands';
import { findContent, focusPane, replacePaneContent, updateTab } from '$lib/layout';
import { toasts, layout, projects, sessions } from '$lib/stores';

import { activateProject } from '../../shell/nav';

let tomlRequest: string | null = null;

let sourceRequest: string | null = null;

const sourceListeners = new Set<() => void>();

/** A mounted Sources section hears of a request made while it is already showing. */
export function onSourceRequest(fn: () => void): () => void {
  sourceListeners.add(fn);
  return () => void sourceListeners.delete(fn);
}

/** Source whose card the next Sources section shows, once. */
export function takeSourceRequest(): string | null {
  const id = sourceRequest;
  sourceRequest = null;
  return id;
}

/** Project whose TOML the next Settings pane opens on, once. */
export function takeTomlRequest(): string | null {
  const id = tomlRequest;
  tomlRequest = null;
  return id;
}

registerAction('settings.open', async (args) => {
  const projectId = projects.activeId ?? projects.home?.id ?? 'home';
  const section = typeof args?.section === 'string' ? args.section : null;
  // PaneContent::Settings only carries a section, so the TOML target travels beside it.
  tomlRequest = typeof args?.toml === 'string' ? args.toml : null;
  sourceRequest = typeof args?.source_id === 'string' ? args.source_id : null;
  try {
    await layout.ensure(projectId);
    await activateProject(projectId); // from a global view the pane would open out of sight
    const current = layout.get(projectId);
    const open = current && findContent(current, (c) => c.kind === 'settings');
    if (open) {
      // A new content object re-runs the pane's section effect, which also takes the TOML request.
      const retarget = section !== null || tomlRequest !== null || sourceRequest !== null;
      layout.update(projectId, (l) => {
        const next = retarget
          ? updateTab(l, open.tabId, (t) => ({
              ...t,
              root: replacePaneContent(t.root, open.paneId, { kind: 'settings', section }),
            }))
          : l;
        return focusPane(next, open.tabId, open.paneId);
      });
      if (sourceRequest !== null) sourceListeners.forEach((fn) => fn());
      return;
    }
    layout.open(projectId, {
      content: { kind: 'settings', section },
      placement: 'new_tab',
      focus: true,
      tab_title: 'Settings',
      work_item_id: null,
    });
  } catch (err) {
    tomlRequest = null;
    sourceRequest = null;
    toasts.error(err, t('Could not open settings'));
  }
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
