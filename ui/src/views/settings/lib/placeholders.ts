// Placeholders of template settings (SETTINGS §6) for the Insert menu (#479). The examples are the sample
// ticket KEL-214 of `kelta_work::plan::preview`, so the menu and the live example agree.
import { msg } from '$lib/i18n/index.svelte';

export interface Placeholder {
  token: string;
  help: string;
  example: string;
}

export const PLACEHOLDER_GROUPS: { label: string; items: Placeholder[] }[] = [
  {
    label: msg('Ticket'),
    items: [
      { token: '{ticket.key}', help: msg('Ticket key'), example: 'KEL-214' },
      { token: '{ticket.title}', help: msg('Ticket title'), example: 'Search settings by meaning' },
      { token: '{ticket.url}', help: msg('Ticket link'), example: 'https://tracker.example/browse/KEL-214' },
      {
        token: '{ticket.file}',
        help: msg('Ticket text file given to Claude'),
        example: '…/1a2b3c4d/ticket.md',
      },
      { token: '{key}', help: msg('Key for branch names'), example: 'KEL-214' },
      { token: '{slug}', help: msg('Title, as a slug'), example: 'search-settings-by-meaning' },
      { token: '{type}', help: msg('Branch type, from the ticket type'), example: 'feat' },
    ],
  },
  {
    label: msg('Pull request'),
    items: [
      { token: '{pr.number}', help: msg('Pull request number'), example: '215' },
      { token: '{pr.title}', help: msg('Pull request title'), example: 'Search settings by meaning' },
      {
        token: '{pr.url}',
        help: msg('Pull request link'),
        example: 'https://github.com/acme/kelta/pull/215',
      },
      {
        token: '{pr.head}',
        help: msg('Pull request branch'),
        example: 'feat/KEL-214-search-settings-by-meaning',
      },
      { token: '{pr.base}', help: msg('Branch it merges into'), example: 'main' },
      { token: '{closes}', help: msg('Line that closes a GitHub issue'), example: 'Closes #214' },
    ],
  },
  {
    label: msg('Project and repository'),
    items: [
      { token: '{project.id}', help: msg('Project id'), example: 'kelta' },
      { token: '{project.name}', help: msg('Project name'), example: 'Kelta' },
      { token: '{project.root}', help: msg('Main repository folder'), example: '~/code/kelta' },
      { token: '{repo.id}', help: msg('Repository id'), example: 'app' },
      { token: '{repo.name}', help: msg('Repository folder name'), example: 'kelta' },
      { token: '{repo.path}', help: msg('Repository folder'), example: '~/code/kelta' },
      { token: '{branch}', help: msg('Work branch'), example: 'feat/KEL-214-search-settings-by-meaning' },
      { token: '{base}', help: msg('Base branch'), example: 'main' },
    ],
  },
  {
    label: msg('Session'),
    items: [
      { token: '{session.id}', help: msg('Session id'), example: '0192aaaa-…' },
      { token: '{session.name}', help: msg('Session name'), example: 'Claude' },
      {
        token: '{session.cwd}',
        help: msg('Session folder'),
        example: '~/.kelta-worktrees/kelta/app/KEL-214-…',
      },
      { token: '{sid8}', help: msg('Short session id'), example: '1a2b3c4d' },
      { token: '{run}', help: msg('Session run folder'), example: '…/run/s/1a2b3c4d' },
      { token: '{port}', help: msg('First port of the work item'), example: '20140' },
    ],
  },
  {
    label: msg('Paths'),
    items: [
      {
        token: '{worktree}',
        help: msg('Worktree folder'),
        example: '~/.kelta-worktrees/kelta/app/KEL-214-search-settings-by-meaning',
      },
      { token: '{config_dir}', help: msg('Kelta settings folder'), example: '~/.config/kelta' },
      { token: '{data_dir}', help: msg('Kelta data folder'), example: '~/.local/share/kelta' },
      { token: '{home}', help: msg('Home folder'), example: '~' },
      { token: '{user}', help: msg('User name'), example: 'ada' },
    ],
  },
];
