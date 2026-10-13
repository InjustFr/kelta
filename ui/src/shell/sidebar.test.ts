import { beforeEach, describe, expect, it } from 'vitest';

import { sidebar } from './sidebar.svelte';

describe('sidebar fold (#431)', () => {
  beforeEach(() => {
    sidebar.fold(false);
    if (sidebar.compact) sidebar.toggle();
  });

  it('folds while the Queue is shown and unfolds when you leave', () => {
    sidebar.fold(true);
    expect(sidebar.compact).toBe(true);
    sidebar.fold(false);
    expect(sidebar.compact).toBe(false);
  });

  it('a hand expansion wins until the view changes, then the Queue folds it again', () => {
    sidebar.fold(true);
    sidebar.toggle();
    expect(sidebar.compact).toBe(false);
    sidebar.fold(false);
    sidebar.fold(true);
    expect(sidebar.compact).toBe(true);
  });

  it('a hand collapse survives leaving the Queue', () => {
    sidebar.fold(true);
    sidebar.toggle(); // expand
    sidebar.toggle(); // collapse
    sidebar.fold(false);
    expect(sidebar.compact).toBe(true);
  });
});
