import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  performance: { label: msg('Performance') },
  'performance.hud': { label: msg('Show memory use in the status bar') },
  web: { label: msg('Web views') },
  'web.embed_default': {
    label: msg('Open web tools'),
    help: msg(
      "In a pane, in a pane through Kelta's local proxy for sites that refuse to be embedded, or in your browser. A tool can set its own.",
    ),
    options: {
      auto: { label: msg('Automatic') },
      iframe: { label: msg('In a pane') },
      proxy: { label: msg('In a pane, through the proxy') },
      external: { label: msg('In your browser') },
    },
  },
  'web.keep_alive': {
    label: msg('Keep hidden web views loaded'),
    help: msg('Plugin screens and web tools keep their state while hidden, at the cost of memory.'),
  },
} satisfies CopyTable;
