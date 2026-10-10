// In-app updates (#74): the status shared by the restart banner and Settings → General, and the
// `app.restart` action. No UiEvent: the status is read on start, on window focus, on the
// backend's `kelta://update` event and after Check now.

import type { UpdateChannel, UpdateStatus } from '$lib/gen';
import { appRestart, appUpdateCheck, appUpdateStatus } from '$lib/ipc/commands';

import { confirms } from './confirm.svelte';

const RELEASES = 'https://github.com/InjustFr/kelta/releases';

/** "What's new": the release page of the running channel. */
export const releasePage = (channel: UpdateChannel): string =>
  channel === 'nightly' ? `${RELEASES}/tag/nightly` : `${RELEASES}/latest`;

export class UpdatesStore {
  status = $state<UpdateStatus | null>(null);
  checking = $state(false);
  /** Version hidden with Later, for this session. */
  dismissed = $state<string | null>(null);

  /** Version installed and waiting for a restart, unless dismissed. */
  get banner(): string | null {
    const s = this.status?.state;
    return s?.type === 'ready' && s.version !== this.dismissed ? s.version : null;
  }

  async refresh(): Promise<void> {
    this.status = await appUpdateStatus();
  }

  async check(): Promise<void> {
    this.checking = true;
    try {
      this.status = await appUpdateCheck();
    } finally {
      this.checking = false;
    }
  }
}

export const updates = new UpdatesStore();

/** Restarts into the installed build; asks first when a working Claude session would stop. */
export async function restartApp(): Promise<void> {
  const status = await appUpdateStatus();
  if (status.restart_needs_confirm) {
    const answer = await confirms.ask({
      title: 'Restart Kelta?',
      body: 'Claude sessions are working and will stop. Restart anyway?',
      tone: 'danger',
      actions: [{ id: 'restart', label: 'Restart', variant: 'danger' }],
    });
    if (answer !== 'restart') return;
  }
  await appRestart();
}
