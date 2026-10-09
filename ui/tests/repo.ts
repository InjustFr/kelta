// Node-side helper for tests: absolute paths inside the repository.
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

/** Repository root (directory holding pnpm-workspace.yaml), found from the test cwd. */
export function repoRoot(): string {
  let dir = resolve(process.cwd());
  while (!existsSync(join(dir, 'pnpm-workspace.yaml'))) {
    const parent = dirname(dir);
    if (parent === dir) throw new Error('repository root not found');
    dir = parent;
  }
  return dir;
}

export function repoPath(...parts: string[]): string {
  return join(repoRoot(), ...parts);
}
