<script lang="ts">
  // The rail's global views (Tickets, Now, Reviews): one pane rendered outside any project, with
  // the `GLOBAL` sentinel as its project (SPEC §1). Settings shows the same way, over them (#410).
  import { t } from '$lib/i18n/index.svelte';
  import { paneComponent, type AnyComponent } from '$app/registry';
  import type { PaneContent } from '$lib/gen';
  import { isEditable } from '$lib/keys/manager';
  import { GLOBAL, projects, ui, type GlobalView } from '$lib/stores';
  import { ErrorState, Spinner } from '$lib/ui';

  import { globalContent } from '../views/work/nav';
  import { lazyComponents } from './lazy.svelte';
  import { VIEW_NAMES } from './nav';
  import { listScope } from './scope.svelte';

  let { view }: { view: GlobalView | 'settings' } = $props();

  // Tickets and Reviews show the shared scope (#409); Now narrows its rows to it itself.
  const content = $derived.by((): PaneContent => {
    if (view === 'settings') return { kind: 'settings', section: ui.settings?.section ?? null };
    const c = globalContent(view);
    return c.kind === 'tickets' || c.kind === 'reviews' ? { ...c, scope: listScope.scope } : c;
  });
  const key = $derived(`pane:${content.kind}`);
  const entry = $derived(lazyComponents.get(key));
  const Component = $derived(entry.component as AnyComponent | null);
  const name = $derived(view === 'settings' ? t('Settings') : VIEW_NAMES[view]);
  // Settings' Project layer starts on the project underneath.
  const projectId = $derived(view === 'settings' ? (projects.activeId ?? 'home') : GLOBAL);

  function onkeydown(e: KeyboardEvent): void {
    // Sheets, dialogs, menus and the chord recorder keep their own Esc.
    if (view !== 'settings' || e.key !== 'Escape' || e.defaultPrevented) return;
    // In a field, Esc first leaves it (committing what was typed); the next one closes Settings.
    if (isEditable(e.target)) (e.target as HTMLElement).blur();
    else ui.settings = null;
  }

  $effect(() => {
    const loader = paneComponent(content.kind);
    if (loader) lazyComponents.ensure(key, loader);
  });
</script>

<svelte:window {onkeydown} />

<div class="global" data-testid="global-host" data-view={view}>
  <svelte:boundary onerror={(e) => console.error(`[kelta] ${name} crashed`, e)}>
    {#if Component}
      {#key view}
        <Component {projectId} tabId={GLOBAL} paneId={view} {content} visible={true} focused={true} />
      {/key}
    {:else if entry.error}
      <ErrorState error={entry.error} title={t('Could not load {name}', { name })} />
    {:else}
      <div class="loading"><Spinner size={16} /></div>
    {/if}
    {#snippet failed(error)}
      <ErrorState
        error={error instanceof Error ? error : String(error)}
        title={t('{name} crashed', { name })}
      />
    {/snippet}
  </svelte:boundary>
</div>

<style>
  .global {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: var(--k-bg);
  }

  .loading {
    display: flex;
    justify-content: center;
    padding: var(--k-space-6);
  }
</style>
