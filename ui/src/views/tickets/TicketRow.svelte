<script lang="ts">
  import type { TicketItem } from '$lib/gen';
  import { Badge, Icon, Lamp, relativeTime } from '$lib/ui';

  import { initials } from '../work/common';
  import type { Phase } from '../work/phase';
  import { waitChip } from '../people/waiting';
  import { ageDays, ageLevel, type GroupBy } from './group';
  import PrChip from './PrChip.svelte';
  import { mainPr, prLabel } from './prs';
  import RowButton from './RowButton.svelte';
  import StatusChip from './StatusChip.svelte';

  // One ticket row of the Tickets list (DESIGN §6.8): a button inside VirtualList.
  interface Props {
    item: TicketItem;
    rowKey: string;
    selected: boolean;
    picked: boolean;
    /** A multi-selection exists: the row says whether it is in it. */
    picking: boolean;
    phase: Phase | null;
    hasWork: boolean;
    marked: boolean;
    groupBy: GroupBy;
    /** The source label, when the list spans several sources. */
    source: string | null;
    showAssignee: boolean;
    onselect: (e: MouseEvent) => void;
    onopen: () => void;
    onmove: () => void;
    onpr: () => void;
    onstart: () => void;
  }

  let {
    item,
    rowKey,
    selected,
    picked,
    picking,
    phase,
    hasWork,
    marked,
    groupBy,
    source,
    showAssignee,
    onselect,
    onopen,
    onmove,
    onpr,
    onstart,
  }: Props = $props();

  const t = $derived(item.ticket);
  const pr = $derived(mainPr(item.prs));
  const wait = $derived(waitChip(item.waiting.filter((w) => w.why !== 'assigned')));
  const age = $derived(ageLevel(t, Date.now()));
</script>

<button
  type="button"
  tabindex="-1"
  class="k-row"
  class:selected
  class:picked
  aria-current={selected ? 'true' : undefined}
  aria-pressed={picking ? picked : undefined}
  data-key={rowKey}
  onclick={onselect}
  ondblclick={onopen}
>
  <span class="k-row-lamp"
    >{#if picked}<Icon name="check" size={12} />{:else}<Lamp
        level={phase?.lamp ?? 'none'}
        title={phase?.label}
      />{/if}</span
  >
  <span class="k-row-key">{t.ref.key}</span>
  <span class="k-row-title"
    >{t.title}<span class="acts" data-acts>
      <RowButton icon="arrow-right" label="Move (m)" onclick={onmove} />
      <RowButton
        icon="git-pull-request"
        label={item.prs.length > 0 ? 'Open pull request (p)' : 'No pull request linked'}
        disabled={item.prs.length === 0}
        onclick={onpr}
      />
      <RowButton icon="play" label={hasWork ? 'Resume work (s)' : 'Start work (s)'} onclick={onstart} /></span
    ></span
  >
  <span class="meta">
    {#if marked}<Badge tone="accent" title="Marked: Mod+Enter starts the marked tickets">marked</Badge>{/if}
    {#if phase}<span
        class="k-row-meta"
        data-phase={phase.id}
        title={phase.id === 'run' ? phase.detail : undefined}>{phase.label}</span
      >{/if}
    {#if pr}<span class="pr" data-pr
        ><RowButton label={`Open ${prLabel(pr)} (p)`} onclick={onpr}><PrChip {pr} /></RowButton></span
      >{/if}
    {#if wait}<Badge tone="neutral" title={wait.title}>{wait.label}</Badge>{/if}
    {#if t.sprint && groupBy !== 'sprint'}
      <span
        class="sprint"
        class:active={t.sprint.active}
        title={t.sprint.active ? `${t.sprint.name} (current)` : t.sprint.name}>{t.sprint.name}</span
      >
    {/if}
    <span class="status-slot" data-status
      ><RowButton label={`${t.status.name}: move ${t.ref.key} (m)`} onclick={onmove}
        ><StatusChip status={t.status} /></RowButton
      ></span
    >
    <span class="age-slot"
      >{#if age}<span
          class="aged {age}"
          data-age={age}
          title={`${ageDays(t, Date.now())} days in ${t.status.name}`}>{ageDays(t, Date.now())}d</span
        >{/if}</span
    >
    {#if source}<span class="source-slot"><Badge>{source}</Badge></span>{/if}
    {#if showAssignee}
      <span class="k-avatar" title={t.assignee?.name ?? 'Unassigned'}
        >{t.assignee ? initials(t.assignee.name) : '–'}</span
      >
    {/if}
    <!-- An aged row shows one duration (days in status); the empty slot keeps the column. -->
    <span class="k-row-meta age">{age ? '' : relativeTime(Date.parse(t.updated_at))}</span>
  </span>
</button>

<style>
  /* Fixed width so the category bars form one column down the list. */
  .status-slot {
    display: inline-flex;
    flex: none;
    width: 112px;
  }

  .status-slot :global(span) {
    max-width: 100%;
  }

  /* Fixed too, so source names of any length do not shift the status column. */
  .source-slot {
    display: inline-flex;
    flex: none;
    justify-content: flex-end;
    width: 88px;
  }

  .source-slot :global(.k-badge) {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The title is the row: the meta shrinks first, then drops the source and avatar at split widths. */
  .k-row-title {
    position: relative;
    min-width: 120px;
  }

  @container (width < 560px) {
    .source-slot,
    .sprint,
    .meta .k-avatar {
      display: none;
    }
  }

  .meta {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-4);
  }

  .pr {
    display: inline-flex;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .age {
    min-width: 52px;
    text-align: right;
  }

  /* Status age: quiet at 7 days, --k-warn at 14, --k-danger at 21; text tint only (DESIGN §6.11). */
  .age-slot {
    display: inline-flex;
    flex: none;
    justify-content: flex-end;
    width: 28px;
  }

  .aged {
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
  }

  .aged.warn {
    color: var(--k-warn);
  }

  .aged.danger {
    color: var(--k-danger);
  }

  .sprint {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .sprint.active {
    color: var(--k-fg-muted);
  }

  /* Compact row actions: on the selected row, on hover and on focus-within (DESIGN §7), laid over
     the end of the title on the row's own backing so showing them never reflows the row. */
  .acts {
    position: absolute;
    inset: 0 0 0 auto;
    display: none;
    align-items: center;
    gap: var(--k-space-1);
    padding-left: var(--k-space-2);
    background: linear-gradient(var(--k-bg-hover), var(--k-bg-hover)), var(--k-well);
  }

  .k-row.selected .acts,
  .k-row.picked .acts {
    background: var(--k-bg-selected);
  }

  .k-row:hover .acts,
  .k-row.selected .acts,
  .k-row:focus-within .acts {
    display: inline-flex;
  }

  .k-row.picked {
    background: var(--k-bg-selected);
  }

  .k-row.picked .k-row-lamp {
    color: var(--k-accent);
  }
</style>
