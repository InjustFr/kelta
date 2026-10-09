// Which top-level settings keys each section of the pane edits. The Rust structs are the source
// of truth for what exists; a vitest checks that every top-level key of the schema is covered.

import type { SettingsSectionId } from '$app/registry';

export const SECTION_ROOTS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  general: ['app'],
  window: ['window'],
  keys: ['keys'],
  terminal: ['terminal'],
  'linux-graphics': ['linux'],
  projects: ['env'],
  accounts: ['accounts'],
  claude: ['claude'],
  editors: ['editor'],
  worktree: ['worktree', 'work', 'session_templates'],
  reviews: ['reviews'],
  polling: ['polling'],
  notifications: ['notifications'],
  performance: ['performance', 'web'],
};

/** Top-level keys edited by the plugin/tool lane's sections (not rendered here). */
export const FOREIGN_ROOTS: readonly string[] = ['tools', 'triggers', 'commands', 'plugins'];

export const SECTION_BLURB: Partial<Record<SettingsSectionId, string>> = {
  general: 'Theme, session restore and quit behaviour.',
  window: 'Window decorations and what closing the window does.',
  terminal: 'Fonts, scrollback, clipboard and keyboard behaviour of terminal panes.',
  'linux-graphics':
    'Workarounds applied before the web view starts. They need a restart; `kelta --safe-graphics` enables all of them for one launch.',
  claude: 'How Kelta launches Claude Code. Binary, extra arguments and extra hooks run commands.',
  editors: 'Editor presets used when Kelta opens files. Presets merge by id across layers.',
  worktree: 'Worktree locations, branch naming, setup commands and what happens on PR and merge.',
  reviews: 'Which pull / merge requests appear in the review lists.',
  polling:
    'How often providers are refreshed. Intervals are owned by the scheduler; nothing polls while idle.',
  notifications: 'Desktop notifications and quiet hours.',
  performance: 'Memory HUD and web view behaviour.',
  projects: 'Environment variables added to every session (project and repo files may add more).',
};
