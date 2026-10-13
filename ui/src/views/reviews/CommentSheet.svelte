<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import type { ReviewRef } from '$lib/gen';
  import { reviewComment } from '$lib/ipc/commands';
  import { reviews, toasts } from '$lib/stores';

  import ReviewTextDialog from './ReviewTextDialog.svelte';

  // Sheet `reviews.comment` (#417): `c` on someone's PR in the Queue and the Reviews list.
  let { review, onclose }: SheetProps & { review: ReviewRef } = $props();

  async function send(body: string): Promise<void> {
    try {
      await reviewComment({ review, body });
      toasts.info(t('Comment posted'));
      void reviews.loadDetail(review);
    } catch (err) {
      toasts.error(err, t('Comment'));
      throw err;
    }
  }
</script>

<ReviewTextDialog
  title={t('Comment')}
  label={t('Comment (Markdown)')}
  submitLabel={t('Comment')}
  required
  onsubmit={send}
  {onclose}
/>
