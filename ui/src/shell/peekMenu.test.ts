import { describe, expect, it } from 'vitest';

import { detectMenu } from './peekMenu';

// Screen tails as `session_text_tail` returns them (Claude Code 2.x).
const BOXED = `╭──────────────────────────────────────────────────────────╮
│ Bash command                                             │
│                                                          │
│   rm -rf build                                           │
│   Remove the build directory                             │
│                                                          │
│ Do you want to proceed?                                  │
│ ❯ 1. Yes                                                 │
│   2. Yes, and don't ask again for rm commands in /repo   │
│   3. No, and tell Claude what to do differently (esc)    │
╰──────────────────────────────────────────────────────────╯


`;

const PLAIN = ` Edit file
 src/main.rs
 Do you want to make this edit to main.rs?
 ❯ 1. Yes
   2. Yes, allow all edits during this session (shift+tab)
   3. No, and tell Claude what to do differently (esc)

 Esc to cancel · Tab to add additional instructions`;

const ELICITATION = ` Which database should I use?
 › 1) Postgres
   2) SQLite`;

const LIST_IN_TEXT = `⏺ Plan:
  1. Read the config
  2. Patch the parser
  3. Run the tests

  Shall I go ahead?




>`;

describe('detectMenu', () => {
  it('reads a boxed permission prompt', () => {
    expect(detectMenu(BOXED)).toEqual([
      { key: '1', label: 'Yes' },
      { key: '2', label: "Yes, and don't ask again for rm commands in /repo" },
      { key: '3', label: 'No, and tell Claude what to do differently (esc)' },
    ]);
  });

  it('reads an unboxed prompt with a footer hint', () => {
    expect(detectMenu(PLAIN)?.map((o) => o.label)).toEqual([
      'Yes',
      'Yes, allow all edits during this session (shift+tab)',
      'No, and tell Claude what to do differently (esc)',
    ]);
  });

  it('reads an elicitation with `N)` options', () => {
    expect(detectMenu(ELICITATION)?.map((o) => o.key)).toEqual(['1', '2']);
  });

  it('ignores a numbered list far from the bottom, and plain text', () => {
    expect(detectMenu(LIST_IN_TEXT)).toBeNull();
    expect(detectMenu('just a prompt\n> ')).toBeNull();
    expect(detectMenu('')).toBeNull();
  });

  it('takes the last run when an older list sits above the menu', () => {
    const tail = `  1. first step\n  2. second step\n Proceed?\n ❯ 1. Yes\n   2. No`;
    expect(detectMenu(tail)).toEqual([
      { key: '1', label: 'Yes' },
      { key: '2', label: 'No' },
    ]);
  });
});
