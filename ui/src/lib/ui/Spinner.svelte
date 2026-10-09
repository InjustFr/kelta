<script lang="ts">
  interface Props {
    size?: number;
    label?: string;
  }

  let { size = 16, label = 'Loading' }: Props = $props();
</script>

<!-- Only mounted while an operation is in flight (no idle animations, ARCH §9.2). -->
<span class="k-spinner" role="status" aria-label={label} style:width="{size}px" style:height="{size}px"
></span>

<style>
  .k-spinner {
    display: inline-block;
    border: 2px solid var(--k-border-strong);
    border-top-color: var(--k-accent);
    border-radius: 50%;
    animation: k-spin 0.8s linear infinite;
    flex: none;
  }

  @keyframes k-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
