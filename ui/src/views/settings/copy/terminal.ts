import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'terminal.bell': { keywords: [msg('sound'), msg('beep')] },
  'terminal.confirm_multiline_paste': { keywords: [msg('clipboard'), msg('paste')] },
  'terminal.copy_on_select': { keywords: [msg('clipboard'), msg('copy')] },
  'terminal.font_size': { keywords: [msg('text size'), msg('zoom in')] },
  'terminal.option_as_meta': { platform: 'macos', keywords: [msg('alt key'), msg('meta key')] },
  'terminal.osc52': { keywords: [msg('clipboard'), msg('tmux')] },
  'terminal.primary_selection': {
    platform: 'linux',
    keywords: [msg('clipboard'), msg('middle click')],
  },
} satisfies CopyTable;
