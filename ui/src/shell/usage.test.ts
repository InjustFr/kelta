import { describe, expect, it } from 'vitest';

import type { ClaudeUsage, SessionInfo } from '$lib/gen';
import { sessionInfo, workItem } from '$lib/gen/fixtures';

import { ctxHot, itemCost, latestWindow, meter, overBudget, usd } from './usage';

const withUsage = (id: string, u: Partial<ClaudeUsage>): SessionInfo => ({
  ...sessionInfo,
  id,
  claude: { ...sessionInfo.claude!, usage: { ...sessionInfo.claude!.usage!, ...u } },
});

describe('claude usage', () => {
  it('formats the pane chip parts', () => {
    expect(usd(1.8412)).toBe('$1.84');
    expect(meter(64)).toBe('▮▮▮▯');
    expect(meter(0)).toBe('▯▯▯▯');
    expect(meter(120)).toBe('▮▮▮▮');
    expect(ctxHot(withUsage('a', { context_pct: 85 }).claude!.usage!)).toBe(false);
    expect(ctxHot(withUsage('a', { context_pct: 86 }).claude!.usage!)).toBe(true);
    expect(ctxHot(withUsage('a', { context_pct: null }).claude!.usage!)).toBe(false);
  });

  it('sums saved and unsaved spend per work item', () => {
    const a = withUsage('a', { unsaved_usd: 1 });
    const b = withUsage('b', { unsaved_usd: 0.5 });
    const other = { ...withUsage('c', { unsaved_usd: 9 }), work_item_id: null };
    expect(itemCost({ ...workItem, cost_usd: 2 }, [a, b, other])).toBe(3.5);
    expect(overBudget(3.5, 3)).toBe(true);
    expect(overBudget(3.5, null)).toBe(false);
  });

  it('picks the freshest rate window and none without rate limits', () => {
    const old = withUsage('a', { five_hour: { used_percentage: 90, resets_at: 100 } });
    const cur = withUsage('b', { five_hour: { used_percentage: 10, resets_at: 200 } });
    expect(latestWindow([old, cur], 'five_hour', 0)?.used_percentage).toBe(10);
    expect(latestWindow([old, cur], 'five_hour', 200)).toBeNull();
    const apiKey = withUsage('c', { five_hour: null, seven_day: null });
    expect(latestWindow([apiKey], 'five_hour', 0)).toBeNull();
    expect(latestWindow([], 'seven_day')).toBeNull();
  });
});
