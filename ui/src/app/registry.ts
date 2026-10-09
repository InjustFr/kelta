// UI registries (BUILD_PLAN §2.4): pane kinds, tab headers, settings sections and sheets map to
// lazily loaded components. Every view is a dynamic import → its own chunk.
// Owners replace the stub components; the keys and paths here are frozen.

import type { Component } from 'svelte';

import type { PaneContent } from '$lib/gen';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type AnyComponent = Component<any>;
export type LazyComponent = () => Promise<{ default: AnyComponent }>;

export type PaneKind = PaneContent['kind'];
/** Pane kinds with a registered view (`empty` is rendered by the shell itself). */
export type RegisteredPaneKind = Exclude<PaneKind, 'empty'>;

/** Props every pane component receives from the shell's pane host. */
export interface PaneProps<K extends PaneKind = PaneKind> {
  projectId: string;
  tabId: string;
  paneId: string;
  content: Extract<PaneContent, { kind: K }>;
  /** Pane is currently visible (inactive tabs are not mounted; zoom hides siblings). */
  visible: boolean;
  focused: boolean;
}

export const paneRegistry: Record<RegisteredPaneKind, LazyComponent> = {
  terminal: () => import('../views/terminal/TerminalPane.svelte'),
  welcome: () => import('../views/welcome/WelcomePane.svelte'),
  tickets: () => import('../views/tickets/TicketsPane.svelte'),
  ticket_detail: () => import('../views/tickets/TicketDetailPane.svelte'),
  reviews: () => import('../views/reviews/ReviewsPane.svelte'),
  review_detail: () => import('../views/reviews/ReviewDetailPane.svelte'),
  inbox: () => import('../views/inbox/InboxPane.svelte'),
  work_item: () => import('../views/work/WorkItemPane.svelte'),
  settings: () => import('../views/settings/SettingsPane.svelte'),
  diagnostics: () => import('../views/diagnostics/DiagnosticsPane.svelte'),
  web: () => import('../views/web-tool/WebToolPane.svelte'),
  plugin_screen: () => import('../views/plugin-screen/PluginScreenPane.svelte'),
};

export function paneComponent(kind: PaneKind): LazyComponent | null {
  return kind === 'empty' ? null : paneRegistry[kind];
}

/** Tab header components, keyed by tab flavour (`work` = tab linked to a work item). */
export type TabHeaderKey = 'work';
export interface TabHeaderProps {
  projectId: string;
  tabId: string;
  workItemId: string;
}
export const tabHeaderRegistry: Record<TabHeaderKey, LazyComponent> = {
  work: () => import('../views/work/WorkItemHeader.svelte'),
};

/** Sheets (modal side panels) opened through `ui.openSheet(key, props)`. */
export type RegisteredSheetKey =
  | 'start_work'
  | 'onboarding'
  | 'project_new'
  | 'plugin_install'
  | 'tool_picker'
  | 'ship'
  | 'finish'
  | 'finish_merged';
export const sheetRegistry: Record<RegisteredSheetKey, LazyComponent> = {
  start_work: () => import('../views/work/StartWorkSheet.svelte'),
  onboarding: () => import('../views/onboarding/OnboardingSheet.svelte'),
  project_new: () => import('../views/onboarding/ProjectNewSheet.svelte'),
  plugin_install: () => import('../views/plugins/InstallSheet.svelte'),
  tool_picker: () => import('../views/tools/ToolPicker.svelte'),
  ship: () => import('../views/work/ShipDialog.svelte'),
  finish: () => import('../views/work/FinishDialog.svelte'),
  finish_merged: () => import('../views/work/FinishMergedDialog.svelte'),
};

export type SettingsSectionId =
  | 'general'
  | 'window'
  | 'keys'
  | 'terminal'
  | 'linux-graphics'
  | 'projects'
  | 'accounts'
  | 'claude'
  | 'editors'
  | 'worktree'
  | 'reviews'
  | 'polling'
  | 'notifications'
  | 'performance'
  | 'tools'
  | 'triggers'
  | 'plugins';

