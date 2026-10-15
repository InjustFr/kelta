import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  accounts: { label: msg('Accounts') },
  oauth: {
    label: msg('Browser sign-in'),
    help: msg('The OAuth apps Sign in with browser uses for GitHub and GitLab.'),
  },
  'oauth.client_ids': {
    label: msg('App client id per host'),
    help: msg(
      'Client id of your GitHub OAuth app or GitLab application, for each host such as github.com or gitlab.com.',
    ),
  },
} satisfies CopyTable;
