import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { repoPath } from '../../../tests/repo';

import * as commands from './commands';
import { COMMAND_NAMES, call } from './commands';
import { createMockTransport } from './mock';
import { IpcError, isIpcError, setTransport, toIpcError } from './transport';

const NAMES_RS = repoPath('apps/desktop/src-tauri/src/commands/names.rs');

function rustCommandNames(): string[] {
  const src = readFileSync(NAMES_RS, 'utf8');
  return [...src.matchAll(/^\s*"([a-z_]+)",/gm)].map((m) => m[1]!);
}

const camel = (s: string): string => s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());

describe('command catalogue', () => {
  it('matches apps/desktop/src-tauri/src/commands/names.rs exactly (same order)', () => {
    expect([...COMMAND_NAMES]).toEqual(rustCommandNames());
  });

  it('has a camelCase wrapper for every command', () => {
    for (const name of COMMAND_NAMES) {
      expect(typeof (commands as Record<string, unknown>)[camel(name)], name).toBe('function');
    }
  });
});

describe('IPC errors', () => {
  it('normalizes KeltaError rejections', () => {
    const e = toIpcError('x', {
      code: 'needs_fields',
      message: 'm',
      detail: { fields: [] },
      retry_after_ms: null,
    });
    expect(e).toBeInstanceOf(IpcError);
    expect(e.code).toBe('needs_fields');
    expect(e.detail).toEqual({ fields: [] });
    expect(isIpcError(e, 'needs_fields')).toBe(true);
    expect(isIpcError(e, 'conflict')).toBe(false);
  });

  it('wraps strings and Errors as internal', () => {
    expect(toIpcError('x', 'boom')).toMatchObject({ code: 'internal', message: 'boom' });
    expect(toIpcError('x', new Error('bad'))).toMatchObject({ code: 'internal', message: 'bad' });
  });

  it('wrappers reject with IpcError', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    controls.failNext('project_list', { code: 'unsupported', message: 'not implemented: project_list' });
    await expect(commands.projectList()).rejects.toMatchObject({
      name: 'IpcError',
      code: 'unsupported',
      command: 'project_list',
    });
  });
});

describe('binary commands', () => {
  it('session_write sends raw bytes with the session header', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    const id = controls.state.sessions[0]!.id;
    await commands.sessionWrite(id, 'ls\r');
    const last = controls.calls.at(-1)!;
    expect(last.cmd).toBe('session_write');
    expect(last.args).toMatchObject({ id });
    expect((last.args as { data: Uint8Array }).data).toBeInstanceOf(Uint8Array);
  });

  it('session_attach streams a snapshot frame and echoes writes', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    const id = controls.state.sessions[0]!.id;
    const frames: Uint8Array[] = [];
    const info = await commands.sessionAttach({ id, cols: 80, rows: 24 }, (f) => frames.push(f));
    expect(info).toMatchObject({ cols: 80, rows: 24 });
    await commands.sessionWrite(id, 'x');
    await new Promise((r) => setTimeout(r, 0));
    expect(frames[0]?.[0]).toBe(2); // FRAME_SNAPSHOT
    expect(frames.at(-1)?.[0]).toBe(1); // FRAME_DATA
    expect(new TextDecoder().decode(frames.at(-1)!.slice(1))).toBe('x');
  });

  it('frameBytes accepts every channel payload form', () => {
    expect(commands.frameBytes([1, 2])).toEqual(new Uint8Array([1, 2]));
    expect(commands.frameBytes(new Uint8Array([3]).buffer)).toEqual(new Uint8Array([3]));
  });
});

describe('call()', () => {
  it('drops undefined arguments', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    await call('session_list', { project_id: undefined });
    expect(controls.calls.at(-1)?.args).toEqual({});
  });
});
