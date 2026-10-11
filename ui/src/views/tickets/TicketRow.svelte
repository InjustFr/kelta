<script lang="ts">
  import type { TicketItem } from '$lib/gen';
  import { Icon, Lamp, relativeTime } from '$lib/ui';

  import { ciGlyph, initials } from '../work/common';
  import type { Phase } from '../work/phase';
  import { waitChip } from '../people/waiting';
  import { ageDays, ageLevel, type GroupBy } from './group';
  import { mainPr, prLabel, prLamp, reviewWord } from './prs';
  import RowButton from './RowButton.svelte';
  import StatusChip from './StatusChip.svelte';

  // One ticket row (DESIGN §6.8): a grid on fixed tracks every row shares, so key, title, status, age
  // and source form straight columns whatever a row carries. Secondary text lives in the tooltip.
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
  const prWords = $derived(pr ? `${ciGlyph(pr.ci).label}, ${reviewWord(pr).toLowerCase()}` : '');
  const wait = $derived(waitChip(item.waiting.filter((w) => w.why !== 'assigned')));
  const age = $derived(ageLevel(t, Date.now()));
  const phaseText = $derived(phase ? (phase.id === 'run' && phase.detail ? phase.detail : phase.label) : '');
  /** The signals in words: the lamp's tooltip once the signals column drops (narrow lists). */
  const signals = $derived(
    [
      phaseText,
      pr ? `${prLabel(pr)}: ${prWords}` : '',
      wait?.label ?? '',
      marked ? 'Marked: Mod+Enter starts the marked tickets' : '',
    ].filter(Boolean),
  );
  const tooltip = $derived(
    [
      t.title,
      t.sprint && groupBy !== 'sprint'
        ? `Sprint: ${t.sprint.name}${t.sprint.active ? ' (current)' : ''}`
        : '',
      ...signals,
      `Updated ${relativeTime(Date.parse(t.updated_at))}`,
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
  class:has-source={source !== null}
  class:has-who={showAssignee}
  aria-current={selected ? 'true' : undefined}
  aria-pressed={picking ? picked : undefined}
  data-key={rowKey}
  title={tooltip}
  onclick={onselect}
  ondblclick={onopen}
>
  <span class="k-row-lamp c-lamp"
    >{#if picked}<Icon name="check" size={12} />{:else}<Lamp
        level={phase?.lamp ?? 'none'}
        title={signals.join('\n') || undefined}
      />{/if}</span
  >
  <span class="k-row-key c-key">{t.ref.key}</span>
  <span class="c-title"
    ><span class="text">{t.title}</span><span class="acts" data-acts>
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
  <!-- Fixed slots: work, pull request, waiting, marked. Icons with the words for tooltips and readers. -->
  <span class="c-signals">
    <span class="sig">
      {#if phase}<span
          class="sig-icon"
          class:error={phase.lamp === 'error'}
          class:ask={phase.lamp === 'needs_input'}
          data-phase={phase.id}
          title={phaseText}
          ><Icon
            name={phase.lamp === 'error' ? 'circle-alert' : phase.id === 'elsewhere' ? 'bot' : 'git-branch'}
            size={14}
          /><span class="k-visually-hidden">{phase.label}</span></span
        >{/if}
    </span>
    <span class="sig sig-pr">
      {#if pr}<span class="pr" data-pr
          ><RowButton label={`Open ${prLabel(pr)} (p)`} onclick={onpr}
            ><Icon name="git-pull-request" size={14} /><span class="k-visually-hidden">{prLabel(pr)}</span
            ><Lamp level={prLamp(pr)} title={prWords} /></RowButton
          ></span
        >{/if}
    </span>
    <span class="sig">
      {#if wait}<span class="sig-icon" class:late={wait.late} title={wait.title}
          ><Icon name="clock" size={14} /><span class="k-visually-hidden">{wait.label}</span></span
        >{/if}
    </span>
    <span class="sig">
      {#if marked}<span class="sig-icon marked" title="Marked: Mod+Enter starts the marked tickets"
          ><Icon name="layers" size={14} /><span class="k-visually-hidden">marked</span></span
        >{/if}
    </span>
  </span>
  <span class="c-status" data-status
    ><RowButton label={`${t.status.name}: move ${t.ref.key} (m)`} onclick={onmove}
      ><StatusChip status={t.status} /></RowButton
    ></span
  >
  <span class="c-age"
    >{#if age}<span
        class="aged {age}"
        data-age={age}
        title={`${ageDays(t, Date.now())} days in ${t.status.name}`}>{ageDays(t, Date.now())}d</span
      >{/if}</span
  >
  {#if source !== null}<span class="c-source">{source}</span>{/if}
  {#if showAssignee}<span class="c-who"
      ><span class="k-avatar" title={t.assignee?.name ?? 'Unassigned'}
        >{t.assignee ? initials(t.assignee.name) : '–'}</span
      ></span
    >{/if}
</button>

<style>
  /* Tracks: lamp | key | title | signals | status | age | source | who. Each cell carries its own left
     padding inside its track, so a dropped track takes no gap with it. The key track fits the longest
     key in view (`--key-chars`, set by the list; a mono ch is 0.6em). */
  .tk {
    --tk-key: calc(var(--key-chars, 8) * 0.6 * var(--k-font-size-sm) + 12px);
    --tk-signals: 90px; /* 8 + slots 16 28 16 16 + 3 gaps */
    --tk-status: 112px;
    --tk-age: 44px;
    --tk-source: 0px;
    --tk-who: 0px;
    display: grid;
    grid-template-columns:
      16px var(--tk-key) minmax(0, 1fr) var(--tk-signals) var(--tk-status) var(--tk-age)
      var(--tk-source) var(--tk-who);
    grid-template-areas: 'lamp key title signals status age source who';
    /* One baseline for every text cell; the track is centred in the row, marks are centred on it. */
    align-items: baseline;
    align-content: center;
    gap: 0;
  }

  .tk.has-source {
    --tk-source: 124px;
  }

  .tk.has-who {
    --tk-who: 32px;
  }

  .tk > * {
    min-width: 0;
  }

  .c-lamp {
    grid-area: lamp;
    align-self: center;
    width: auto;
  }

  .c-key {
    grid-area: key;
    width: auto;
    padding-left: var(--k-space-2);
  }

  .c-title {
    grid-area: title;
    position: relative;
    display: block;
    padding-left: var(--k-space-3);
  }

  .text {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .c-signals {
    grid-area: signals;
    align-self: center;
    display: grid;
    grid-template-columns: 16px 28px 16px 16px;
    gap: var(--k-space-1);
    padding-left: var(--k-space-3);
    color: var(--k-fg-subtle);
  }

  .sig {
    display: inline-flex;
    align-items: center;
    height: 20px;
  }

  .sig-icon {
    display: inline-flex;
  }

  .sig-icon.error {
    color: var(--k-lamp-error);
  }

  .sig-icon.ask,
  .sig-icon.late {
    color: var(--k-warn);
  }

  .sig-icon.marked {
    color: var(--k-accent);
  }

  .pr :global(.rb) {
    gap: var(--k-space-1);
    height: 20px;
    padding: 0 var(--k-space-1);
    color: var(--k-fg-subtle);
  }

  .c-status {
    grid-area: status;
    padding-left: var(--k-space-4);
    overflow: hidden;
  }

  .c-status :global(.rb) {
    max-width: 100%;
  }

  .c-age {
    grid-area: age;
    padding-left: var(--k-space-3);
    text-align: right;
  }

  /* Status age: quiet at 7 days, --k-warn at 14, --k-danger at 21; text tint only (DESIGN §6.11). */
  .aged {
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .aged.warn {
    color: var(--k-warn);
  }

  .aged.danger {
    color: var(--k-danger);
  }

  .c-source {
    grid-area: source;
    padding-left: var(--k-space-5);
    padding-right: var(--k-space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .c-who {
    grid-area: who;
    align-self: center;
    display: flex;
    justify-content: flex-end;
  }

  .c-who .k-avatar {
    width: 20px;
    height: 20px;
    font-size: 10px;
  }

  /* Narrow lists: source and who drop first (the split view at 1440 wide leaves the list ~670px, and
     a 60-character title needs 45 shown there), then the age, then the signals fold into the lamp's
     tooltip. Key, title and status never drop. */
  @container (width < 720px) {
    .tk,
    .tk.has-source,
    .tk.has-who {
      --tk-source: 0px;
      --tk-who: 0px;
    }

    .c-source,
    .c-who {
      display: none;
    }
  }

  @container (width < 480px) {
    .tk {
      --tk-age: 0px;
    }

    .c-age {
      display: none;
    }
  }

  @container (width < 420px) {
    .tk {
      --tk-signals: 0px;
    }

    .c-signals {
      display: none;
    }
  }

  /* Compact row actions: on the selected row, on hover and on focus-within (DESIGN §7), laid over
     the end of the title on the row's own backing so showing them never reflows the row. */
  .acts {
    position: absolute;
    /* the title cell is one line tall: reach the row's full height */
    inset: calc((var(--k-line-height) * 1em - var(--k-row-height)) / 2) 0
      calc((var(--k-line-height) * 1em - var(--k-row-height)) / 2) auto;
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
