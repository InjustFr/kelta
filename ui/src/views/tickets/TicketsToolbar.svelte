<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { tick } from 'svelte';

  import type { TicketSort, TicketsMode, Who } from '$lib/gen';
  import { Button, Icon, IconButton, relativeTime, Select, Tabs } from '$lib/ui';

  import TagFilter from '../tags/TagFilter.svelte';
  import { GROUP_BYS, SORTS, type GroupBy } from './group';

  // The Tickets toolbar (DESIGN §6.8), in two groups. Left, what you look at: source, then whose
  // tickets. Right, how it is arranged: one View menu (person, sprint, grouping, sort), the list/board
  // toggle and Refresh. The text filter is in the shared filter bar below (#370). The pane owns the
  // state and the keys; this only shows them.
  interface Props {
    /** "All projects" title (the rail's Tickets view). */
    allProjects: boolean;
    /** Who tabs, person and arrangement need a source. */
    hasSource: boolean;
    who: { id: Who; label: string; title?: string }[];
    whoTab: Who | null;
    onwho: (w: Who) => void;
    people: { value: string; label: string }[];
    person: string;
    onperson: (id: string | null) => void;
    showSprint: boolean;
    currentSprint: boolean;
    onsprint: () => void;
    /** Source button text; `null` hides it (no choice to offer). */
    sourceText: string | null;
    sourceBtn?: HTMLElement;
    sourceOpen: boolean;
    onsource: () => void;
    /** Group and sort apply to the list only. */
    list: boolean;
    groupBy: GroupBy;
    ongroup: (g: GroupBy) => void;
    sort: TicketSort;
    onsort: (s: TicketSort) => void;
    /** The detail column beside the list (Space); `null` when the pane has no room for it. */
    split: boolean | null;
    onsplit: () => void;
    /** List/board toggle: one source in effect. */
    canBoard: boolean;
    mode: TicketsMode;
    onmode: (m: TicketsMode) => void;
    onrefresh: () => void;
    /** When the list was last fetched (#372). */
    fetchedAt: number | null;
    /** The list's ticket keys and the tag chip picked (#349). */
    tagKeys: readonly string[];
    tag: string | null;
    ontag: (name: string | null) => void;
  }

  let {
    allProjects,
    hasSource,
    who,
    whoTab,
    onwho,
    people,
    person,
    onperson,
    showSprint,
    currentSprint,
    onsprint,
    sourceText,
    sourceBtn = $bindable(),
    sourceOpen,
    onsource,
    list,
    groupBy,
    ongroup,
    sort,
    onsort,
    split,
    onsplit,
    canBoard,
    mode,
    onmode,
    onrefresh,
    fetchedAt,
    tagKeys,
    tag,
    ontag,
  }: Props = $props();

  const GROUP_LABELS: Record<GroupBy, string> = $derived({
    flow: t('By flow'),
    status: t('By status'),
    priority: t('By priority'),
    sprint: t('By sprint'),
    assignee: t('By assignee'),
    source: t('By source'),
    parent: t('By epic or parent'),
    none: t('No grouping'),
  });
  const SORT_LABELS: Record<TicketSort, string> = $derived({
    priority: t('Sort by priority'),
    updated: t('Sort by updated'),
    age: t('Sort by age in status'),
    key: t('Sort by key'),
  });

  const hasPeople = $derived(people.length > 1);
  const hasView = $derived(hasSource && (hasPeople || showSprint || list));
  /** What narrows the list from inside the menu: the button says so while the menu is closed. */
  const narrowed = $derived(
    [
      person ? people.find((p) => p.value === person)?.label : '',
      currentSprint ? t('current sprint') : '',
    ].filter(Boolean),
  );

  let viewOpen = $state(false);
  let viewBox = $state<HTMLElement>();
  let viewBtn = $state<HTMLButtonElement>();

  async function toggleView(): Promise<void> {
    viewOpen = !viewOpen;
    await tick();
    if (viewOpen) viewBox?.querySelector<HTMLElement>('select, button')?.focus();
  }

  function closeView(focusButton: boolean): void {
    viewOpen = false;
    if (focusButton) viewBtn?.focus();
  }
</script>

