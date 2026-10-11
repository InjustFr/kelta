// Startup prompt for the encrypted secrets file (SPEC: secret backends).
import { t } from '$lib/i18n/index.svelte';
import type { Settings } from '$lib/gen';
import { secretBackendsStatus, secretUnlock } from '$lib/ipc/commands';
import { toasts } from '$lib/stores';

import { prompts } from './confirm.svelte';

/** Returns a callback for the first settings value of the run: when an account reads its token
 * from the encrypted file and that file exists but is locked, it asks for the passphrase once
 * (a miss can be retried in Settings → Accounts). Later settings changes never prompt. */
export function startupUnlock(): (value: Settings | null) => void {
  let asked = false;
  return (value) => {
    if (asked || !value) return;
    asked = true;
    if (Object.values(value.accounts ?? {}).some((a) => a?.secret?.startsWith('file:'))) void askUnlock();
  };
}

async function askUnlock(): Promise<void> {
  const file = (await secretBackendsStatus().catch(() => [])).find((b) => b.backend === 'encrypted-file');
  // A missing file ("not set up") is created from Settings → Accounts, not unlocked here.
  if (!file?.detail?.startsWith('locked')) return;
  const passphrase = await prompts.ask({
    title: t('Unlock secrets'),
    label: t('Passphrase of the encrypted secrets file'),
    type: 'password',
    confirmLabel: t('Unlock'),
  });
  if (!passphrase) return;
  await secretUnlock({ passphrase, create: false }).catch((err: unknown) =>
    toasts.error(err, t('Unlocking the secrets file failed')),
  );
}
