import { afterEach, describe, expect, it } from 'vitest';

import type { ProjectInfo } from '$lib/gen';
import { projects, ui } from '$lib/stores';

import { offerOnboarding, ONBOARDING_DONE } from './firstRun';

const project = (id: string, builtin: boolean) => ({ id, builtin }) as ProjectInfo;

describe('offerOnboarding', () => {
  afterEach(() => {
    localStorage.clear();
    ui.sheets = [];
  });

  it('opens on a fresh install only', () => {
    projects.loaded = true;
    projects.list = [project('home', true)];
    offerOnboarding();
    expect(ui.sheets.map((s) => s.key)).toEqual(['onboarding']);

    ui.sheets = [];
    localStorage.setItem(ONBOARDING_DONE, '1');
    offerOnboarding();
    expect(ui.sheets).toEqual([]);

    localStorage.clear();
    projects.list = [project('home', true), project('shop', false)];
    offerOnboarding();
    expect(ui.sheets).toEqual([]);
  });
});
