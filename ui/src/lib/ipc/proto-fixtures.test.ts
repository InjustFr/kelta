// Every kelta-proto JSON fixture (crates/kelta-proto/fixtures/*.json) must parse into the typed
// copy in gen/fixtures.ts — those copies are annotated with the generated TS types, so equality here
// plus svelte-check on fixtures.ts proves the JSON conforms to the TS types (serde ↔ ts-rs drift).
import { readFileSync, readdirSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { fixtures } from '$lib/gen/fixtures';

import { repoPath } from '../../../tests/repo';

const DIR = repoPath('crates/kelta-proto/fixtures') + '/';
const files = readdirSync(DIR)
  .filter((f) => f.endsWith('.json'))
  .sort();

describe('kelta-proto JSON fixtures against generated TS types', () => {
  it('every JSON file has a typed fixture and vice versa', () => {
    expect(files.map((f) => f.replace(/\.json$/, ''))).toEqual(Object.keys(fixtures).sort());
  });

  for (const file of files) {
    it(`${file} parses and equals its typed fixture`, () => {
      const parsed: unknown = JSON.parse(readFileSync(DIR + file, 'utf8'));
      expect(parsed).toEqual(fixtures[file.replace(/\.json$/, '')]);
    });
  }
});
