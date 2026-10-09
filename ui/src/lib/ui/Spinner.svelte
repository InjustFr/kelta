<script lang="ts">
  interface Props {
    size?: number;
    label?: string;
  }

  let { size = 16, label = 'Loading' }: Props = $props();
</script>

<!-- Only mounted while an operation is in flight. Static: the open `working` ring, nothing loops (DESIGN §8). -->
<span class="k-spinner" role="status" aria-label={label} style:width="{size}px" style:height="{size}px"
></span>

<style>
  .k-spinner {
    display: inline-block;
    border: 1.5px solid var(--k-lamp-working);
    border-top-color: transparent;
    border-radius: 50%;
    flex: none;
  }
</style>
