import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  env: { label: msg('Environment variables') },
} satisfies CopyTable;
