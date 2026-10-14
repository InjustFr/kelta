<script lang="ts">
  // The floating key panel: the prefix's which-key panel (WhichKey) and a list's `?` panel (#437).
  // A `chord` key is drawn as Kbd parts: alternatives split by a space, a sequence by `then`.
  import type { Snippet } from 'svelte';

  import { t } from '$lib/i18n/index.svelte';
  import { Kbd } from '$lib/ui';

  interface Line {
    key: string;
    chord?: boolean;
    label: string;
  }

  interface Props {
    label: string;
    groups: readonly { id: string; title: string; lines: readonly Line[] }[];
    head?: Snippet;
    foot: Snippet;
    [attr: `data-${string}`]: string;
  }

  let { label, groups, head, foot, ...rest }: Props = $props();
</script>

<div class="wk" role="status" aria-label={label} {...rest}>
  {#if head}<div class="head">{@render head()}</div>{/if}
  <div class="cols">
    {#each groups as group (group.id)}
      <section data-group={group.id}>
        <h5>{group.title}</h5>
        <dl>
          {#each group.lines as l (l.key + l.label)}
            <dt class="key">
              {#if l.chord}{#each l.key.split(' ') as part (part)}{#if part === 'then'}<span class="then"
                      >{t('then')}</span
                    >{:else}<Kbd chord={part} />{/if}{/each}{:else}{l.key}{/if}
            </dt>
            <dd>{l.label}</dd>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
  <div class="foot">{@render foot()}</div>
</div>

<style>
  .wk {
    position: absolute;
    left: 50%;
    bottom: calc(var(--k-statusbar-height) + var(--k-space-3));
    transform: translateX(-50%);
    z-index: var(--k-z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    max-width: calc(100% - 2 * var(--k-space-5));
    max-height: 70%;
    overflow: auto;
    padding: var(--k-space-4) var(--k-space-5);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    pointer-events: none;
  }

  .head {
    display: flex;
    gap: var(--k-space-3);
    align-items: center;
  }

  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-4) var(--k-space-6);
  }

  h5 {
    margin: 0 0 var(--k-space-2);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-strong);
  }

  dl {
    display: grid;
    grid-template-columns: auto auto;
    gap: var(--k-space-1) var(--k-space-3);
    margin: 0;
  }

  dd {
    margin: 0;
    white-space: nowrap;
  }

  .key {
    display: inline-flex;
    gap: var(--k-space-1);
    justify-content: center;
    min-width: 18px;
    color: var(--k-accent);
    font-family: var(--k-font-mono);
  }

  .then {
    padding: 0 2px;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  /* The foot's buttons (a list's `? All keys`) take clicks; the rest lets them through. */
  .foot {
    display: flex;
    align-items: center;
    gap: var(--k-space-5);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    pointer-events: auto;
  }
</style>
