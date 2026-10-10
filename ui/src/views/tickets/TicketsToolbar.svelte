<script lang="ts">
  import type { TicketSort, TicketsMode, Who } from '$lib/gen';
  import { Button, Icon, Select, Tabs } from '$lib/ui';

  import { GROUP_BYS, SORTS, type GroupBy } from './group';

  // The Tickets toolbar (DESIGN §6.8). The pane owns the state and the keys; this only shows them.
  interface Props {
    /** "All projects" title (the rail's Tickets view). */
    allProjects: boolean;
    filter: string;
    filterInput?: HTMLInputElement;
    onfilterkey: (e: KeyboardEvent) => void;
    /** Who tabs, person and arrangement need a source. */
    hasSource: boolean;
    who: { id: Who; label: string }[];
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
    /** List/board toggle: one source in effect. */
    canBoard: boolean;
    mode: TicketsMode;
    onmode: (m: TicketsMode) => void;
    onrefresh: () => void;
  }

  let {
    allProjects,
    filter = $bindable(),
    filterInput = $bindable(),
    onfilterkey,
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
    canBoard,
    mode,
    onmode,
    onrefresh,
  }: Props = $props();

  const GROUP_LABELS: Record<GroupBy, string> = {
    flow: 'By flow',
    status: 'By status',
    priority: 'By priority',
    sprint: 'By sprint',
    assignee: 'By assignee',
    source: 'By source',
    none: 'No grouping',
  };
  const SORT_LABELS: Record<TicketSort, string> = {
    priority: 'Sort by priority',
    updated: 'Sort by updated',
    age: 'Sort by age in status',
    key: 'Sort by key',
  };
</script>

<header class="k-toolbar bar">
  {#if allProjects}<span class="title">All projects</span>{/if}
  <input
    class="k-filter"
    bind:this={filterInput}
    bind:value={filter}
    placeholder="Filter (/)"
    aria-label="Filter tickets"
    onkeydown={onfilterkey}
  />
  {#if hasSource}
    <div class="who">
      <Tabs label="Whose tickets" items={who} value={whoTab ?? undefined} onchange={onwho} />
      {#if people.length > 1}
        <Select
          label="Person"
          value={person}
          options={people}
          onchange={(id) => onperson(id === '' ? null : id)}
        />
      {/if}
    </div>
  {/if}
  {#if showSprint}
    <Button
      variant="ghost"
      size="sm"
      icon={currentSprint ? 'check' : undefined}
      aria-pressed={currentSprint}
      title="Only tickets in an active sprint (f, then s)"
      onclick={onsprint}>Current sprint</Button
    >
  {/if}
  <span class="spacer"></span>
  {#if sourceText !== null}
    <button
      type="button"
      class="source"
      bind:this={sourceBtn}
      aria-haspopup="menu"
      aria-expanded={sourceOpen}
      title="Source (v)"
      onclick={onsource}
    >
      {sourceText}
      <Icon name="chevron-down" size={12} />
    </button>
  {/if}
  {#if list && hasSource}
    <Select
      label="Group (g)"
      value={groupBy}
      options={GROUP_BYS.map((g) => ({ value: g, label: GROUP_LABELS[g] }))}
      onchange={ongroup}
    />
    <Select
      label="Sort"
      value={sort}
      options={SORTS.map((x) => ({ value: x, label: SORT_LABELS[x] }))}
      onchange={onsort}
    />
  {/if}
  {#if canBoard}
    <Tabs
      label="Display mode"
      items={[
        { id: 'list', label: 'List', icon: 'list' },
        { id: 'board', label: 'Board', icon: 'kanban' },
      ]}
      value={mode}
      onchange={onmode}
    />
  {/if}
  <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={onrefresh}>Refresh</Button>
</header>

<style>
  /* A narrow pane wraps the toolbar to a second line rather than hiding controls; labels never wrap. */
  .bar {
    flex-wrap: wrap;
    row-gap: var(--k-space-2);
    height: auto;
    min-height: var(--k-tabbar-height);
    padding-block: var(--k-space-2);
    white-space: nowrap;
  }

  .bar .k-filter {
    flex: 0 1 180px;
    min-width: 72px;
  }

  .who {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    font-variant-numeric: tabular-nums;
  }

  .source {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    height: 22px;
    padding: 0 var(--k-space-3);
    border: 0;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  .source:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }
</style>
