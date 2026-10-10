<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { AccountError, KeltaError } from '$lib/gen';
  import { Button, Icon, relativeTime } from '$lib/ui';

  import { accountErrorBanner, loadErrorBanner, type BannerInfo } from '../common';

  interface Props {
    /** Data comes from the cache. */
    stale?: boolean;
    fetchedAt?: number | null;
    /** The last refresh failed (stale data is still shown). */
    error?: KeltaError | null;
    /** Per-account failures next to aggregated data. */
    errors?: AccountError[];
    onretry?: () => void;
  }

  let { stale = false, fetchedAt = null, error = null, errors = [], onretry }: Props = $props();

  const banners = $derived<BannerInfo[]>([
    ...(error ? [loadErrorBanner(error)] : []),
    ...errors.map((e) => accountErrorBanner(e)),
  ]);

  function reauth(): void {
    void dispatch('settings.open', { section: 'accounts' });
  }
</script>

{#if stale || banners.length > 0}
  <div class="banners" data-testid="state-banners">
    {#if stale}
      <div class="banner info" role="status">
        <Icon name="clock" size={14} />
        <span>
          Showing cached data{fetchedAt ? `, updated ${relativeTime(fetchedAt)}` : ''}.
        </span>
        {#if onretry}<Button size="sm" variant="ghost" onclick={onretry}>Retry</Button>{/if}
      </div>
    {/if}
    {#each banners as b, i (i)}
      <div class="banner {b.tone}" role="alert">
        <Icon name="triangle-alert" size={14} />
        <span>{b.text}</span>
        {#if b.reauth}<Button size="sm" variant="ghost" onclick={reauth}>Re-authenticate</Button>{/if}
        {#if !b.reauth && onretry}<Button size="sm" variant="ghost" onclick={onretry}>Retry</Button>{/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .banners {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .banner {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    padding: var(--k-space-1) var(--k-space-3);
    font-size: var(--k-font-size-sm);
    background: var(--k-bezel-raised);
    color: var(--k-fg-muted);
  }

  .banner span {
    flex: 1;
  }

  .warn {
    color: var(--k-warn);
  }

  .danger {
    color: var(--k-danger);
  }
</style>
