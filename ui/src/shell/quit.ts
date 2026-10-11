// Quit dialog (#305): the backend sends `app.quit_requested` when keltad would keep sessions
// running past the quit. The answer quits through `app_quit`; "Don't ask again" stores it in
// `app.quit_sessions`, which skips the dialog from then on. Cancel stays open.

import { appQuit, settingsSet } from '$lib/ipc/commands';

import { confirms } from './confirm.svelte';

let asking = false;

export async function askQuit({ live, busy }: { live: number; busy: string[] }): Promise<void> {
  if (asking) return; // Cmd+Q again while the dialog is open
  asking = true;
  try {
    const { id, checked } = await confirms.askChecked({
      title: 'Quit Kelta?',
      body:
        live === 1
          ? '1 session is running. Keep it running in the background (it is back on the next launch), or stop it?'
          : `${live} sessions are running. Keep them running in the background (they are back on the next launch), or stop them all?`,
      details: busy.map((name) => `${name} is busy`),
      checkbox: "Don't ask again",
      actions: [
        { id: 'stop', label: 'Stop all sessions', variant: 'danger' },
        { id: 'keep', label: 'Keep sessions running', variant: 'primary' },
      ],
    });
    if (id !== 'keep' && id !== 'stop') return;
    if (checked) await settingsSet({ layer: 'global', path: 'app.quit_sessions', value: id });
    await appQuit({ stop: id === 'stop' });
  } finally {
    asking = false;
  }
}
