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
  worktree: ['worktree', 'work', 'tickets', 'session_templates'],
  reviews: ['reviews'],
  polling: ['polling'],
  notifications: ['notifications'],
  performance: ['performance', 'web'],
  tools: ['tools'],
};

/** Top-level keys edited by the plugin/tool lane's sections or the theme picker (not rendered here). */
export const FOREIGN_ROOTS: readonly string[] = ['triggers', 'commands', 'plugins', 'themes'];

export const SECTION_BLURB: Partial<Record<SettingsSectionId, string>> = {
  general: 'Theme, session restore and quit behaviour.',
  window: 'Window decorations and what closing the window does.',
  terminal: 'Fonts, scrollback, clipboard and keyboard behaviour of terminal panes.',
  'linux-graphics':
    'Workarounds applied before the web view starts. They need a restart; `kelta --safe-graphics` enables all of them for one launch.',
  claude: 'How Kelta launches Claude Code. Binary, extra arguments and extra hooks run commands.',
  editors:
    'Editors Kelta can open files in. A preset set here replaces the one with the same name from a broader level.',
  worktree:
    'Branch folders (git worktrees) let each work item have its own copy of the code. Choose where they live, how branches are named, and what runs on PR and merge.',
  reviews: 'Which pull / merge requests appear in the review lists.',
  polling: 'How often Kelta checks your tracker and code host for changes.',
  notifications: 'Desktop notifications and quiet hours.',
  performance: 'Memory use and web view behaviour.',
  tools: 'Programs like lazygit that open inside Kelta in their own pane.',
  projects: 'Folders Kelta manages, with their repositories and ticket sources.',
};

/** Visible section title; the registry label stays for ids and tests. */
export const sectionLabel = (s: { id: string; label: string }): string =>
  s.id === 'worktree' ? 'Branch folders' : s.label;