export interface SettingsSection {
  id: SettingsSectionId;
  label: string;
  icon: string;
  /** Owning lane (documentation). */
  owner: 'L4' | 'L8';
  /** Only shown on this platform. */
  platform?: 'linux' | 'macos';
  load: LazyComponent;
}

/** Props of a settings section component. */
export interface SettingsSectionProps {
  /** Layer being edited (`global` unless a project is selected). */
  layer: 'global' | 'project' | 'repo';
  projectId: string | null;
  repoId: string | null;
}

export const settingsSections: readonly SettingsSection[] = [
  {
    id: 'general',
    label: 'General',
    icon: 'settings',
    owner: 'L4',
    load: () => import('../views/settings/sections/General.svelte'),
  },
  {
    id: 'window',
    label: 'Window',
    icon: 'app-window',
    owner: 'L4',
    load: () => import('../views/settings/sections/Window.svelte'),
  },
  {
    id: 'keys',
    label: 'Keys',
    icon: 'keyboard',
    owner: 'L4',
    load: () => import('../views/settings/sections/Keys.svelte'),
  },
  {
    id: 'terminal',
    label: 'Terminal',
    icon: 'square-terminal',
    owner: 'L4',
    load: () => import('../views/settings/sections/Terminal.svelte'),
  },
  {
    id: 'linux-graphics',
    label: 'Linux graphics',
    icon: 'monitor',
    owner: 'L4',
    platform: 'linux',
    load: () => import('../views/settings/sections/LinuxGraphics.svelte'),
  },
  {
    id: 'projects',
    label: 'Projects',
    icon: 'folder',
    owner: 'L4',
    load: () => import('../views/settings/sections/Projects.svelte'),
  },
  {
    id: 'accounts',
    label: 'Accounts',
    icon: 'key-round',
    owner: 'L4',
    load: () => import('../views/settings/sections/Accounts.svelte'),
  },
  {
    id: 'claude',
    label: 'Claude',
    icon: 'bot',
    owner: 'L4',
    load: () => import('../views/settings/sections/Claude.svelte'),
  },
  {
    id: 'editors',
    label: 'Editors',
    icon: 'file-code',
    owner: 'L4',
    load: () => import('../views/settings/sections/Editors.svelte'),
  },
  {
    id: 'worktree',
    label: 'Worktrees',
    icon: 'git-branch',
    owner: 'L4',
    load: () => import('../views/settings/sections/Worktree.svelte'),
  },
  {
    id: 'reviews',
    label: 'Reviews',
    icon: 'git-pull-request',
    owner: 'L4',
    load: () => import('../views/settings/sections/Reviews.svelte'),
  },
  {
    id: 'polling',
    label: 'Polling',
    icon: 'refresh-cw',
    owner: 'L4',
    load: () => import('../views/settings/sections/Polling.svelte'),
  },
  {
    id: 'notifications',
    label: 'Notifications',
    icon: 'bell',
    owner: 'L4',
    load: () => import('../views/settings/sections/Notifications.svelte'),
  },
  {
    id: 'performance',
    label: 'Performance',
    icon: 'gauge',
    owner: 'L4',
    load: () => import('../views/settings/sections/Performance.svelte'),
  },
  {
    id: 'tools',
    label: 'Tools',
    icon: 'wrench',
    owner: 'L8',
    load: () => import('../views/plugins/sections/Tools.svelte'),
  },
  {
    id: 'triggers',
    label: 'Triggers',
    icon: 'zap',
    owner: 'L8',
    load: () => import('../views/plugins/sections/Triggers.svelte'),
  },
  {
    id: 'plugins',
    label: 'Plugins',
    icon: 'puzzle',
    owner: 'L8',
    load: () => import('../views/plugins/sections/Plugins.svelte'),
  },
];

export function settingsSection(id: string): SettingsSection | null {
  return settingsSections.find((s) => s.id === id) ?? null;
}

/** Props every sheet component receives (`ui.openSheet(key, props)` props are spread in). */
export interface SheetProps {
  onclose: () => void;
  [key: string]: unknown;
}
