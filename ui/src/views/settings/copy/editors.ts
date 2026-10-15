import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'editor.default': { keywords: [msg('vscode'), msg('vs code'), msg('zed'), msg('neovim')] },
} satisfies CopyTable;
