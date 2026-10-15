import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export const APPEARANCE = msg('Appearance');
const STARTUP = msg('Startup & quit');
export const UPDATES = msg('Updates');

export default {
  // roots: not shown as rows, required by the COPY_DONE test
  app: { label: msg('General') },
  window: { label: msg('Window') },
  'app.theme': {
    group: APPEARANCE,
    order: 0,
    label: msg('Light or dark'),
    help: msg('Follow the system, or always use the light or the dark theme.'),
    keywords: [msg('dark mode'), msg('light mode')],
    options: { system: { label: msg('Follow the system') } },
  },
  'app.dark_theme': {
    group: APPEARANCE,
    order: 1,
    label: msg('Dark theme'),
    help: msg('Theme id used in dark mode: bezel-dark or a [themes.<id>] with base = "dark".'),
  },
  'app.light_theme': {
    group: APPEARANCE,
    order: 2,
    label: msg('Light theme'),
    help: msg('Theme id used in light mode: bezel-light or a [themes.<id>] with base = "light".'),
  },
  'app.language': {
    group: APPEARANCE,
    order: 3,
    label: msg('Language'),
    help: msg('Language of menus and messages. System follows your computer.'),
    keywords: [msg('locale')],
  },
  'app.animations': {
    group: APPEARANCE,
    order: 4,
    label: msg('Animations'),
    help: msg(
      'Motion that shows what changed. Reduced keeps fades and loaders only; Auto reduces motion in safe graphics mode.',
    ),
  },
  'window.decorations': {
    group: APPEARANCE,
    order: 5,
    label: msg('Window frame'),
    help: msg('Automatic draws no frame on Hyprland and Sway, and the native title bar elsewhere.'),
    keywords: [msg('title bar')],
  },
  'app.restore_mode': {
    group: STARTUP,
    order: 10,
    label: msg('Reopen sessions on launch'),
    help: msg(
      'Lazy reopens each session when you first look at it, eager reopens all of them at startup, none starts empty.',
    ),
  },
  'window.restore_geometry': {
    group: STARTUP,
    order: 11,
    label: msg('Reopen the window where you left it'),
    help: msg('Same size and position as when you quit.'),
  },
  'app.quit_sessions': {
    group: STARTUP,
    order: 12,
    label: msg('Sessions on quit'),
    help: msg(
      'Ask, keep them running in the background (they are back on the next launch), or stop them all.',
    ),
  },
  'app.confirm_quit_with_running': {
    group: STARTUP,
    order: 13,
    label: msg('Ask before quitting while sessions run'),
  },
  'window.close_behavior': {
    group: STARTUP,
    order: 14,
    label: msg('Closing the window'),
    help: msg(
      'Keep Kelta running in the background, or quit. Automatic keeps it running on macOS and quits on Linux.',
    ),
    options: { background: { label: msg('Keep Kelta running') }, quit: { label: msg('Quit') } },
  },
  'app.updates': {
    group: UPDATES,
    order: 20,
    label: msg('Check for updates automatically'),
    help: msg('On start and when Kelta gets focus, at most every 6 hours. Check now works either way.'),
    keywords: [msg('upgrade'), msg('new version')],
  },
  // Advanced
  'app.log_level': { label: msg('Log detail'), help: msg('How much Kelta writes to its log file.') },
} satisfies CopyTable;
