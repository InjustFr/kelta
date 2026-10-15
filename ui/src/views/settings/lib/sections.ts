// Which top-level settings keys each section of the pane edits, how the sections are grouped in the
// nav, and which keys start collapsed. The Rust structs are the source of truth for what exists; a
// vitest walks the schema and checks every key is reachable (a field, an advanced field or TOML).

import { msg, t } from '$lib/i18n/index.svelte';
import type { SettingsSectionId } from '$app/registry';

export const SECTION_ROOTS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  general: ['app', 'window'],
  keys: ['keys'],
  terminal: ['terminal'],
  'linux-graphics': ['linux'],
  projects: ['env'],
  sources: ['sources', 'polling'],
  accounts: ['accounts', 'oauth'],
  claude: ['claude'],
  editors: ['editor'],
  worktree: ['worktree', 'work', 'tickets', 'session_templates', 'ports'],
  reviews: ['reviews'],
  notifications: ['notifications'],
  performance: ['performance', 'web'],
  tools: ['tools'],
};

/** Top-level keys no form edits (or only through a custom picker): the section links to Edit TOML. */
export const TOML_ROOTS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  general: ['themes'],
  triggers: ['triggers', 'commands', 'workflows'],
  plugins: ['plugins'],
};

/** Nav groups, in order. A section missing here (e.g. a platform-only one) is not shown. */
export const SECTION_GROUPS: readonly { label: string; ids: readonly SettingsSectionId[] }[] = [
  {
    label: msg('Basics'),
    ids: ['health', 'general', 'terminal', 'keys', 'notifications', 'linux-graphics'],
  },
  { label: msg('Work'), ids: ['projects', 'sources', 'worktree', 'reviews', 'tags', 'claude', 'editors'] },
  { label: msg('Integrations'), ids: ['accounts', 'tools', 'triggers', 'plugins'] },
  { label: msg('Advanced'), ids: ['performance'] },
];

/** Rarely changed keys: rendered under the section's collapsed "Advanced" block, still searchable. */
export const ADVANCED: readonly string[] = [
  'app.log_level',
  'keys.list_keys',
  'keys.prefix_hint_ms',
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
  'claude.hooks_offer',
  'claude.min_version',
  'claude.prompt_templates',
  'claude.queue_hold_pct',
  'claude.terminal_command',
  'editor.review_args',
  'work.on_start.transition_to',
  'work.on_start.comment',
  'work.on_pr.transition_to',
  'work.on_pr.comment',
  'work.on_merge.transition_to',
  'work.pipeline',
  'work.scratch_branch_template',
  'work.review_template',
  'worktree.slug_max',
  'worktree.type_map',
  'worktree.env_template',
  'worktree.cleanup',
  'ports',
  'worktree.fetch_timeout_secs',
  'session_templates',
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
/** The key or one of its parents is in ADVANCED (the form and the search). Field.svelte keeps the exact
 *  `isAdvanced`: inside an advanced group (web, oauth) its children must still show. */
export const underAdvanced = (path: string): boolean =>
  ADVANCED.some((a) => path === a || path.startsWith(`${a}.`));

export const SECTION_BLURB: Partial<Record<SettingsSectionId, string>> = {
  general: msg('Appearance, startup and quit, the window and updates.'),
  keys: msg(
    'Matching uses physical keys. Mod is Cmd on macOS and Ctrl on Linux, where a focused terminal keeps plain Ctrl+letter and Ctrl+Shift+letter still reaches Kelta. An app prefix key followed by one key reaches every action.',
  ),
  terminal: msg('Fonts, scrollback, clipboard and keyboard behaviour of terminal panes.'),
  'linux-graphics': msg(
    'Workarounds applied before the web view starts. They need a restart; `kelta --safe-graphics` enables all of them for one launch.',
  ),
  accounts: msg(
    'Accounts connect trackers and code hosts. Tokens are kept in your system keychain or read from a CLI you already use, never in config files.',
  ),
  claude: msg(
    'How many Claude sessions run and what they cost, their profiles, and Kelta in Claude Code. The launch options under Advanced run commands.',
  ),
  editors: msg(
    'Editors Kelta can open files in. A preset set here replaces the one with the same name from a broader level.',
  ),
  worktree: msg('What Kelta does to tickets, branches and pull requests as work moves from start to merge.'),
  reviews: msg('Which pull / merge requests appear in the review lists.'),
  tags: msg(
    'Local tags: reminders you put on tickets and pull requests. Kept in Kelta, never sent to a tracker or code host.',
  ),
  notifications: msg('Desktop notifications and quiet hours.'),
  performance: msg('Memory use and how web views are kept.'),
  tools: msg('Buttons that open programs like lazygit in a pane, or apps like Fork.'),
  triggers: msg('Commands that run when something happens, such as a ticket starting. Plugins can add some.'),
  plugins: msg('Plugins add tools, triggers and screens. Each asks for the permissions it needs.'),
  projects: msg('Folders Kelta manages, with their repositories.'),
  sources: msg(
    'Boards you take tickets from, which projects they feed, how tickets route to a repository, and how often they refresh.',
  ),
};

/** Words the settings search matches on a section's name. */
export const SECTION_KEYWORDS: Partial<Record<SettingsSectionId, readonly string[]>> = {
  keys: [msg('shortcut'), msg('keyboard shortcuts'), msg('keybinding'), msg('hotkey')],
  worktree: [msg('worktree')],
  triggers: [msg('automation')],
  plugins: [msg('extensions')],
};

export interface SectionRow {
  label: string;
  keywords?: readonly string[];
  /** Heading shown in the breadcrumb (Section › group). */
  group?: string;
  /** data-testid of the row in its section; none: the hit opens the section at the top. */
  anchor?: string;
}

/** Rows a section draws itself (not schema keys), for the settings search. Kept here, not in the
 *  sections: they load lazily and the search must not load them all. At most one row per section
 *  without an anchor. */
export const SECTION_ROWS: Partial<Record<SettingsSectionId, readonly SectionRow[]>> = {
  claude: [
    {
      label: msg("Kelta's ticket tools in Claude Code"),
      keywords: [msg('mcp'), msg('install')],
      group: msg('Kelta in Claude Code'),
      anchor: 'claude-mcp',
    },
    {
      label: msg('Claude sessions started outside Kelta'),
      keywords: [msg('hooks')],
      group: msg('Kelta in Claude Code'),
      anchor: 'claude-hooks',
    },
    {
      label: msg('Profiles without a permission mode start in'),
      group: msg('Profiles'),
      anchor: 'claude-permission-mode',
    },
  ],
  notifications: [
    { label: msg('Test notification'), keywords: [msg('send a test')], anchor: 'notification-test' },
  ],
  accounts: [
    {
      label: msg('Add account'),
      keywords: [msg('token'), msg('github token'), msg('sign in')],
      anchor: 'add-account',
    },
  ],
  projects: [
    { label: msg('Create project from folder'), keywords: [msg('add project'), msg('new project')] },
  ],
  tags: [{ label: msg('New tag'), keywords: [msg('label')] }],
  tools: [{ label: msg('Add tool'), keywords: [msg('lazygit')] }],
};

/** Visible section title. */
export const sectionLabel = (s: { id: string; label: string }): string => t(s.label);
