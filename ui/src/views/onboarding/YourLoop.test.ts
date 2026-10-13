import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { LOOP_STEPS, type LoopStepId } from './firstRun';
import YourLoop from './YourLoop.svelte';

const ids = LOOP_STEPS.map((s) => s.id);

describe('YourLoop', () => {
  it.each(ids)('%s is the current step: earlier ones checked, one highlighted', (id: LoopStepId) => {
    const at = ids.indexOf(id);
    render(YourLoop, { props: { done: Object.fromEntries(ids.map((s, i) => [s, i < at])) as never } });
    const items = [...screen.getByTestId('your-loop').querySelectorAll('li')];
    expect(items.map((li) => li.getAttribute('aria-current'))).toEqual(
      ids.map((s) => (s === id ? 'step' : null)),
    );
    expect(items[at]!.querySelector('button') !== null).toBe(['project', 'source', 'start'].includes(id));
  });
});
