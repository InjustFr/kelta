<script lang="ts">
  // The shell root: context bar, project rail, layout renderer, status bar, palette, project switcher,
  // sheets, dialogs and toasts (SPEC §1, BUILD_PLAN §2.4).
  import { onMount } from 'svelte';

  import type { AnyComponent } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { setLocale, t } from '$lib/i18n/index.svelte';
  import { onUiEvent } from '$lib/ipc/events';
  import { appInfo, sessionSpawnTemplate, terminalSetPalette } from '$lib/ipc/commands';
  import { keyManager } from '$lib/keys';
  import {
    GLOBAL,
    layout,
    projects,
    reviews,
    sessions,
    settings,
    toasts,
    tools,
    ui,
    type GlobalView,
  } from '$lib/stores';
  import { findTab } from '$lib/layout';
  import { configFromSettings, terminalPool } from '$lib/terminal';
  import { terminalPalette } from '$lib/terminal/theme';
  import { resolveProjectColor, resolveTheme } from '$lib/theme';
  import { Button, currentPlatform, EmptyState, resolveMotion, setMotion } from '$lib/ui';
  import { startWorkOnTicket } from '../views/work/startWork';

  import { confirms, prompts } from './confirm.svelte';
  import ContextBar from './ContextBar.svelte';
  import { closeDetachedWindow, detached } from './detached';
  import DialogHost from './DialogHost.svelte';
  import JumpHud from './JumpHud.svelte';
  import GlobalHost from './GlobalHost.svelte';
  import { noteFocus } from './jumplist';
  import PeekPopover from './PeekPopover.svelte';
  import { askQuit } from './quit';
  import { lazyComponents } from './lazy.svelte';
  import { projectAccent } from './accent';
  import { chordFor } from './labels';
  import { fileRoots, openFileLink } from './actions';
  import { activateProject, currentTab, focusedPane, focusedSessionId, openView } from './nav';
  import ProjectRail from './ProjectRail.svelte';
  import { sidebar } from './sidebar.svelte';
  import SheetHost from './SheetHost.svelte';
  import StatusBar from './StatusBar.svelte';
  import ToastHost from './ToastHost.svelte';
  import { startupUnlock } from './unlock';
  import UpdateBanner from './UpdateBanner.svelte';
  import Workspace from './Workspace.svelte';
  import { GLOBAL_VIEWS, loadGlobalLayout } from '../views/work/nav';

  let prefixArmed = $state(false);

  // ---- theme ---------------------------------------------------------------------------------

  const themeSetting = $derived(settings.value()?.app.theme ?? 'system');
  const resolvedTheme = $derived(settings.theme);
  const resolved = $derived(resolveTheme(settings.value(), resolvedTheme));
  const terminalConfig = $derived(configFromSettings(settings.value()?.terminal, resolved.terminal));

  // Interface language (#344): `t()` reads it, so the switch applies without a restart.
  $effect(() => {
    void setLocale(settings.value()?.app.language);
  });

  $effect(() => {
    const root = document.documentElement;
    if (themeSetting === 'system') delete root.dataset.theme;
    else root.dataset.theme = themeSetting;
  });

  // app.animations (#398): auto turns to reduced in safe graphics mode, which only the desktop knows.
  let safeGraphics = $state(false);
  appInfo()
    .then((i) => (safeGraphics = i.safe_graphics))
    .catch(() => {});
  $effect(() => setMotion(resolveMotion(settings.value()?.app.animations, safeGraphics)));

  let themeKeys: string[] = [];
  $effect(() => {
    // Theme overrides sit inline on <html> over tokens.css; drop the keys the previous theme set.
    const style = document.documentElement.style;
    for (const key of themeKeys) if (!(key in resolved.vars)) style.removeProperty(key);
    for (const [key, value] of Object.entries(resolved.vars)) style.setProperty(key, value);
    themeKeys = Object.keys(resolved.vars);
    // project colours paint theme slots as var(--k-ansi-N)
    resolved.terminal.ansi.forEach((c, i) => style.setProperty(`--k-ansi-${i}`, c));
  });

  $effect(() => {
    // The window takes the hue of the active project, a detached window its tab's (else the theme's);
    // the palette is not re-pushed.
    const shown = detached ? projects.byId(detached.project_id) : projects.active;
    const accent =
      projectAccent(resolveProjectColor(shown?.color, resolved)) ?? projectAccent(resolved.accent);
    const style = document.documentElement.style;
    if (accent) style.setProperty('--k-project', accent);
    else style.removeProperty('--k-project');
  });

  $effect(() => {
    // The Rust model answers OSC 4/10/11/12 queries from this palette: push it at startup and on
    // every theme change.
    const palette = terminalPalette(resolved.terminal);
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
  $effect(() => {
    if (!detached) unlockOnce(settings.value()); // asked once, in the main window
  });

  // ---- layouts ---------------------------------------------------------------------------------

  $effect(() => {
    // Layouts of every open project are loaded ahead of time so a project switch is warm.
    if (detached) return;
    for (const p of projects.openProjects) {
      if (!layout.get(p.id)) void layout.ensure(p.id).catch(() => {});
    }
  });
  if (!detached) void loadGlobalLayout().catch(() => {});

  $effect(() => {
    // A detached window (#342) goes away once its tab is back in the main window or closed, after
    // its own save of that change landed.
    const l = detached ? layout.get(detached.project_id) : null;
    if (detached && l && !findTab(l, detached.tab_id)?.detached) {
      void layout.flush(detached.project_id).finally(closeDetachedWindow);
    }
  });

  // ---- jumplist -------------------------------------------------------------------------------

  $effect(() => {
    // Every focus change, wherever it came from; `nav.back` landing on an entry records nothing.
    const project = projects.activeId;
    if (ui.view) {
      noteFocus({ project: GLOBAL, tab: ui.view, pane: ui.view });
      return;
    }
    const tab = currentTab();
    const pane = tab ? focusedPane() : null;
    if (project && tab && pane) noteFocus({ project, tab: tab.id, pane: pane.id });
  });

  // ---- global views ----------------------------------------------------------------------------

  // The view shown is the active tab of the built-in GLOBAL layout (`null` = the project): the core
  // polls it like a visible pane and keeps it, so Kelta reopens on it. Restored once the projects
  // and that layout are known, saved on every change after.
  let placeRestored = false;
  $effect(() => {
    if (detached) return; // the main window's place, never a detached window's
    const view = ui.view;
    const global = layout.get(GLOBAL);
    // tabs: seeded by `loadGlobalLayout`, which carries the pre-#270 place
    if (!projects.loaded || !global?.tabs.length) return;
    const saved = global.active_tab;
    if (!placeRestored) {
      placeRestored = true;
      if (view === null && GLOBAL_VIEWS.includes(saved as GlobalView)) return openView(saved as GlobalView);
    }
    if (saved !== view) layout.update(GLOBAL, (l) => ({ ...l, active_tab: view }));
  });

  // The Queue lists the same work as the rail: the rail folds to its lamps while it is shown (#431).
  $effect(() => sidebar.fold(ui.view === 'now' && !ui.settings));

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
    if (ui.overlay === 'switcher' || ui.overlay === 'reopen')
      lazyComponents.ensure('overlay:switcher', () => import('./palette/Switcher.svelte'));
  });

  const Palette = $derived(lazyComponents.get('overlay:palette').component as AnyComponent | null);
  const Switcher = $derived(lazyComponents.get('overlay:switcher').component as AnyComponent | null);

  let overlayWasOpen = false;
  $effect(() => {
    const open =
      ui.overlay !== null ||
      ui.sheets.length > 0 ||
      confirms.current !== null ||
      prompts.current !== null ||
      ui.settings !== null; // Esc out of Settings gives the keyboard back too
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
    const offQuit = onUiEvent('app.quit_requested', (ev) => {
      askQuit(ev).catch((err: unknown) => toasts.error(err, t('Quitting failed')));
    });
    const offOpen = onUiEvent('ui.open', (ev) => {
      // The backend asks for a pane: the layout store applies it to a loaded layout; an unloaded
      // layout is fetched first. Focus requests also bring the project to the front.
      const id = ev.project_id;
      if (!layout.get(id)) {
        layout
          .ensure(id)
          .then(() => layout.open(id, ev.request))
          .catch((err: unknown) => toasts.error(err, t('Opening the pane failed')));
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
          .catch((err: unknown) => toasts.error(err, t('Bench scenario failed')));
      else if (cmd.cmd === 'start') {
        // The backend asked for the Start sheet (guessed project or plan preview).
        if (cmd.resolved) void startWorkOnTicket(cmd.resolved, cmd.project, { preview: true });
      } else if (cmd.cmd === 'new') {
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
          .catch((err: unknown) => toasts.error(err, t('Starting the session failed')));
      }
    });

    const mql = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
    const onScheme = (e: MediaQueryListEvent): void => {
      settings.systemDark = e.matches;
    };
    mql?.addEventListener('change', onScheme);
    const onFocus = (): void => void (ui.focused = true);
    const onBlur = (): void => void (ui.focused = false);
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);

    // Cached review requests feed the Inbox badge (no network before first paint: cache only).
    if (!detached) void reviews.load({ kind: 'all' }, 'review_requested', false);

    return () => {
      stopKeys();
      offPrefix();
      offRemoved();
      offQuit();
      offOpen();
      offCtl();
      mql?.removeEventListener('change', onScheme);
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    };
  });

  const active = $derived(projects.active);
