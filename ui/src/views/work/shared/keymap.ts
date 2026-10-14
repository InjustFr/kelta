// One verb per letter across the Queue, Tickets and Reviews (#417, #440, FLOW §3.3): each list's keys, as its
// `?` dialog and hint line show them, tagged with the verb of the shared table. keymap.test.ts holds
// every list to that table, so a letter learned in one list means the same in the next.
import { msg } from '$lib/i18n/index.svelte';

import { chordFor } from '../../../shell/labels';

export type Verb =
  | 'next'
  | 'work'
  | 'view'
  | 'open'
  | 'move'
  | 'comment'
  | 'expand'
  | 'filter'
  | 'keys'
  | 'mark_reviewed'
  | 'refresh'
  | 'mark'
  | 'panel';

/** The table: what each of these keys means in every list (a list may leave one unbound). */
export const VERB_OF: Readonly<Record<string, Verb>> = {
  enter: 'next',
  s: 'work',
  v: 'view',
  'shift+v': 'view',
  o: 'open',
  m: 'move',
  c: 'comment',
  e: 'expand',
  '/': 'filter',
  '?': 'keys',
  'shift+r': 'mark_reviewed',
  'mod+shift+r': 'refresh',
  b: 'mark',
  space: 'panel',
};

/** `[keys, label, verb]`: alternatives split by a space (`'s shift+s'`), a sequence by `then`. */
export type ListKey = readonly [keys: string, label: string, verb?: Verb];

const startMarked = (): ListKey => [chordFor('work.start') ?? 'mod+enter', msg('Start marked')];

/** The Queue: tickets, my work and others' PRs in one list. */
export const queueKeys = (): ListKey[] => [
  ['j k', msg('Move')],
  ['enter', msg('Next step'), 'next'],
  ['s', msg('Work on it: start, go to its tab, review locally'), 'work'],
  ['.', msg('More actions')],
  ['/', msg('Filter'), 'filter'],
  ['v shift+v', msg('View changes, the whole diff'), 'view'],
  ['o', msg('Open on host'), 'open'],
  ['m', msg('Move the ticket'), 'move'],
  ['c', msg('Comment'), 'comment'],
  ['e', msg("Expand Claude's message"), 'expand'],
  ['shift+r', msg('Mark reviewed'), 'mark_reviewed'],
  ['g', msg('Go to')],
  ['b', msg('Mark ticket'), 'mark'],
  ['space', msg('Show or hide the brief'), 'panel'],
  startMarked(),
  ['mod+shift+r', msg('Refresh'), 'refresh'],
];

/** The Tickets list and board; `enter` names what Enter does on the selected row. */
export const ticketsKeys = (enter: string): ListKey[] => [
  ['j k', msg('Move')],
  ['enter', enter, 'next'],
  ['s shift+s', msg('Start work, start now'), 'work'],
  ['m', msg('Move to'), 'move'],
  ['/', msg('Filter'), 'filter'],
  ['g', msg('Open detail')],
  ['w', msg('Run workflow')],
  ['space', msg('Split'), 'panel'],
  ['shift+enter', msg('Own pane')],
  ['1 2 3', msg('Who')],
  startMarked(),
  ['x', msg('Select')],
  ['b', msg('Mark for batch start'), 'mark'],
  ['p shift+p', msg('Pull request, in browser')],
  ['f then s', msg('Current sprint')],
  ['f then v', msg('Source')],
  ['a shift+a', msg('Assign me, unassign')],
  ['shift+g', msg('Group')],
  ['c', msg('Comment'), 'comment'],
  ['t', msg('Tags')],
  ['o', msg('Open in browser'), 'open'],
  ['mod+shift+r', msg('Refresh'), 'refresh'],
];

/** The Reviews list. */
export const reviewsKeys = (): ListKey[] => [
  ['j k', msg('Move')],
  ['enter', msg('Open review'), 'next'],
  ['s', msg('Review locally'), 'work'],
  ['v', msg('View changes'), 'view'],
  ['/', msg('Filter'), 'filter'],
  ['c', msg('Comment'), 'comment'],
  ['space', msg('Show or hide the detail'), 'panel'],
  ['shift+enter', msg('Own pane')],
  ['n', msg('Nudge (my PRs)')],
  ['d', msg('Mark ready / draft (my PRs)')],
  ['t', msg('Tags')],
  ['w', msg('Run workflow')],
  ['o', msg('Open in browser'), 'open'],
  ['shift+m', msg('Merge when ready')],
  ['mod+shift+r', msg('Refresh'), 'refresh'],
];
