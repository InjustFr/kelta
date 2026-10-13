<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  interface Props {
    size?: number;
    label?: string;
  }

  let { size = 16, label }: Props = $props();
</script>

<!-- Only mounted while an operation is in flight: the open `working` ring turns until it unmounts (DESIGN §8). -->
<span
  class="k-spinner"
  role="status"
  aria-label={label ?? t('Loading')}
  style:width="{size}px"
  style:height="{size}px"
></span>

<style>
  .k-spinner {
    display: inline-block;
    border: 1.5px solid var(--k-lamp-working);
    border-top-color: transparent;
    border-radius: 50%;
    flex: none;
    animation: k-spin 0.8s linear infinite;
  }
</style>