</script>

{#if !detached}
  <ContextBar />
{/if}
<main class="shell" data-testid="shell" data-theme-resolved={resolvedTheme}>
  {#if !detached}
    <ProjectRail />
  {/if}
  <div class="main">
    {#if detached}
      <!-- A detached window (#342): its tab alone, under the native title bar. -->
      {#if ui.settings}
        <GlobalHost view="settings" />
      {:else}
        <Workspace projectId={detached.project_id} tabId={detached.tab_id} />
      {/if}
    {:else}
      <UpdateBanner />
      {#if ui.settings}
        <GlobalHost view="settings" />
      {:else if ui.view}
        <GlobalHost view={ui.view} />
      {:else if active}
        <Workspace projectId={active.id} />
      {:else if projects.loaded}
        <EmptyState
          icon="folder"
          title={t('No project open')}
          body={t('Pick a project in the sidebar, or create one from a code folder.')}
        >
          {#snippet actions()}
            <Button variant="primary" onclick={() => ui.openSheet('project_new')}
              >{t('Create project from folder')}</Button
            >
            <Button chord={chordFor('project.switcher')} onclick={() => ui.openOverlay('switcher')}
              >{t('Switch project')}</Button
            >
          {/snippet}
        </EmptyState>
      {:else}
        <div class="grow"></div>
      {/if}
      <StatusBar {prefixArmed} />
    {/if}
  </div>

  {#if ui.overlay === 'palette' && Palette}
    <Palette initialQuery={ui.paletteQuery} onclose={() => ui.closeOverlay()} />
  {:else if (ui.overlay === 'switcher' || ui.overlay === 'reopen') && Switcher}
    <Switcher
      closedOnly={ui.overlay === 'reopen'}
      onclose={() => ui.closeOverlay()}
      placeholder={ui.pick?.placeholder}
      onpick={ui.pick ? (id: string) => ui.resolvePick(id) : undefined}
    />
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
