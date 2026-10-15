import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'editor.default': {
    order: 0,
    label: msg('Open files in'),
    help: msg('The id of the editor below that Open file and review sessions use, such as nvim or vscode.'),
    keywords: [msg('vscode'), msg('vs code'), msg('zed'), msg('neovim')],
  },
  'editor.follow_claude_edits': {
    order: 1,
    label: msg('When Claude edits a file'),
    help: msg('Reload it in the editor if it is open, open it, or leave the editor alone.'),
    options: {
      off: { label: msg('Leave the editor alone') },
      reload: { label: msg('Reload it if open') },
      open: { label: msg('Open it') },
    },
  },
  'editor.presets': {
    order: 2,
    label: msg('Editors'),
    help: msg('The command and arguments of each editor, and how it opens a file at a line.'),
  },
  'editor.review_args': {
    label: msg('Diff editor arguments'),
    help: msg(
      'Added for review sessions and used by Review diff; {range} is what to compare. Empty runs git diff in a shell.',
    ),
  },
} satisfies CopyTable;
