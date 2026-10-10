// `plugin.command.<id>` (palette, keybindings, ticket/review actions): runs a contributed command in
// the backend, where its actions are permission-checked against the plugin's grants.

import type { ActionDef, TemplateCtx } from '$lib/gen';
import { dispatch, registerAction } from '$lib/actions';
import { commandRun } from '$lib/ipc/commands';
import '$lib/plugin-host/boot.svelte';
import { plugins, projects, settings, toasts } from '$lib/stores';

import { activateProject } from '../../shell/nav';
import { EMPTY_CTX } from '../tools/actions';

export interface PluginAction {
  command_id: string;
  title: string;
  plugin: string;
}

const KINDS = ['ticket', 'review'] as const;

/** Enabled plugins' `ticket_actions` / `review_actions`, ids as in the backend's `command_defs`. */
export function pluginActions(kind: (typeof KINDS)[number]): PluginAction[] {
  return (plugins.plugins.data ?? [])
    .filter((p) => p.enabled)
    .flatMap((p) =>
      p.contributes[`${kind}_actions`].map((b) => ({
        command_id: `${p.id}/${b.id}`,
        title: b.title,
        plugin: p.name,
      })),
    );
}

export function runPluginAction(commandId: string, ctx: Partial<TemplateCtx>): Promise<boolean> {
  return dispatch(`plugin.command.${commandId}`, { ctx });
}

/** The steps of a config or enabled-plugin command. */
function commandSteps(id: string): ActionDef[] {
  const config = settings.value()?.commands.find((c) => c.id === id);
  if (config) return config.do;
  for (const p of plugins.plugins.data ?? []) {
    if (!p.enabled) continue;
    const cmd =
      p.contributes.commands.find((c) => c.id === id) ??
      KINDS.flatMap((k) => p.contributes[`${k}_actions`]).find((b) => `${p.id}/${b.id}` === id);
    if (cmd) return cmd.do;
  }
  return [];
}

const PANE_STEPS: ActionDef['action'][] = ['open_screen', 'open_tool', 'spawn_session'];

registerAction('plugin.command.*', async (args) => {
  const commandId = String(args?.command_id ?? '');
  const given = (args?.ctx ?? {}) as Partial<TemplateCtx>;
  const projectId = given.extra?.project_id ?? projects.activeId;
  const extra = projectId ? { ...given.extra, project_id: projectId } : { ...given.extra };
  try {
    // A pane never opens in a hidden project: show the run's project first (leaves Now, a global view).
    if (projectId && commandSteps(commandId).some((s) => PANE_STEPS.includes(s.action))) {
      await activateProject(projectId);
    }
    await commandRun({ command_id: commandId, ctx: { ...EMPTY_CTX, ...given, extra } });
  } catch (e) {
    toasts.error(e, commandId);
  }
});
