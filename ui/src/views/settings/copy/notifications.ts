import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

// Groups in the order the form shows them; each group's keys take consecutive `order`s.
const EVENTS = msg('Tell me when');
const QUIET = msg('Quiet time');

export default {
  // The switch the others depend on: Notifications.svelte disables them while it is off.
  'notifications.enabled': {
    label: msg('Desktop notifications'),
    help: msg('Turn off to silence everything below.'),
    order: 0,
  },
  'notifications.claude_blocked': {
    label: msg('Claude needs you'),
    help: msg('It asks a question, waits for input, or replied without changes.'),
    group: EVENTS,
    order: 10,
  },
  'notifications.claude_done': {
    label: msg('Claude is done'),
    help: msg('Ready for review, or finished.'),
    group: EVENTS,
    order: 11,
  },
  'notifications.review_requested': {
    label: msg('Someone requests my review'),
    help: msg('Keeps checking review requests while Kelta is in the background.'),
    group: EVENTS,
    order: 12,
  },
  'notifications.pr_approved': { label: msg('My pull request is approved'), group: EVENTS, order: 13 },
  'notifications.pr_changes_requested': {
    label: msg('Changes are requested on my pull request'),
    group: EVENTS,
    order: 14,
  },
  'notifications.ci_failed_mine': { label: msg('CI fails on my pull request'), group: EVENTS, order: 15 },
  'notifications.quiet_hours': {
    label: msg('Do not disturb hours'),
    help: msg('No notifications between these times, every day. Leave both empty to turn it off.'),
    keywords: [msg('do not disturb'), msg('dnd'), msg('mute'), msg('quiet hours')],
    group: QUIET,
    order: 20,
  },
  'notifications.only_when_unfocused': {
    label: msg('Only when Kelta is in the background'),
    help: msg('Or when the pane it is about is hidden.'),
    group: QUIET,
    order: 21,
  },
  'notifications.focus_allow_blocked': {
    label: msg('In focus mode, still tell me when Claude is blocked'),
    help: msg('Focus mode holds the other notifications.'),
    group: QUIET,
    order: 22,
  },
  // Advanced (lib/sections.ts ADVANCED): no group.
  'notifications.bell_background': { label: msg('A bell in a background terminal notifies') },
} satisfies CopyTable;
