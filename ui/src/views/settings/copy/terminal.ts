import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

// Groups in the order the form shows them; each group's keys take consecutive `order`s.
const TEXT = msg('Text');
const CURSOR = msg('Cursor');
const CLIPBOARD = msg('Clipboard');
const KEYBOARD = msg('Keyboard');
const HISTORY = msg('Scrollback & history');
const SHELL = msg('Shell');
const SOUND = msg('Sound');

export default {
  'terminal.font_family': {
    label: msg('Font'),
    help: msg('Installed fonts, separated by commas; the first one found is used.'),
    group: TEXT,
    order: 0,
  },
  'terminal.font_size': {
    label: msg('Font size'),
    help: msg('In pixels.'),
    keywords: [msg('text size'), msg('zoom in')],
    group: TEXT,
    order: 1,
  },
  'terminal.line_height': {
    label: msg('Line height'),
    help: msg('Space between lines, as a multiple of the font size.'),
    group: TEXT,
    order: 2,
  },
  'terminal.letter_spacing': {
    label: msg('Letter spacing'),
    help: msg('Extra space between characters, in pixels.'),
    group: TEXT,
    order: 3,
  },
  'terminal.cursor_style': { label: msg('Cursor shape'), group: CURSOR, order: 10 },
  'terminal.cursor_blink': {
    label: msg('Blink the cursor'),
    help: msg('In the focused pane only.'),
    group: CURSOR,
    order: 11,
  },
  'terminal.copy_on_select': {
    label: msg('Copy text when you select it'),
    keywords: [msg('clipboard'), msg('copy')],
    group: CLIPBOARD,
    order: 20,
  },
  'terminal.primary_selection': {
    platform: 'linux',
    label: msg('Middle-click pastes the selection'),
    help: msg('Selected text also goes to the primary selection, which a middle click pastes.'),
    keywords: [msg('clipboard'), msg('middle click')],
    group: CLIPBOARD,
    order: 21,
  },
  // no 'clipboard' keyword: the label has the word, and the keyword would rank it above copy_on_select
  'terminal.osc52': {
    label: msg('Let programs copy to the clipboard'),
    help: msg(
      'Programs such as tmux, vim or a remote shell can set your clipboard. Copy and read also lets them read it.',
    ),
    keywords: [msg('tmux'), msg('OSC 52')],
    group: CLIPBOARD,
    order: 22,
    options: {
      write: { label: msg('Copy only') },
      'read-write': { label: msg('Copy and read') },
      off: { label: msg('Never') },
    },
  },
  'terminal.confirm_multiline_paste': {
    label: msg('Confirm before pasting several lines'),
    help: msg('Only when the program has not turned on bracketed paste.'),
    keywords: [msg('clipboard'), msg('paste')],
    group: CLIPBOARD,
    order: 23,
  },
  'terminal.option_as_meta': {
    platform: 'macos',
    label: msg('Use ⌥ Option as Alt'),
    help: msg(
      'Lets terminal programs read ⌥ shortcuts such as ⌥B and ⌥F. A key left out types special characters.',
    ),
    keywords: [msg('alt key'), msg('meta key')],
    group: KEYBOARD,
    order: 30,
    options: {
      both: { label: msg('Both keys') },
      left: { label: msg('Left key only') },
      right: { label: msg('Right key only') },
      none: { label: msg('Neither (type special characters)') },
    },
  },
  'terminal.shift_enter': {
    label: msg('Shift+Enter in sessions'),
    help: msg('What Shift+Enter sends, per kind of session; the default row covers the others.'),
    group: KEYBOARD,
    order: 31,
  },
  'terminal.scrollback': {
    label: msg('Lines kept per session'),
    help: msg('Per kind of session. Older lines scroll off, or go to disk when saving is on.'),
    keywords: [msg('history'), msg('scrollback')],
    group: HISTORY,
    order: 40,
  },
  'terminal.scrollback.shell': { label: msg('Shell'), order: 0 },
  'terminal.scrollback.claude': { label: msg('Claude'), order: 1 },
  'terminal.scrollback.editor': { label: msg('Editor'), order: 2 },
  'terminal.scrollback.tool': { label: msg('Tools'), order: 3 },
  'terminal.scrollback.setup': { label: msg('Setup commands'), order: 4 },
  'terminal.scrollback.custom': { label: msg('Other commands'), order: 5 },
  'terminal.history_log': {
    label: msg('Save scrolled-off lines to disk'),
    help: msg('Lets you search and restore old output of each session.'),
    keywords: [msg('history')],
    group: HISTORY,
    order: 41,
  },
  'terminal.history_log_mb': {
    label: msg('Disk space per session, in MB'),
    help: msg('Older lines are dropped first.'),
    group: HISTORY,
    order: 42,
  },
  'terminal.history_log_total_mb': {
    label: msg('Disk space for all sessions, in MB'),
    help: msg('The oldest logs are deleted first.'),
    group: HISTORY,
    order: 43,
  },
  'terminal.shell': {
    label: msg('Shell'),
    help: msg('Program new terminal sessions start. Empty uses your login shell.'),
    group: SHELL,
    order: 50,
  },
  'terminal.env': {
    label: msg('Environment variables'),
    help: msg('Added to every terminal session.'),
    group: SHELL,
    order: 51,
  },
  'terminal.shell_integration': {
    label: msg('Shell integration'),
    help: msg(
      'zsh, bash and fish tell Kelta their folder and where each command starts and ends, so new splits and restored sessions open where you were. Off starts shells untouched.',
    ),
    keywords: [msg('folder'), msg('cwd')],
    group: SHELL,
    order: 52,
  },
  'terminal.bell': {
    label: msg('Bell'),
    help: msg('What a terminal bell does.'),
    keywords: [msg('sound'), msg('beep')],
    group: SOUND,
    order: 60,
    options: {
      attention: { label: msg('Mark the session for attention') },
      visual: { label: msg('Visual only') },
      none: { label: msg('Off') },
    },
  },
  // Advanced (lib/sections.ts ADVANCED): no group, the collapsed block lists them in ADVANCED order.
  'terminal.keyboard_protocol': {
    label: msg('Keyboard protocol'),
    help: msg(
      'Kitty tells programs that ask exactly which keys and modifiers you press. Pick legacy if a program misreads keys.',
    ),
  },
  'terminal.renderer': {
    label: msg('Renderer'),
    help: msg(
      'Auto draws with the GPU on macOS and with plain HTML on Linux. Try the other one if text draws wrong.',
    ),
    options: {
      auto: { label: msg('Auto') },
      webgl: { label: msg('GPU (WebGL)') },
      dom: { label: msg('Plain HTML') },
    },
  },
  'terminal.minimum_contrast_ratio': {
    label: msg('Minimum contrast'),
    help: msg(
      'Raises text contrast against its background up to this ratio. 1 changes nothing; 4.5 keeps every color readable.',
    ),
  },
  'terminal.memory_cap_mb': {
    label: msg('Memory for all terminals, in MB'),
    help: msg('Cap on the memory the lines kept by every session use together.'),
  },
  'terminal.max_live_views': {
    label: msg('Hidden terminals kept live'),
    help: msg(
      'Terminals you are not looking at that stay ready, on top of the visible ones. More switches faster and uses more memory.',
    ),
  },
  'terminal.view_scrollback': {
    label: msg('Lines shown while scrolling'),
    help: msg('History lines a pane holds when it opens. More scrolls further back and uses more memory.'),
  },
  'terminal.session_host': {
    label: msg('Keep sessions running after Kelta quits'),
    help: msg(
      'Sessions run in a background helper and are back on the next launch. Off: quitting Kelta ends them.',
    ),
  },
} satisfies CopyTable;
