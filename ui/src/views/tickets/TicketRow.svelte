<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { TicketItem } from '$lib/gen';
  import { inflight } from '$lib/stores/inflight.svelte';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Icon, relativeTime, Spinner } from '$lib/ui';

  import { waitChip } from '../people/waiting';
  import { batch } from '../work/batch.svelte';
  import { initials } from '../work/common';
  import StageTrack from '../work/shared/StageTrack.svelte';
  import { fieldText } from './fields';
  import { ageDays, type GroupBy } from './group';
  import RowButton from './RowButton.svelte';
  import { prState, type Standing } from './standing';
  import TagChips from '../tags/TagChips.svelte';

  // One ticket row (#400): key, the title on up to two lines with the epic and the PR state under it,
  // Now's stage track (#399) and the days in status. The status is the group's; actions live in the pane.
  interface Props {
    item: TicketItem;
    rowKey: string;
    selected: boolean;
    picked: boolean;
    /** A multi-selection exists: the row says whether it is in it. */
    picking: boolean;
    standing: Standing;
    groupBy: GroupBy;
    /** The source label, when the list spans several sources. */
    source: string | null;
    showAssignee: boolean;
    onselect: (e: MouseEvent) => void;
    onopen: () => void;
    /** The checkbox shown while a multi-selection exists. */
    onpick: () => void;
  }

  let {
    item,
    rowKey,
    selected,
    picked,
    picking,
    standing,
    groupBy,
    source,
    showAssignee,
    onselect,
    onopen,
    onpick,
  }: Props = $props();

  /** Ages past two weeks stand out. */
  const STALE = 14;
  const tk = $derived(item.ticket);
  const pr = $derived(prState(item));
  /** No age without the tracker's status date, nor once done. */
  const days = $derived(tk.status_since && tk.status.category !== 'done' ? ageDays(tk, Date.now()) : null);
  /** The epic by name (#400), not by key; Redmine sends no title, so the key then. */
  const epic = $derived(groupBy !== 'parent' && tk.parent ? tk.parent.title || tk.parent.key : null);
  const tooltip = $derived(
    [
      tk.title,
      tk.sprint && groupBy !== 'sprint'
        ? tk.sprint.active
          ? t('Sprint: {name} (current)', { name: tk.sprint.name })
          : t('Sprint: {name}', { name: tk.sprint.name })
        : '',
      standing.where,
      waitChip(item.waiting.filter((w) => w.why !== 'assigned'))?.label ?? '',
      t('Updated {ago}', { ago: relativeTime(Date.parse(tk.updated_at)) }),
    ]
      .filter(Boolean)
      .join('\n'),
  );
</script>

<button
  type="button"
  tabindex="-1"
  class="k-row tk"
  class:selected
  class:picked
  class:picking
  class:has-source={source !== null}
  class:has-who={showAssignee}
  aria-current={selected ? 'true' : undefined}
  aria-pressed={picking ? picked : undefined}
  data-key={rowKey}
  title={tooltip}
  onclick={onselect}
  ondblclick={onopen}
