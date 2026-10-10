<script lang="ts">
  // The shell root: project rail, tab bar, layout renderer, status bar, palette, project switcher,
  // sheets, dialogs and toasts (SPEC §1, BUILD_PLAN §2.4).
  import { onMount } from 'svelte';

  import type { AnyComponent } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { onUiEvent } from '$lib/ipc/events';
  import { sessionSpawnTemplate, terminalSetPalette } from '$lib/ipc/commands';
  import { keyManager } from '$lib/keys';
  import { layout, projects, reviews, sessions, settings, toasts, tools, ui } from '$lib/stores';
  import { configFromSettings, terminalPool } from '$lib/terminal';
  import { terminalPalette } from '$lib/terminal/theme';
  import { Button, currentPlatform, EmptyState } from '$lib/ui';

  import { confirms, prompts } from './confirm.svelte';
  import DialogHost from './DialogHost.svelte';
  import JumpHud from './JumpHud.svelte';
  import InboxHost from './InboxHost.svelte';
  import { noteFocus } from './jumplist';
  import PeekPopover from './PeekPopover.svelte';
  import { lazyComponents } from './lazy.svelte';
  import { projectAccent } from './accent';
  import { chordFor } from './labels';
  import { fileRoots, openFileLink } from './actions';
  import { activateProject, currentTab, focusedPane, focusedSessionId } from './nav';
  import ProjectRail from './ProjectRail.svelte';
  import SheetHost from './SheetHost.svelte';
  import StatusBar from './StatusBar.svelte';
  import TabBar from './TabBar.svelte';
  import ToastHost from './ToastHost.svelte';
  import WindowChrome from './WindowChrome.svelte';
  import { startupUnlock } from './unlock';
  import Workspace from './Workspace.svelte';

  let prefixArmed = $state(false);
  let systemDark = $state(
    typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)').matches : true,
  );

  // ---- theme ---------------------------------------------------------------------------------

  const themeSetting = $derived(settings.value()?.app.theme ?? 'system');
  const resolvedTheme = $derived<'dark' | 'light'>(
    themeSetting === 'system' ? (systemDark ? 'dark' : 'light') : themeSetting,
  );
  const terminalConfig = $derived(configFromSettings(settings.value()?.terminal, resolvedTheme));

  $effect(() => {
    const root = document.documentElement;
    if (themeSetting === 'system') delete root.dataset.theme;
    else root.dataset.theme = themeSetting;
  });

  $effect(() => {
    // The window takes the hue of the active project; the terminal palette is not re-pushed.
    const accent = projectAccent(projects.active?.color);
    const style = document.documentElement.style;
    if (accent) style.setProperty('--k-project', accent);
    else style.removeProperty('--k-project');
  });

  $effect(() => {
    // The Rust model answers OSC 4/10/11/12 queries from this palette: push it at startup and on
    // every theme change.
    const palette = terminalPalette(resolvedTheme);
    terminalSetPalette({ palette }).catch(() => {});
  });

  // ---- terminal views --------------------------------------------------------------------------

  terminalPool.configure({
    config: () => terminalConfig,
    platform: currentPlatform(),
    kindOf: (id) => sessions.get(id)?.kind.type ?? 'shell',
    onKey: (event) => keyManager.handleKeyDown(event) === 'consumed',
    onError: (err, context) => toasts.error(err, context),
    fileRoots,
    openFile: (id, path, line, focusEditor) => void openFileLink(id, path, line, focusEditor),
  });

  $effect(() => {
    terminalPool.applyConfig(terminalConfig);
  });

  $effect(() => {
    terminalPool.setCapacity(settings.value()?.terminal.max_live_views ?? 2);
  });

  // ---- encrypted secrets file ------------------------------------------------------------------

  const unlockOnce = startupUnlock();
  $effect(() => unlockOnce(settings.value()));

  // ---- layouts ---------------------------------------------------------------------------------

  $effect(() => {
    // Layouts of every open project are loaded ahead of time so a project switch is warm.
    for (const p of projects.openProjects) {
      if (!layout.get(p.id)) void layout.ensure(p.id).catch(() => {});
    }
  });

  // ---- jumplist -------------------------------------------------------------------------------

  $effect(() => {
    // Every focus change, wherever it came from; `nav.back` landing on an entry records nothing.
    const project = projects.activeId;
    const tab = ui.inboxActive ? null : currentTab();
    const pane = tab ? focusedPane() : null;
    if (project && tab && pane) noteFocus({ project, tab: tab.id, pane: pane.id });
  });

  // ---- keys ------------------------------------------------------------------------------------

  $effect(() => {
    // Rebind when `[keys]` or the tool keybindings change.
    void settings.value()?.keys;
    void tools.byProject;
    keyManager.refresh();
  });

  // ---- overlays ----------------------------------------------------------------------------------

  $effect(() => {
    if (ui.overlay === 'palette')
      lazyComponents.ensure('overlay:palette', () => import('./palette/Palette.svelte'));
    if (ui.overlay === 'switcher')
      lazyComponents.ensure('overlay:switcher', () => import('./palette/Switcher.svelte'));
  });

  const Palette = $derived(lazyComponents.get('overlay:palette').component as AnyComponent | null);
  const Switcher = $derived(lazyComponents.get('overlay:switcher').component as AnyComponent | null);

  let overlayWasOpen = false;
  $effect(() => {
    const open =
      ui.overlay !== null || ui.sheets.length > 0 || confirms.current !== null || prompts.current !== null;
    if (overlayWasOpen && !open) {
      // Give the keyboard back to the focused terminal.
      queueMicrotask(() => {
        const pane = focusedPane();
        const id = focusedSessionId();
        if (pane && id) terminalPool.focus(id);
      });
    }
    overlayWasOpen = open;
  });

  onMount(() => {
    const stopKeys = keyManager.start();
    const offPrefix = keyManager.onPrefixChange((armed) => (prefixArmed = armed));
    const offRemoved = onUiEvent('session.removed', (ev) => terminalPool.release(ev.id));
    const offOpen = onUiEvent('ui.open', (ev) => {
      // The backend asks for a pane: the layout store applies it to a loaded layout; an unloaded
      // layout is fetched first. Focus requests also bring the project to the front.
      const id = ev.project_id;
      if (!layout.get(id)) {
        layout
          .ensure(id)
          .then(() => layout.open(id, ev.request))
          .catch((err: unknown) => toasts.error(err, 'Opening the pane failed'));
      }
      if (ev.request.focus) void activateProject(id);
    });
    const offCtl = ui.onCtl((cmd) => {
      if (cmd.cmd === 'focus_project') void activateProject(cmd.id);
      else if (cmd.cmd === 'next') void dispatch('attention.next');
      else if (cmd.cmd === 'back') void dispatch('nav.back');
      else if (cmd.cmd === 'emit' && cmd.name === 'custom.bench.run')
        import('./bench')
          .then((b) => b.runScenario(cmd.payload))
          .catch((err: unknown) => toasts.error(err, 'Bench scenario failed'));
      else if (cmd.cmd === 'new') {
        const project = cmd.project ?? projects.activeId;
        if (!project) return;
        sessionSpawnTemplate({
          project_id: project,
          template_id: cmd.template,
          ctx: {
            repo_id: null,
            cwd: cmd.cwd,
            session_id: null,
            work_item_id: null,
            ticket: null,
            review: null,
            extra: {},
          },
          placement: 'new_tab',
        })
          .then((created) => {
            for (const s of created) sessions.upsert(s);
            void activateProject(project);
          })
          .catch((err: unknown) => toasts.error(err, 'Starting the session failed'));
      }
    });

    const mql = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
    const onScheme = (e: MediaQueryListEvent): void => {
      systemDark = e.matches;
    };
    mql?.addEventListener('change', onScheme);
    const onFocus = (): void => void (ui.focused = true);
    const onBlur = (): void => void (ui.focused = false);
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);

    // Cached review requests feed the Inbox badge (no network before first paint: cache only).
    void reviews.load({ kind: 'all' }, 'review_requested', false);

    return () => {
      stopKeys();
      offPrefix();
      offRemoved();
      offOpen();
      offCtl();
      mql?.removeEventListener('change', onScheme);
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    };
  });

  const active = $derived(projects.active);
