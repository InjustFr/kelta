import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'notifications.quiet_hours': { keywords: [msg('do not disturb'), msg('mute')] },
} satisfies CopyTable;
