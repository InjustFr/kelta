import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'worktree.branch_template': { keywords: [msg('branch name')] },
  'worktree.root': { keywords: [msg('worktree folder'), msg('location')] },
  'work.pr.body_template': { keywords: [msg('pr'), msg('pull request description')] },
  'work.pr.draft': { keywords: [msg('pr')] },
  'work.pr.merge_method': { keywords: [msg('pr'), msg('squash')] },
  'work.pr.title_template': { keywords: [msg('pr'), msg('pull request title')] },
} satisfies CopyTable;
