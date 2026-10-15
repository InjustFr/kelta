import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

const REFRESH = msg('Refresh');

export default {
  sources: { label: msg('Ticket sources') },
  polling: { label: REFRESH },
  'polling.focused_secs': {
    group: REFRESH,
    order: 0,
    label: msg('Refresh tickets and pull requests every, in seconds'),
    help: msg('While the Kelta window is focused.'),
    keywords: [msg('how often'), msg('refresh interval')],
  },
  'polling.background_secs': {
    group: REFRESH,
    order: 1,
    label: msg('Refresh in the background every, in seconds'),
    help: msg('While another app is in front. 0 stops refreshing.'),
  },
  'polling.when_closed': {
    group: REFRESH,
    order: 2,
    label: msg('With the window closed'),
    help: msg('When Kelta keeps running after its window is closed.'),
    options: { off: { label: msg('Stop refreshing') }, background: { label: msg('Keep refreshing') } },
  },
  'polling.min_secs': {
    label: msg('Fastest refresh, in seconds'),
    help: msg('Shorter intervals above are raised to this. Redmine boards refresh at most every 60 seconds.'),
  },
} satisfies CopyTable;