<header class="k-toolbar bar">
  {#if allProjects}<span class="title">{t('All projects')}</span>{/if}
  {#if sourceText !== null}
    <button
      type="button"
      class="source"
      bind:this={sourceBtn}
      aria-haspopup="menu"
      aria-expanded={sourceOpen}
      title={t('Source (v)')}
      onclick={onsource}
    >
      {sourceText}
      <Icon name="chevron-down" size={12} />
    </button>
  {/if}
  {#if hasSource}
    <Tabs label={t('Whose tickets')} items={who} value={whoTab ?? undefined} onchange={onwho} />
  {/if}
  <TagFilter kind="tickets" keys={tagKeys} value={tag} onchange={ontag} />
  <span class="spacer"></span>
  {#if hasView}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="view"
      onfocusout={(e) => {
        if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) closeView(false);
      }}
      onkeydown={(e) => {
        if (e.key === 'Escape' && viewOpen) {
          e.stopPropagation();
          closeView(true);
        }
      }}
    >
      <button
        type="button"
        class="source"
        class:on={narrowed.length > 0}
        bind:this={viewBtn}
        aria-haspopup="dialog"
        aria-expanded={viewOpen}
        aria-label={narrowed.length > 0
          ? t('View, narrowed to {what}', { what: narrowed.join(` ${t('and')} `) })
          : t('View')}
        title={t('Person, sprint, grouping (Shift+G) and sort')}
        onclick={() => void toggleView()}
      >
        {t('View')}{#if narrowed.length > 0}<span class="dot" aria-hidden="true"></span>{/if}
        <Icon name="chevron-down" size={12} />
      </button>
      {#if viewOpen}
        <div class="pop" role="dialog" aria-label={t('View')} bind:this={viewBox}>
          {#if hasPeople}
            <Select
              label={t('Person')}
              value={person}
              options={people}
              onchange={(id) => onperson(id === '' ? null : id)}
            />
          {/if}
          {#if list}
            <Select
              label={t('Group (Shift+G)')}
              value={groupBy}
              options={GROUP_BYS.map((g) => ({ value: g, label: GROUP_LABELS[g] }))}
              onchange={ongroup}
            />
            <Select
              label={t('Sort')}
              value={sort}
              options={SORTS.map((x) => ({ value: x, label: SORT_LABELS[x] }))}
              onchange={onsort}
            />
          {/if}
          {#if showSprint}
            <Button
              variant="ghost"
              size="sm"
              icon={currentSprint ? 'check' : 'square'}
              aria-pressed={currentSprint}
              title={t('Only tickets in an active sprint (f, then s)')}
              onclick={onsprint}>{t('Current sprint')}</Button
            >
          {/if}
          {#if split !== null}
            <Button
              variant="ghost"
              size="sm"
              icon={split ? 'check' : 'square'}
              aria-pressed={split}
              title={t('The selected ticket on the right (Space)')}
              onclick={onsplit}>{t('Detail')}</Button
            >
          {/if}
        </div>
      {/if}
    </div>
  {/if}
  {#if canBoard}
    <Tabs
      label={t('Display mode')}
      items={[
        { id: 'list', label: t('List'), icon: 'list' },
        { id: 'board', label: t('Board'), icon: 'kanban' },
      ]}
      value={mode}
      onchange={onmode}
    />
  {/if}
  {#if fetchedAt}
    <!-- shortcut: the age is computed on render (no timers), it ages with the next re-render. -->
    <span class="asof" data-testid="tickets-asof">{t('Updated {ago}', { ago: relativeTime(fetchedAt) })}</span
    >
  {/if}
  <IconButton icon="refresh-cw" label={t('Refresh')} chord="shift+r" size="sm" onclick={onrefresh} />
</header>

<style>
  /* A narrow pane wraps the toolbar to a second line rather than hiding controls; labels never wrap. */
  .bar {
    position: relative;
    flex-wrap: wrap;
    row-gap: var(--k-space-2);
    height: auto;
    min-height: var(--k-toolbar-height);
    padding-block: var(--k-space-2);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .asof {
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }

  .source {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    height: var(--k-control-height-sm);
    padding: 0 var(--k-space-3);
    border: 0;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  .source:hover,
  .source[aria-expanded='true'] {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .source.on {
    color: var(--k-fg);
  }

  /* The View menu narrows the list: one accent dot says so without opening it. */
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--k-accent);
  }

  .view {
    position: relative;
  }

  .pop {
    position: absolute;
    top: calc(100% + var(--k-space-2));
    right: 0;
    z-index: var(--k-z-menu);
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--k-space-2);
    min-width: 220px;
    padding: var(--k-space-3);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
  }

  /* In the menu the selects are full width; their labels stay for screen readers only (.k-toolbar). */
  .pop :global(.k-select),
  .pop :global(.k-select select) {
    width: 100%;
  }

  .pop :global(.k-button) {
    justify-content: flex-start;
  }
</style>
