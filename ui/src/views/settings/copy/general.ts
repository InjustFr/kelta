import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'app.restore_mode': {
    label: msg('Reopen sessions on launch'),
    help: msg(
      'Lazy reopens each session when you first look at it, eager reopens all of them at startup, none starts empty.',
    ),
  },
  'app.confirm_quit_with_running': { label: msg('Ask before quitting while sessions run') },
  'app.quit_sessions': {
    label: msg('Sessions on quit'),
    help: msg(
      'Ask, keep them running in the background (they are back on the next launch), or stop them all.',
    ),
  },
  'app.updates': {
    label: msg('Check for updates automatically'),
    help: msg('On start and when Kelta gets focus, at most every 6 hours. Check now works either way.'),
    keywords: [msg('upgrade'), msg('new version')],
  },
  'app.dark_theme': {
    label: msg('Dark theme'),
    help: msg('Theme id used in dark mode: bezel-dark or a [themes.<id>] with base = "dark".'),
  },
  'app.light_theme': {
    label: msg('Light theme'),
    help: msg('Theme id used in light mode: bezel-light or a [themes.<id>] with base = "light".'),
  },
} satisfies CopyTable;
