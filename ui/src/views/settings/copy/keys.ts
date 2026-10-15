import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'keys.prefix': {
    label: msg('Prefix key'),
    help: msg('Press it, then one key, to reach any action. Off turns it off.'),
    keywords: [msg('leader')],
  },
  'keys.prefix_hint_ms': {
    label: msg('Show the next keys after, in milliseconds'),
    help: msg('Pause after the prefix before a panel lists the next keys. 0 turns the panel off.'),
  },
  'keys.prefix_timeout_ms': {
    label: msg('Wait for the key after the prefix, in milliseconds'),
    help: msg('The prefix is dropped after this long.'),
  },
  'keys.prefix_bindings': {
    label: msg('Keys after the prefix'),
    help: msg('One key per action, pressed after the prefix key.'),
  },
  'keys.bindings': {
    label: msg('Shortcuts'),
    help: msg('Changed shortcuts only; every other action keeps its default.'),
  },
  'keys.list_keys': { label: msg('Single-key shortcuts in lists and boards') },
  'keys.linux_terminal': {
    platform: 'linux',
    label: msg('Ctrl+letter in a focused terminal'),
    help: msg(
      'Who gets a plain Ctrl+letter that Kelta also binds: the terminal program or Kelta.',
    ),
    options: { terminal_first: { label: msg('Terminal first') }, kelta_first: { label: msg('Kelta first') } },
  },
} satisfies CopyTable;
