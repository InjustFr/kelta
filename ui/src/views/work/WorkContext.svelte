<script lang="ts">
  // The work tab header above its panes (#428, DESIGN §6.3): key, project and title, the stage track
  // with whose move and why, the primary action and the ⌘. Actions menu. Status, PR, git and cost
  // live in the Story panel's summaries.
  import { t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { effectiveChords } from '$lib/keys/manager';
  import { layout, projects, settings, tickets, work } from '$lib/stores';
  import { terminalPool } from '$lib/terminal';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, currentPlatform, Menu, type MenuItem } from '$lib/ui';
  import { formatChord } from '$lib/ui/format';

  import { focus } from '../../shell/focus.svelte';
  import { chordFor } from '../../shell/labels';
  import { focusedSessionId } from '../../shell/nav';
  import { pluginActions, runPluginAction, workCtx } from '../plugins/actions';
  import { blockedReason, runPrimary, runWorkAction, workMenu, type WorkAction } from './actions';
  import { phaseNow, prOf, workTitle } from './live';
  import { workKey } from './phase';
  import StageTrack from './shared/StageTrack.svelte';
  import { workHeader } from './stage';
  import { story } from './story/panel.svelte';
  import { workUi } from './ui.svelte';

  let { projectId, workItemId }: TabHeaderProps = $props();

  const item = $derived(work.get(workItemId));
  const ticketRef = $derived(item?.ticket ?? null);
  const phase = $derived(item ? phaseNow(item) : null);
  const head = $derived(item && phase ? workHeader(item, phase, prOf(item)) : null);
  const project = $derived(projects.byId(projectId));
  const tabId = $derived(layout.get(projectId)?.tabs.find((t) => t.work_item_id === workItemId)?.id);
  const primaryBlocked = $derived(item && phase?.primary ? blockedReason(phase.primary, item, phase) : null);
  const menuChord = $derived(
    effectiveChords('work.menu', settings.value()?.keys ?? null, currentPlatform())[0],
  );
  // ⌘↵ runs the primary from any pane (#441): the button shows the chord, its tooltip spells it out.
  const startChord = $derived(chordFor('work.start'));
  const primaryTip = $derived(
    startChord && phase
      ? `${phase.primaryLabel} (${formatChord(startChord).join(currentPlatform() === 'macos' ? '' : '+')})`
      : undefined,
  );
  // ⇧⌘F works from any pane; Esc Esc only outside terminals and fields, so it is the tooltip's fallback.
  const focusChord = $derived(chordFor('view.focus'));

  let workBtn = $state<HTMLElement>();

  // The title (here and in the breadcrumb) comes from the ticket's detail.
  $effect(() => {
    const ref = ticketRef;
    if (ref) untrack(() => void tickets.loadDetail(ref));
  });

  // Enabled plugins' work actions (PLUGINS §4): after the built-ins, no letters.
  const plugin = $derived(pluginActions('work'));
  let pluginRunning = $state(false);

  async function runPlugin(commandId: string): Promise<void> {
    if (!item) return;
    pluginRunning = true;
    try {
      await runPluginAction(commandId, workCtx(item));
    } finally {
      pluginRunning = false;
    }
  }

  const menuItems = $derived.by((): MenuItem[] => {
    if (!item || !phase) return [];
    const out = workMenu(item, phase);
    plugin.forEach((a, i) =>
      out.push({ id: `plugin:${a.command_id}`, label: a.title, detail: a.plugin, separator: i === 0 }),
    );
    return out;
  });

  const menuPos = $derived.by(() => {
    if (workUi.menu !== workItemId) return null;
    const r = workBtn?.getBoundingClientRect();
    return { x: r ? r.right - 280 : 40, y: (r?.bottom ?? 40) + 2 };
  });

  function onMenu(id: string): void {
    if (!item) return;
    if (id === 'primary') void runPrimary(item);
    else if (id.startsWith('plugin:')) void runPlugin(id.slice('plugin:'.length));
    else void runWorkAction(id as WorkAction['id'], item);
  }

  /** ⌘. is used from inside Claude or nvim: closing the menu gives the keyboard back. */
  function closeMenu(): void {
    workUi.menu = null;
    const sid = focusedSessionId();
    if (sid) terminalPool.focus(sid);
  }