</script>

<WindowChrome />
<main class="shell" data-testid="shell" data-theme-resolved={resolvedTheme}>
  <ProjectRail />
  <div class="main">
    {#if ui.inboxActive}
      <InboxHost />
    {:else if active}
      <TabBar projectId={active.id} />
      <Workspace projectId={active.id} />
    {:else if projects.loaded}
      <EmptyState
        icon="folder"
        title="No project open"
        body="Pick a project in the sidebar, or create one from a code folder."
      >
        {#snippet actions()}
          <Button variant="primary" onclick={() => ui.openSheet('project_new')}
            >Create project from folder</Button
          >
          <Button chord={chordFor('project.switcher')} onclick={() => ui.openOverlay('switcher')}
            >Switch project</Button
          >
        {/snippet}
      </EmptyState>
    {:else}
      <div class="grow"></div>
    {/if}
    <StatusBar {prefixArmed} inbox={ui.inboxActive} />
  </div>

  {#if ui.overlay === 'palette' && Palette}
    <Palette initialQuery={ui.paletteQuery} onclose={() => ui.closeOverlay()} />
  {:else if ui.overlay === 'switcher' && Switcher}
    <Switcher onclose={() => ui.closeOverlay()} />
  {/if}
  <SheetHost />
  <DialogHost />
  <ToastHost />
  <JumpHud />
  <PeekPopover />
</main>

<style>
  .shell {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
    min-width: 0;
    background: var(--k-bezel);
  }

  .main {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .grow {
    flex: 1;
  }
</style>
