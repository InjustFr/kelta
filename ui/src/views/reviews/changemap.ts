// The change map (#401): a PR's changed lines split by area (code, tests, config, other) from file paths.
// The row bar grows linearly with the lines up to CAP, then shows a cut.

import { msg } from '$lib/i18n/index.svelte';
import type { FileChange, Review } from '$lib/gen';

export const AREAS = ['code', 'tests', 'config', 'other'] as const;
export type Area = (typeof AREAS)[number];

export const AREA_LABEL: Record<Area, string> = {
  code: msg('Code'),
  tests: msg('Tests'),
  config: msg('Config and deploy'),
  other: msg('Other'),
};

/** Lines past which the row bar stops growing. */
export const CAP = 2000;

const TEST_DIR = /(^|\/)(tests?|__tests__|spec|specs|e2e|fixtures|testdata)\//i;
const TEST_FILE = /([._-](test|spec)\.[^/]+$|Test\.[a-z]+$|_test\.[a-z]+$)/;
const CONFIG_DIR =
  /(^|\/)(config|deploy|\.github|\.gitlab|k8s|helm|charts|docker|infra|terraform|ansible)\//i;
const CONFIG_FILE =
  /(^|\/)(Dockerfile[^/]*|Makefile|Procfile|\.gitlab-ci\.yml|\.env[^/]*|[^/]+\.(ya?ml|toml|ini|conf|cfg|properties|tf|json|xml|neon|dist))$/i;
const CODE_FILE =
  /\.(ts|tsx|js|jsx|mjs|cjs|svelte|vue|rs|php|py|go|java|kt|kts|scala|rb|cs|c|cc|cpp|h|hpp|m|swift|dart|ex|exs|erl|clj|lua|sh|bash|zsh|sql|css|scss|sass|less|html|twig|hbs|graphql|proto)$/i;

export function areaOf(path: string): Area {
  if (TEST_DIR.test(path) || TEST_FILE.test(path)) return 'tests';
  if (CONFIG_DIR.test(path) || CONFIG_FILE.test(path)) return 'config';
  return CODE_FILE.test(path) ? 'code' : 'other';
}

export interface ChangeMap {
  /** Changed lines per area; lines the file list does not cover count as other. */
  areas: Record<Area, number>;
  lines: number;
  files: number | null;
}

/** `files` may be partial (GitHub lists the first 100) or empty (hosts that list none). */
export function changeMap(
  rv: Pick<Review, 'additions' | 'deletions' | 'changed_files'>,
  files: readonly FileChange[],
): ChangeMap | null {
  const areas: Record<Area, number> = { code: 0, tests: 0, config: 0, other: 0 };
  for (const f of files) areas[areaOf(f.path)] += f.additions + f.deletions;
  const listed = AREAS.reduce((n, a) => n + areas[a], 0);
  const total =
    rv.additions === null && rv.deletions === null ? null : (rv.additions ?? 0) + (rv.deletions ?? 0);
  if (total === null && files.length === 0) return null;
  const lines = Math.max(total ?? 0, listed);
  areas.other += lines - listed;
  return { areas, lines, files: rv.changed_files ?? (files.length || null) };
}

/** The row bar's width (0..1) and whether it is cut. */
export function barOf(lines: number): { width: number; capped: boolean } {
  return { width: Math.min(lines, CAP) / CAP, capped: lines > CAP };
}

/** `85`, `1.3k`, `7k`. */
export function kLines(n: number): string {
  return n < 1000 ? String(n) : `${(n / 1000).toFixed(1).replace(/\.0$/, '')}k`;
}

/** Files grouped by area (code, tests, config, other), biggest first within each. */
export function filesByArea(files: readonly FileChange[]): { area: Area; files: FileChange[] }[] {
  const size = (f: FileChange) => f.additions + f.deletions;
  return AREAS.map((area) => ({
    area,
    files: files.filter((f) => areaOf(f.path) === area).sort((a, b) => size(b) - size(a)),
  })).filter((g) => g.files.length > 0);
}
