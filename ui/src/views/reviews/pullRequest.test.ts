import { describe, expect, it } from 'vitest';

import type { CiCheck } from '$lib/gen';

import { checksFirst } from './pullRequest';

const c = (name: string, state: CiCheck['state']): CiCheck => ({ name, state, url: null });

describe('checksFirst', () => {
  it('failed and errored first, then running, then the rest, each in host order', () => {
    const out = checksFirst([
      c('lint', 'success'),
      c('e2e', 'pending'),
      c('unit', 'failure'),
      c('build', 'success'),
      c('deploy', 'error'),
    ]);
    expect(out.map((x) => x.name)).toEqual(['unit', 'deploy', 'e2e', 'lint', 'build']);
  });
});
