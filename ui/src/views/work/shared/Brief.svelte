<script lang="ts" module>
  export interface BriefAction {
    key: string;
    label: string;
    run: () => unknown;
  }

  export interface Fact {
    label: string;
    value: string;
    tone?: 'warn' | 'danger';
  }
</script>

<script lang="ts">
  // The selected item's brief in a side panel (#399): what it is and where it stands, the primary action
  // filled, the two likely next ones, the rest behind "N more actions" (`.`), then Claude's last message
  // and the facts. Now uses it; Tickets and Reviews reuse it (#400, #401).
  import type { Snippet } from 'svelte';

  import { t, tn } from '$lib/i18n/index.svelte';
  import { Button, Kbd, Spinner } from '$lib/ui';

  interface Props {
    id?: string;
    title: string;
    /** Where it stands, in a sentence. */
    where?: string;
    /** Filled, first. */
    primary?: BriefAction | null;
    /** In likely order: the first two show, the rest wait behind "N more actions". */
    actions?: BriefAction[];
    /** The rest are shown (`.` toggles it). */
    more?: boolean;
    message?: string;
    facts?: Fact[];
    /** A run under way: its button spins. */
    busy?: boolean;
    testid?: string;
    /** Under `where`, before the actions (Reviews: the branch; the Queue: Claude's question). */
    sub?: Snippet;
    /** Under Claude's message (the Queue: the reply field, #414). */
    reply?: Snippet;
    /** After the facts (Reviews: review status, change map, description). */
    children?: Snippet;
  }

  let {
    id,
    title,
    where = '',
    primary = null,
    actions = [],
    more = $bindable(false),
    message = '',
    facts = [],
    busy = false,
    testid = 'brief',
    sub,
    reply,
    children,
  }: Props = $props();

  const SHOWN = 2;
  const rest = $derived(actions.length - SHOWN);

  // The message folds after 6 lines (#414); unfolded only for the message it was opened on.
  let openFor = $state<string | null>(null);
  const expanded = $derived(openFor === message);
  let overflows = $state(false);
  let msgEl = $state<HTMLElement>();
  $effect(() => {
    void message;
    if (msgEl && !expanded) overflows = msgEl.scrollHeight > msgEl.clientHeight + 1;
  });
</script>

<aside class="brief" aria-label={title} data-testid={testid}>
  <div class="head">
    {#if id}<span class="id">{id}</span>{/if}
    <h3>{title}</h3>
    {#if where}<p class="where">{where}</p>{/if}
    {@render sub?.()}
  </div>
  {#if primary || actions.length}
    <div class="actions">
      {#if primary}
        {@const a = primary}
        <button type="button" class="btn primary" data-key={a.key} onclick={() => void a.run()}
          ><span
            >{#if busy}<Spinner size={10} />{/if}{a.label}</span
          ><span aria-hidden="true"><Kbd chord={a.key} /></span></button
        >
      {/if}
      {#each more ? actions : actions.slice(0, SHOWN) as a (a.key)}
        <button type="button" class="btn" data-key={a.key} onclick={() => void a.run()}
          ><span>{a.label}</span><span aria-hidden="true"><Kbd chord={a.key} /></span></button
        >
      {/each}
      {#if !more && rest > 0}
        <button type="button" class="btn more" aria-expanded="false" onclick={() => (more = true)}
          ><span>{tn('{n} more action', '{n} more actions', rest)}</span><span aria-hidden="true"
            ><Kbd chord="." /></span
          ></button
        >
      {/if}
    </div>
  {/if}
  {#if message}
    <blockquote>
      <cite>{t('Claude, last message')}</cite>
      <span class="msg" class:clamped={!expanded} bind:this={msgEl}>{message}</span>
      {#if overflows || expanded}
        <Button size="sm" variant="ghost" onclick={() => (openFor = expanded ? null : message)}
          >{expanded ? t('Show less') : t('Show more')}</Button
        >
      {/if}
    </blockquote>
  {/if}
  {@render reply?.()}
  {#if facts.length}
    <dl class="facts">
      {#each facts as f (f.label)}
        <dt>{f.label}</dt>
        <dd class={f.tone ?? ''}>{f.value}</dd>
      {/each}
    </dl>
  {/if}
  {@render children?.()}
</aside>

<style>
  .brief {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    min-width: 0;
    padding: var(--k-space-4);
    background: var(--k-bg-elev);
    color: var(--k-fg);
    font-size: var(--k-font-size);
  }

  .id {
    display: block;
    margin-bottom: var(--k-space-1);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  h3 {
    margin: 0;
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-strong);
    text-wrap: balance;
    overflow-wrap: anywhere;
  }

  .where {
    margin: var(--k-space-1) 0 0;
    color: var(--k-fg-muted);
  }

  .actions {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
  }

  .btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
    width: 100%;
    padding: var(--k-space-1) var(--k-space-2);
    min-height: 30px;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg);
    color: var(--k-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .btn > span {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    min-width: 0;
  }

  .btn:hover {
    border-color: var(--k-border-strong);
  }

  .btn:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: 1px;
  }

  .primary {
    border-color: var(--k-accent);
    background: var(--k-accent);
    color: var(--k-accent-fg);
    font-weight: var(--k-weight-strong);
  }

  .primary:hover {
    border-color: var(--k-accent);
    filter: brightness(1.1);
  }

  /* The key cap follows the text colour, at full strength (#399: dimmed caps fell under 3:1). */
  .btn :global(.k-kbd) {
    flex: none;
    color: inherit;
    border-color: currentColor;
  }

  .more {
    border-style: dashed;
    color: var(--k-fg-muted);
  }

  blockquote {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    border-left: 2px solid var(--k-accent);
    border-radius: 0 var(--k-radius-sm) var(--k-radius-sm) 0;
    background: var(--k-bg);
  }

  cite {
    display: block;
    margin-bottom: var(--k-space-1);
    font-style: normal;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .msg {
    display: block;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .msg.clamped {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 6;
    line-clamp: 6;
    overflow: hidden;
  }

  .facts {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px var(--k-space-4);
    margin: 0;
  }

  dt {
    color: var(--k-fg-muted);
  }

  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }

  dd.warn {
    color: var(--k-warn);
  }

  dd.danger {
    color: var(--k-danger);
  }
</style>