</script>

{#if item && phase && head}
  <section
    class="work-header"
    aria-label={t('Work item')}
    data-testid="work-header"
    data-phase={phase.id}
    data-mark={head.stage.mark}
  >
    <div class="text">
      <div class="names">
        <span class="key" data-testid="work-header-key">{workKey(item)}</span>
        {#if project}<span class="project" style:--project-color={resolveProjectColor(project.color)}
            >{project.name}</span
          >{/if}
        <span class="ttl">{workTitle(item)}</span>
      </div>
      <button
        type="button"
        class="why"
        title={t('Open the Story panel')}
        data-testid="work-why"
        onclick={() => tabId && story.setOpen(tabId, true)}>{head.why}</button
      >
    </div>
    <span class="stage"><StageTrack stage={head.stage} labels /></span>
    {#if phase.primary}
      <!-- The wrapper carries the blocked reason: a disabled button shows no tooltip in WebKit. -->
      <span class="primary" title={primaryBlocked ?? (phase.id === 'failed' ? phase.detail : primaryTip)}>
        <Button
          size="sm"
          variant="primary"
          disabled={primaryBlocked !== null}
          chord={startChord}
          data-testid="work-primary"
          onclick={() => void runPrimary(item)}
        >
          {phase.primaryLabel}
        </Button>
      </span>
    {/if}
    {#if focus.on}
      <Button
        size="sm"
        variant="ghost"
        aria-label={t('Leave focus')}
        title={`${t('Leave focus')} ${t('or Esc Esc outside a terminal')}`}
        data-testid="work-leave-focus"
        chord={focusChord}
        onclick={() => void focus.leave()}>{t('Leave focus')}</Button
      >
    {/if}
    <span class="menu" bind:this={workBtn}>
      <Button
        size="sm"
        variant="ghost"
        icon="ellipsis"
        aria-haspopup="menu"
        aria-label={t('Actions')}
        title={t('Work menu')}
        data-testid="work-menu-button"
        chord={menuChord}
        loading={pluginRunning}
        onclick={() => (workUi.menu = item.id)}>{t('Actions')}</Button
      >
    </span>
  </section>
{:else}
  <section class="work-header muted" data-testid="work-header">{t('Work item not found')}</section>
{/if}

{#if menuPos}
  <Menu
    items={menuItems}
    x={menuPos.x}
    y={menuPos.y}
    label={t('Work')}
    onselect={onMenu}
    onclose={closeMenu}
  />
{/if}

<style>
  .work-header {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--k-space-4);
    margin: calc(var(--k-gap) / 2) calc(var(--k-gap) / 2) 0;
    padding: var(--k-space-2) var(--k-space-3);
    container: workheader / inline-size;
    border-radius: var(--k-radius-screen);
    background: var(--k-bg);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: var(--k-space-1);
    min-width: 0;
  }

  .names {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-3);
    min-width: 0;
  }

  .key {
    flex: none;
    color: var(--k-fg-muted);
    font-family: var(--k-font-mono);
  }

  .project {
    flex: none;
    padding-left: var(--k-space-2);
    box-shadow: inset 3px 0 0 var(--project-color, var(--k-border-strong));
    color: var(--k-fg-muted);
  }

  .ttl,
  .why {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ttl {
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  .why {
    padding: 0;
    border: 0;
    background: none;
    color: var(--k-fg-muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .why:hover {
    color: var(--k-fg);
  }

  /* Room for the six names in French and German (~240px, #438, DESIGN §6.14); 300px leaves the text block room above the 640px drop. */
  .stage {
    flex: 0 1 300px;
    min-width: 140px;
  }

  .primary,
  .menu {
    display: inline-flex;
    flex: none;
  }

  /* A long primary label ("Retry …") ellipsises instead of pushing Actions out. */
  .primary :global(.k-button) {
    max-width: 28ch;
  }

  .primary :global(.k-button .label) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  /* A narrow pane area keeps the sentence, the primary and ⌘.: the track goes, then the Actions label. */
  @container workheader (max-width: 640px) {
    .stage {
      display: none;
    }
  }

  @container workheader (max-width: 480px) {
    .menu :global(.label),
    .menu :global(.chord) {
      display: none;
    }
  }
</style>
