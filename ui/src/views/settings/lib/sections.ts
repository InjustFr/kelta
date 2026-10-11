// Which top-level settings keys each section of the pane edits, how the sections are grouped in the
// nav, and which keys start collapsed. The Rust structs are the source of truth for what exists; a
// vitest walks the schema and checks every key is reachable (a field, an advanced field or TOML).

import type { SettingsSectionId } from '$app/registry';

export const SECTION_ROOTS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  general: ['app'],
  window: ['window'],
  keys: ['keys'],
  terminal: ['terminal'],
  'linux-graphics': ['linux'],
  projects: ['env'],
  sources: ['sources'],
  accounts: ['accounts', 'oauth'],
  claude: ['claude'],
  editors: ['editor'],
  worktree: ['worktree', 'work', 'tickets', 'session_templates', 'ports'],
  reviews: ['reviews'],
  notifications: ['notifications'],
  performance: ['polling', 'performance', 'web'],
  tools: ['tools'],
};

/** Top-level keys no form edits (or only through a custom picker): the section links to Edit TOML. */
export const TOML_ROOTS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  general: ['themes'],
  triggers: ['triggers', 'commands'],
  plugins: ['plugins'],
};

/** Nav groups, in order. A section missing here (e.g. a platform-only one) is not shown. */
export const SECTION_GROUPS: readonly { label: string; ids: readonly SettingsSectionId[] }[] = [
  { label: 'Basics', ids: ['general', 'window', 'terminal', 'keys', 'notifications', 'linux-graphics'] },
  { label: 'Work', ids: ['projects', 'sources', 'worktree', 'reviews', 'claude', 'editors'] },
  { label: 'Integrations', ids: ['accounts', 'tools', 'triggers', 'plugins'] },
  { label: 'Advanced', ids: ['performance'] },
];

/** Rarely changed keys: rendered under the section's collapsed "Advanced" block, still searchable. */
export const ADVANCED: readonly string[] = [
  'app.log_level',
  'keys.list_keys',
  'keys.prefix_timeout_ms',
  'terminal.env',
  'terminal.history_log',
  'terminal.history_log_mb',
  'terminal.history_log_total_mb',
  'terminal.keyboard_protocol',
  'terminal.letter_spacing',
  'terminal.max_live_views',
  'terminal.memory_cap_mb',
  'terminal.minimum_contrast_ratio',
  'terminal.osc52',
  'terminal.renderer',
  'terminal.scrollback',
  'terminal.session_host',
  'terminal.shift_enter',
  'terminal.view_scrollback',
  'oauth',
  'claude.append_system_prompt',
  'claude.binary',
  'claude.extra_args',
  'claude.extra_hooks',
  'claude.hook_transport',
  'claude.ide_bridge',
  'claude.mcp_offer',
  'claude.min_version',
  'claude.prompt_templates',
  'claude.queue_hold_pct',
  'editor.review_args',
  'worktree.env_template',
  'worktree.fetch_timeout_secs',
  'worktree.slug_max',
  'worktree.type_map',
  'work.review_template',
  'work.scratch_branch_template',
  'session_templates',
  'ports',
  'reviews.ignore_globs',
  'reviews.nudge_template',
  'reviews.ticket_key_regex',
  'notifications.bell_background',
  'polling.min_secs',
  'web',
  'tools',
];

const advanced = new Set(ADVANCED);
export const isAdvanced = (path: string): boolean => advanced.has(path);

export const SECTION_BLURB: Partial<Record<SettingsSectionId, string>> = {
  general: 'Theme, updates, session restore and quit behaviour.',
  window: 'Window decorations and what closing the window does.',
  keys: 'Matching uses physical keys. On Linux Mod is Ctrl+Shift, on macOS it is Cmd. An app prefix key followed by one key reaches every action.',
  terminal: 'Fonts, scrollback, clipboard and keyboard behaviour of terminal panes.',
  'linux-graphics':
    'Workarounds applied before the web view starts. They need a restart; `kelta --safe-graphics` enables all of them for one launch.',
  accounts:
    'Accounts connect trackers and code hosts. Tokens are kept in your system keychain or read from a CLI you already use, never in config files.',
  claude: 'How Kelta launches Claude Code. Binary, extra arguments and extra hooks run commands.',
  editors:
    'Editors Kelta can open files in. A preset set here replaces the one with the same name from a broader level.',
  worktree:
    'Branch folders (git worktrees) let each work item have its own copy of the code. Choose where they live, how branches are named, and what runs on PR and merge.',
  reviews: 'Which pull / merge requests appear in the review lists.',
  notifications: 'Desktop notifications and quiet hours.',
  performance: 'How often Kelta checks your tracker and code host, memory use and web view behaviour.',
  tools: 'Buttons that open programs like lazygit in a pane, or apps like Fork.',
  triggers: 'Commands that run when something happens, such as a ticket starting. Plugins can add some.',
  plugins: 'Plugins add tools, triggers and screens. Each asks for the permissions it needs.',
  projects: 'Folders Kelta manages, with their repositories.',
  sources: 'Boards you take tickets from, which projects they feed, and how tickets route to a repository.',
};

/** Visible section title; the registry label stays for ids and tests. */
export const sectionLabel = (s: { id: string; label: string }): string =>
  s.id === 'worktree' ? 'Branch folders' : s.label;
