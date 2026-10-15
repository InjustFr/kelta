import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'polling.focused_secs': { keywords: [msg('how often'), msg('refresh interval')] },
} satisfies CopyTable;
