import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

const SESSIONS = msg('Sessions & cost');
export const PROFILES = msg('Profiles');
export const IN_CLAUDE_CODE = msg('Kelta in Claude Code');

export default {
  'claude.max_live': {
    group: SESSIONS,
    order: 0,
    label: msg('Claude sessions running at once'),
    help: msg('Past this, new work waits in Queued and starts when a session ends. 0 means no limit.'),
    keywords: [msg('max sessions'), msg('concurrent')],
  },
  'claude.budget_usd': {
    group: SESSIONS,
    order: 1,
    label: msg('Cost warning per work item, in US dollars'),
    help: msg('The cost chip turns red past this amount. Set it per project.'),
    keywords: [msg('budget'), msg('spend')],
  },
  'claude.auto_park_after_mins': {
    group: SESSIONS,
    order: 2,
    label: msg('Park idle Claude after, in minutes'),
    help: msg(
      'Once done, seen and untouched this long, its Claude stops; showing the work item resumes the conversation. 0 never parks.',
    ),
    keywords: [msg('idle')],
  },
  'claude.profiles': {
    group: PROFILES,
    order: 10,
    label: msg('Profile list'),
    help: msg('Model, effort and permission mode of each profile. Templates and workflows pick one by name.'),
  },
  'claude.allowed_tools': {
    group: PROFILES,
    order: 11,
    label: msg('Tools Claude uses without asking'),
    help: msg('Patterns such as mcp__kelta__*.'),
  },
  'claude.mcp': {
    group: IN_CLAUDE_CODE,
    order: 20,
    label: msg("Give Kelta's sessions the ticket tools"),
    help: msg('Claude sessions Kelta starts can read and move tickets.'),
  },
  'claude.mcp_board_writes': {
    group: IN_CLAUDE_CODE,
    order: 21,
    label: msg('Let Claude move and edit any ticket'),
    help: msg('Off: Claude can only read the board.'),
  },
  // Advanced (sections.ts ADVANCED): launch options and offers
  'claude.binary': { label: msg('Claude Code command'), help: msg('Found in your login PATH.') },
  'claude.extra_args': { label: msg('Extra arguments'), help: msg('Added before the prompt.') },
  'claude.extra_hooks': {
    label: msg('Extra hooks'),
    help: msg('Added to the hooks Kelta gives each session.'),
  },
  'claude.append_system_prompt': {
    label: msg('Extra instructions'),
    help: msg('Added to the context every session starts with.'),
  },
  'claude.terminal_command': {
    label: msg('Terminal for Claude'),
    help: msg(
      "Opens a template's Claude when it runs in a terminal: {command} is the script to run, {cwd} the folder, {shell} your shell. Empty uses Terminal on macOS and x-terminal-emulator on Linux.",
    ),
  },
  'claude.hook_transport': {
    label: msg('How hooks reach Kelta'),
    help: msg('Session start always uses a command.'),
  },
  'claude.ide_bridge': {
    label: msg("Act as Claude's IDE"),
    help: msg("Claude sessions open files and diffs in Kelta's editor pane."),
  },
  'claude.mcp_offer': {
    label: msg('Offer the ticket tools at startup'),
    help: msg('When Claude Code does not have them yet.'),
  },
  'claude.hooks_offer': {
    label: msg('Offer to show outside sessions at startup'),
    help: msg("When Kelta's hooks are not in Claude Code's user settings yet."),
  },
  'claude.min_version': {
    label: msg('Oldest supported Claude Code'),
    help: msg('Older versions show a warning.'),
  },
  'claude.prompt_templates': {
    label: msg('Prompts'),
    help: msg('What Kelta asks Claude for a ticket, a review, a fix or a workflow.'),
  },
  'claude.queue_hold_pct': {
    label: msg('Hold the queue at rate-limit use, in percent'),
    help: msg('New work waits while the 5-hour limit is used at least this much. Empty never holds.'),
  },
} satisfies CopyTable;
