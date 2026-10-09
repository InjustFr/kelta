<script lang="ts">
  import Icon from './Icon.svelte';

  interface Props {
    /** Component or feature name (e.g. "TicketsPane"). */
    name: string;
    /** Owning lane (BUILD_PLAN §3), shown for contributors. */
    owner?: string;
  }

  let { name, owner }: Props = $props();
</script>

<div class="k-not-implemented" data-testid="not-implemented" data-name={name}>
  <Icon name="wrench" size={20} />
  <p>
    <strong>{name}</strong> is not implemented yet{#if owner}&nbsp;({owner}){/if}.
  </p>
</div>

<style>
  .k-not-implemented {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--k-space-3);
    height: 100%;
    min-height: 80px;
    padding: var(--k-space-5);
    color: var(--k-fg-subtle);
    border: 1px dashed var(--k-border-strong);
    border-radius: var(--k-radius);
  }

  p {
    margin: 0;
  }
</style>
