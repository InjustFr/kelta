import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  env: { label: msg('Environment variables'), help: msg('Added to every session of the project.') },
} satisfies CopyTable;
