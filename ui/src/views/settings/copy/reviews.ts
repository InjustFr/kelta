import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'reviews.include_team_requests': {
    order: 0,
    label: msg('Include requests to my teams'),
    help: msg('Pull requests whose review was asked of a team you are in.'),
  },
  'reviews.include_drafts': {
    order: 1,
    label: msg('Show draft pull requests'),
    help: msg('In review requests. Your own pull requests always show drafts.'),
  },
  'reviews.repos_allow': {
    order: 2,
    label: msg('Only these repositories'),
    help: msg('Patterns such as acme/*. Empty shows every repository.'),
  },
  'reviews.repos_deny': {
    order: 3,
    label: msg('Never these repositories'),
    help: msg('Patterns such as acme/legacy-*.'),
  },
  'reviews.sla_hours': {
    order: 4,
    label: msg('Flag my pull requests waiting longer than, in hours'),
    help: msg('Their waiting-on-reviewer chip turns amber.'),
  },
  'reviews.ignore_globs': {
    label: msg('Generated files'),
    help: msg('Paths the Ready for review chip does not count as lines to review.'),
  },
  'reviews.nudge_template': {
    label: msg('Reminder comment'),
    help: msg('Posted by Comment ping on your pull request; {reviewers} lists the reviewers.'),
  },
  'reviews.ticket_key_regex': {
    label: msg('Ticket key pattern'),
    help: msg('A regular expression that finds linked tickets in the branch name and title.'),
  },
} satisfies CopyTable;
