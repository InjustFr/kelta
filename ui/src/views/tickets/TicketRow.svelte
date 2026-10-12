<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { TicketItem } from '$lib/gen';
  import { Icon, Lamp, relativeTime } from '$lib/ui';

  import { ciGlyph, initials } from '../work/common';
  import { phaseWord, type Phase } from '../work/phase';
  import { waitChip } from '../people/waiting';
  import { fieldText } from './fields';
  import { ageDays, ageLevel, type GroupBy } from './group';
  import { mainPr, prLabel, reviewWord } from './prs';
  import RowButton from './RowButton.svelte';
  import StatusChip from './StatusChip.svelte';
  import TagChips from '../tags/TagChips.svelte';

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
    /** What Enter runs on this row (the phase's next step), null without work under way. */
    next: string | null;
    hasWork: boolean;
    marked: boolean;
    groupBy: GroupBy;
    /** The source label, when the list spans several sources. */
    source: string | null;
    showAssignee: boolean;
    onselect: (e: MouseEvent) => void;
    onopen: () => void;
    onnext: () => void;
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
    next,
    hasWork,
    marked,
    groupBy,
    source,
    showAssignee,
    onselect,
    onopen,
    onnext,
    onmove,
    onpr,
    onstart,
  }: Props = $props();

  const tk = $derived(item.ticket);
  const pr = $derived(mainPr(item.prs));
  const ci = $derived(pr ? ciGlyph(pr.ci) : null);
  const prWords = $derived(pr && ci ? `${ci.label}, ${reviewWord(pr).toLowerCase()}` : '');
  const wait = $derived(waitChip(item.waiting.filter((w) => w.why !== 'assigned')));
  const age = $derived(ageLevel(tk, Date.now()));
  const phaseText = $derived(phase ? (phase.id === 'run' && phase.detail ? phase.detail : phase.label) : '');
  const word = $derived(phase ? phaseWord(phase) : '');
  /** The signals in words: the lamp's tooltip once the signals column drops (narrow lists). */
  const signals = $derived(
    [
      phaseText,
      pr ? `${prLabel(pr)}: ${prWords}` : '',
      wait?.label ?? '',
      marked ? t('Marked: Mod+Enter starts the marked tickets') : '',
    ].filter(Boolean),
  );
  const tooltip = $derived(
    [
      tk.title,
      tk.parent && groupBy !== 'parent'
        ? t('Parent: {name}', { name: [tk.parent.key, tk.parent.title].filter(Boolean).join(' · ') })
        : '',
      tk.sprint && groupBy !== 'sprint'
        ? tk.sprint.active
          ? t('Sprint: {name} (current)', { name: tk.sprint.name })
          : t('Sprint: {name}', { name: tk.sprint.name })
        : '',
      ...signals,
      t('Updated {ago}', { ago: relativeTime(Date.parse(tk.updated_at)) }),
      // The row shows two actions (#384): the others by key.
      t('Keys: s start work, m move, p open pull request, t tags'),
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
  <span class="k-row-key c-key">{tk.ref.key}</span>
  <span class="c-title"
    ><span class="text"
      >{#if tk.blocked}<span class="blocked" data-blocked>{t('Blocked')}</span
        >{/if}{#if tk.parent && groupBy !== 'parent'}<span class="parent" data-parent>{tk.parent.key}</span
        >{/if}{tk.title}</span
    >{#each tk.fields ?? [] as f, i (i)}<span class="field" data-field title={fieldText(f)}
        >{fieldText(f)}</span
      >{/each}<TagChips kind="tickets" itemKey={rowKey} /><span class="acts" data-acts
      >{#if next}<RowButton label={next} onclick={onnext}
          ><span class="act primary" data-next>{next}</span></RowButton
        >{:else}<RowButton label={hasWork ? t('Resume work (s)') : t('Start work (s)')} onclick={onstart}
          ><span class="act primary"
            ><Icon name="play" size={14} />{hasWork ? t('Resume work') : t('Start work')}</span
          ></RowButton
        >{/if}<RowButton label={t('Move (m)')} onclick={onmove}
        ><span class="act"><Icon name="arrow-right" size={14} />{t('Move')}</span></RowButton
      ></span
    ></span
  >
  <!-- Fixed slots: work, pull request, waiting, marked. Icons, and words where the list has room (#373). -->
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
        >{#if word}<span class="word" data-phase-word data-lamp={phase.lamp} aria-hidden="true">{word}</span
          >{/if}{/if}
    </span>
    <span class="sig sig-pr">
      {#if pr && ci}<span class="pr" class:draft={pr.state === 'open' && pr.draft} data-pr
          ><RowButton label={t('Open {name} (p)', { name: prLabel(pr) })} onclick={onpr}
            ><span class="pr-icon"><Icon name="git-pull-request" size={14} /></span><span class="num"
              >{prLabel(pr)}</span
            ><Lamp level={ci.lamp} title={ci.label} /><span class="word" aria-hidden="true"
              >{reviewWord(pr)}</span
            ></RowButton
          ></span
        >{/if}
    </span>
    <span class="sig">
      {#if wait}<span class="sig-icon" class:late={wait.late} title={wait.title}
          ><Icon name="clock" size={14} /><span class="k-visually-hidden">{wait.label}</span></span
        >{/if}
    </span>
    <span class="sig">
      {#if marked}<span class="sig-icon marked" title={t('Marked: Mod+Enter starts the marked tickets')}
          ><Icon name="layers" size={14} /><span class="k-visually-hidden">{t('marked')}</span></span
        >{/if}
    </span>
  </span>
  <span class="c-status" data-status
    ><RowButton
      label={t('{status}: move {key} (m)', { status: tk.status.name, key: tk.ref.key })}
      onclick={onmove}><StatusChip status={tk.status} source={item.source_ids[0]} /></RowButton
    ></span
  >
  <span class="c-age"
    >{#if age}<span
        class="aged {age}"
        data-age={age}
        title={t('{n} days in {status}', { n: ageDays(tk, Date.now()), status: tk.status.name })}
        >{t('{n}d', { n: ageDays(tk, Date.now()) })}</span
      >{/if}</span
  >
  {#if source !== null}<span class="c-source">{source}</span>{/if}
  {#if showAssignee}<span class="c-who"
      ><span class="k-avatar" title={tk.assignee?.name ?? t('Unassigned')}
        >{tk.assignee ? initials(tk.assignee.name) : '–'}</span
      ></span
    >{/if}
</button>

<style>
  /* Tracks: lamp | key | title | signals | status | age | source | who. Each cell carries its own left
     padding inside its track, so a dropped track takes no gap with it. The key track fits the longest
     key in view (`--key-chars`, set by the list; a mono ch is 0.6em). */
  .tk {
    --tk-key: calc(var(--key-chars, 8) * 0.6 * var(--k-font-size-sm) + 12px);
    --tk-signals: 90px; /* 8 + slots 16 28 16 16 + 3 gaps; the words widen the first two */
    --tk-phase: 16px;
    --tk-pr: 28px;
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

  /* The title, then its local tags (#349): the title gives way first. */
  .c-title {
    grid-area: title;
    position: relative;
    display: flex;
    align-items: baseline;
    padding-left: var(--k-space-3);
  }

  .text {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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

  .parent {
    display: inline-block;
    font-weight: normal;
    max-width: 16ch;
    overflow: hidden;
    margin-right: var(--k-space-2);
    padding: 0 var(--k-space-1);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-hover);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    text-overflow: ellipsis;
    vertical-align: bottom;
    white-space: nowrap;
  }

  /* The source's chosen tracker fields (#351), after the title: they give way with it, ellipsised. */
  .field {
    flex: 0 1 auto;
    min-width: 4ch;
    max-width: 20ch;
    overflow: hidden;
    margin-left: var(--k-space-1);
    padding: 0 var(--k-space-1);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-hover);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .c-signals {
    grid-area: signals;
    align-self: center;
    display: grid;
    grid-template-columns: var(--tk-phase) var(--tk-pr) 16px 16px;
    gap: var(--k-space-1);
    padding-left: var(--k-space-3);
    color: var(--k-fg-subtle);
  }

  .sig {
    display: inline-flex;
    align-items: center;
    min-width: 0;
    height: 20px;
  }

  .sig-icon {
    display: inline-flex;
  }

  .sig .word {
    display: none;
    min-width: 0;
    margin-left: var(--k-space-1);
    overflow: hidden;
    font-size: var(--k-font-size-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The phase word takes its lamp's colour (#382), so the state reads without the tooltip. */
  .word[data-lamp='needs_input'] {
    color: var(--k-lamp-needs-input);
  }

  .word[data-lamp='working'] {
    color: var(--k-lamp-working);
  }

  .word[data-lamp='error'] {
    color: var(--k-lamp-error);
  }

  .word[data-lamp='done'] {
    color: var(--k-lamp-done);
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

  .pr {
    min-width: 0;
  }

  .pr :global(.rb) {
    gap: var(--k-space-1);
    max-width: 100%;
    height: 20px;
    padding: 0 var(--k-space-1);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
  }

  .pr .word {
    margin-left: 0;
  }

  .pr .num {
    display: none;
  }

  .pr-icon {
    display: inline-flex;
  }

  /* A draft PR reads as one at every width: a dashed outline round its number (#373). */
  .pr.draft :global(.rb) {
    outline: 1px dashed var(--k-border-strong);
    outline-offset: -1px;
  }

  /* Row actions read as buttons (#384): outlined, icon and word; the next step's outline is the accent. */
  .acts :global(.rb) {
    flex: none;
  }

  /* Only the step's label gives way, ellipsised, once the actions reach their share of the title cell. */
  .acts :global(.rb:has(.primary)) {
    flex: 0 1 auto;
    min-width: 0;
  }

  .act {
    display: block;
    line-height: 18px;
    overflow: hidden;
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border-strong);
    border-radius: var(--k-radius-sm);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .act :global(svg) {
    margin-right: var(--k-space-1);
    vertical-align: -2px;
  }

  .act.primary {
    border-color: var(--k-accent);
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

  /* Wide lists show the signals in words (#373): the phase and the PR number from 800px, the PR state
     from 1040px. The split view's list (~710px at 1440 wide) keeps the icons and its title width. */
  @container (width >= 800px) {
    .tk {
      --tk-phase: 84px;
      --tk-pr: 52px;
      --tk-signals: 182px;
    }

    .pr :global(.rb) {
      gap: var(--k-space-2);
    }

    .sig .word,
    .pr .num {
      display: block;
    }

    .pr .word,
    .pr-icon {
      display: none;
    }
  }

  @container (width >= 1040px) {
    .tk {
      --tk-pr: 156px;
      --tk-signals: 290px;
    }

    .pr .word {
      display: block;
    }
  }

  /* Narrow lists: source, who and the field chips drop first (the split view at 1440 wide leaves the list ~670px, and
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
    .c-who,
    .field {
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

  /* Row actions on the selected row only (#384), after the title and its chips: the title gives way to
     them instead of being covered. */
  .acts {
    display: none;
    flex: none;
    align-items: center;
    gap: var(--k-space-1);
    max-width: 60%;
    margin-left: auto;
    padding-left: var(--k-space-2);
  }

  .k-row.selected .acts {
    display: inline-flex;
  }

  .k-row.picked {
    background: var(--k-bg-selected);
  }

  .k-row.picked .k-row-lamp {
    color: var(--k-accent);
  }
</style>
