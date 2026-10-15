import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import StageTrack from './StageTrack.svelte';

describe('StageTrack pipeline (#462)', () => {
  it('names the steps and lights Claude working on the current one', () => {
    const labels = ['Ticket', 'Implementing', 'PR', 'PO Review', 'Merge'];
    const { container } = render(StageTrack, {
      props: { stage: { at: 3, reached: 3, mark: 'wait', labels, working: true }, labels: true },
    });
    expect([...container.querySelectorAll('.name')].map((n) => n.textContent)).toEqual(labels);
    const now = container.querySelector('.name.now')!;
    expect(now.textContent).toBe('PO Review');
    expect(now.querySelector('[data-attention="working"]')).not.toBeNull();
    expect(container.querySelector('.track')?.getAttribute('aria-label')).toBe(
      'At PO Review, waiting on others, Claude working; furthest stage reached: PO Review',
    );
  });
});
