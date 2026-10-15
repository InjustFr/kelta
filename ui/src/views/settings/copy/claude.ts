import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'claude.max_live': { keywords: [msg('max sessions'), msg('concurrent')] },
} satisfies CopyTable;
