// Mark my draft PR ready for review, or convert it back to a draft (#353): My PRs, the review
// detail and the work menu.
import type { Review } from '$lib/gen';
import { t } from '$lib/i18n/index.svelte';
import { reviewSetDraft } from '$lib/ipc/commands';
import { reviews, settings, toasts } from '$lib/stores';

import { confirms } from '../../shell/confirm.svelte';

export const draftLabel = (r: Review): string =>
  r.draft ? t('Mark ready for review…') : t('Convert to draft');

/** Why `r`'s draft state cannot change from Kelta, or null. */
export function draftBlocked(r: Review): string | null {
  if (r.kind !== 'authored') return t('Only on your own pull requests');
  // Plugin code hosts have no draft call (`CodeHost::set_draft`); the built-in ones all do.
  return settings.value()?.accounts[r.ref.account]?.kind === 'plugin_codehost'
    ? t('This code host cannot change the draft state')
    : null;
}

/** Toggles `r`'s draft state; Mark ready asks first, since the host then requests the reviews. */
export async function toggleDraft(r: Review): Promise<void> {
  const label = draftLabel(r);
  const why = draftBlocked(r);
  if (why) return void toasts.info(t('{label}: {why}', { label, why }));
  if (r.draft) {
    const answer = await confirms.ask({
      title: t('Mark #{n} ready for review?', { n: r.ref.number }),
      body: t('The code host then asks the required reviewers to review it.'),
      actions: [{ id: 'ready', label: t('Mark ready'), variant: 'primary' }],
    });
    if (answer !== 'ready') return;
  }
  try {
    await reviewSetDraft({ review: r.ref, draft: !r.draft });
    toasts.info(
      r.draft
        ? t('#{n} is ready for review', { n: r.ref.number })
        : t('#{n} is a draft again', { n: r.ref.number }),
    );
    void reviews.loadDetail(r.ref);
  } catch (err) {
    toasts.error(err, label);
  }
}
