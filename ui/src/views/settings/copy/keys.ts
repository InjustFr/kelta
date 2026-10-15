import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'keys.linux_terminal': {
    platform: 'linux',
    options: { terminal_first: { label: msg('Terminal first') }, kelta_first: { label: msg('Kelta first') } },
  },
} satisfies CopyTable;