>
  {#if picking}<span class="c-pick"
      ><RowButton label={picked ? t('Deselect') : t('Select')} onclick={onpick}
        ><span class="box" class:on={picked} data-pick
          ><Icon name={picked ? 'check' : 'square'} size={12} /></span
        ></RowButton
      ></span
    >{/if}
  <span class="k-row-key c-key"
    >{#if inflight.has(ticketKey(tk.ref))}<Spinner size={10} />{/if}{tk.ref.key}</span
  >
  <span class="c-what">
    <span class="text"
      >{#if tk.blocked}<span class="blocked" data-blocked>{t('Blocked')}</span>{/if}{tk.title}</span
    >
    <span class="sub">
      {#if batch.has(tk.ref)}<span
          class="marked"
          data-marked
          title={t('Marked: Mod+Enter starts the marked tickets')}>{t('marked')}</span
        >{/if}
      {#if epic}<span class="epic" data-epic>{epic}</span>{/if}
      <span class="pr" class:bad={pr?.bad} data-pr>{pr?.text ?? t('No PR found')}</span>
      {#each tk.fields ?? [] as f, i (i)}<span class="field" data-field title={fieldText(f)}
          >{fieldText(f)}</span
        >{/each}
      <TagChips kind="tickets" itemKey={rowKey} />
    </span>
  </span>
  {#if source !== null}<span class="c-source">{source}</span>{/if}
  {#if showAssignee}<span class="c-who"
      ><span class="k-avatar" title={tk.assignee?.name ?? t('Unassigned')}
        >{tk.assignee ? initials(tk.assignee.name) : '–'}</span
      ></span
    >{/if}
  <span class="c-stage"><StageTrack stage={standing.stage} named={selected} /></span>
  <span
    class="c-age"
    class:stale={days !== null && days > STALE}
    data-age
    title={days === null ? undefined : t('{n} days in {status}', { n: days, status: tk.status.name })}
    >{days === null ? '—' : t('{n}d', { n: days })}</span
  >
</button>

<style>
  /* Tracks: [pick] key | title | source | who | stage | age, placed from the end; source and who are
     0px when the row has none. Each cell after the key carries its own left padding: an empty track
     takes no gap. The key track fits the longest key in view (`--key-chars`, set by the list; a
     mono ch is 0.6em). The list's header shares the last two tracks. */
  .tk {
    --tk-key: calc(var(--key-chars, 8) * 0.6 * var(--k-font-size-sm) + 12px);
    --tk-src: 0px;
    --tk-who: 0px;
    --tk-tail: var(--tk-src) var(--tk-who) var(--tk-stage) var(--tk-age);
    display: grid;
    grid-template-columns: var(--tk-key) minmax(0, 1fr) var(--tk-tail);
    gap: 0;
    align-items: center;
    height: auto;
    min-height: 56px;
    padding-block: var(--k-space-2);
    padding-inline: var(--k-space-4);
    white-space: normal;
    --track-bg: var(--k-well);
  }

  .tk.has-source {
    --tk-src: 140px;
  }

  .tk.has-who {
    --tk-who: 48px;
  }

  .tk.picking {
    grid-template-columns: 24px var(--tk-key) minmax(0, 1fr) var(--tk-tail);
  }

  .c-what {
    grid-column: -6 / -5;
  }

  .c-source {
    grid-column: -5 / -4;
  }

  .c-who {
    grid-column: -4 / -3;
  }

  .c-stage {
    grid-column: -3 / -2;
  }

  .c-age {
    grid-column: -2 / -1;
  }

  .c-what,
  .c-source,
  .c-who,
  .c-stage,
  .c-age {
    padding-left: var(--k-space-4);
  }

  .tk.selected,
  .tk.picked {
    --track-bg: var(--k-bg-selected-row);
  }

  .tk > * {
    min-width: 0;
  }

  .c-key {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    width: auto;
  }

  .c-what {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* Titles wrap to two lines (#400), as in Now. */
  .text {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    font-weight: var(--k-weight-medium);
    color: var(--k-fg-strong);
  }

  .k-row.selected .text {
    font-weight: var(--k-weight-strong);
  }

  /* The source's blocked marker (#348), ahead of the title. */
  .blocked {
    font-weight: normal;
    margin-right: var(--k-space-2);
    padding: 0 var(--k-space-1);
    border: 1px solid var(--k-danger);
    border-radius: var(--k-radius-sm);
    color: var(--k-danger);
    font-size: var(--k-font-size-xs);
  }

  .sub {
    display: flex;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-4);
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .epic,
  .pr {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .epic {
    flex: 0 1 auto;
  }

  .pr {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .marked {
    flex: none;
    color: var(--k-accent);
  }

  .pr.bad {
    color: var(--k-danger);
  }

  /* The source's chosen tracker fields (#351): they give way first, ellipsised. */
  .field {
    flex: 0 1 auto;
    min-width: 4ch;
    max-width: 20ch;
    overflow: hidden;
    padding: 0 var(--k-space-1);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-hover);
    color: var(--k-fg-muted);
    text-overflow: ellipsis;
  }

  .c-age {
    text-align: right;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .c-age.stale {
    color: var(--k-warn);
    font-weight: var(--k-weight-strong);
  }

  .c-source {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .c-who {
    display: flex;
    justify-content: flex-end;
  }

  .c-who .k-avatar {
    width: 20px;
    height: 20px;
    font-size: 10px;
  }

  /* Narrow lists: source, who and the field chips drop, then the track goes under the title. */
  @container (width < 720px) {
    .tk.has-source,
    .tk.has-who {
      --tk-src: 0px;
      --tk-who: 0px;
    }

    .c-source,
    .c-who,
    .field {
      display: none;
    }
  }

  @container (width < 560px) {
    .tk,
    .tk.picking {
      grid-template-columns: var(--tk-key) minmax(0, 1fr) var(--tk-age);
      row-gap: var(--k-space-1);
    }

    .c-pick {
      display: none;
    }

    .c-what {
      grid-column: 2;
      grid-row: 1;
    }

    .c-stage {
      grid-column: 2 / 4;
      grid-row: 2;
    }

    .c-age {
      grid-column: 3;
      grid-row: 1;
    }
  }

  .k-row.picked {
    background: var(--k-bg-selected-row);
    --k-fg-subtle: var(--k-fg-subtle-sel);
    --k-fg-muted: var(--k-fg-muted-sel);
  }

  .box {
    display: inline-flex;
    color: var(--k-fg-muted);
  }

  .box.on {
    color: var(--k-accent);
  }
</style>
