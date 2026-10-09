// `plugin.command.<id>` (palette, keybindings): runs a contributed command in the backend, where
// its actions are permission-checked against the plugin's grants.

import type { TemplateCtx } from '$lib/gen';
import { registerAction } from '$lib/actions';
import { commandRun } from '$lib/ipc/commands';
import '$lib/plugin-host/boot.svelte';
import { toasts } from '$lib/stores';

import { EMPTY_CTX } from '../tools/actions';

registerAction('plugin.command.*', async (args) => {
  const commandId = String(args?.command_id ?? '');
  try {
    await commandRun({
      command_id: commandId,
      ctx: { ...EMPTY_CTX, ...((args?.ctx ?? {}) as Partial<TemplateCtx>) },
    });
  } catch (e) {
    toasts.error(e, commandId);
  }
});
