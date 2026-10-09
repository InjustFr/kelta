// @vitest-environment jsdom
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

import ts from 'typescript';
import { describe, expect, it, vi } from 'vitest';

import { connect, KeltaError, matches, type Kelta } from '../src/index';

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

/** A fake host: answers `kelta:ready` with an init message from `source`. */
function host(source: Window | null = window) {
  const channel = new MessageChannel();
  const requests: { id: number; method: string; params: unknown }[] = [];
  channel.port1.onmessage = (ev: MessageEvent) => {
    const m = ev.data as { id: number; method: string; params: unknown };
    requests.push(m);
    if (m.method === 'tickets.list') {
      channel.port1.postMessage({
        id: m.id,
        error: { code: 'permission_denied', message: 'missing permission: tickets.read' },
      });
    } else {
      channel.port1.postMessage({ id: m.id, result: { echo: m.method, params: m.params } });
    }
  };
  const init = (src: Window | null = source) =>
    window.dispatchEvent(
      new MessageEvent('message', {
        data: {
          type: 'kelta:init',
          api: '0.1',
          instance: 'i1',
          plugin: 'hello-screen',
          project: 'shop',
          params: { a: 1 },
          theme: { '--k-bg': '#000' },
        },
        source: src,
        ports: [channel.port2],
      }),
    );
  return { channel, requests, init };
}

describe('connect', () => {
  it('ignores init messages from a foreign source', async () => {
    const h = host();
    let connected: Kelta | null = null;
    const p = connect({ window }).then((k) => (connected = k));
    const foreign = document.createElement('iframe');
    document.body.append(foreign);
    h.init(foreign.contentWindow);
    await flush();
    expect(connected).toBeNull();
    h.init(window);
    await p;
    expect(connected).not.toBeNull();
  });

  it('applies the theme and resolves requests by id', async () => {
    const h = host();
    const p = connect({ window });
    h.init();
    const k = await p;
    expect(k.init.plugin).toBe('hello-screen');
    expect(k.init.project).toBe('shop');
    expect(document.documentElement.style.getPropertyValue('--k-bg')).toBe('#000');
    const [a, b] = await Promise.all([k.app.info(), k.call('projects.current')]);
    expect(a).toEqual({ echo: 'app.info', params: {} });
    expect(b).toEqual({ echo: 'projects.current', params: {} });
    expect(new Set(h.requests.map((r) => r.id)).size).toBe(2);
  });

  it('rejects with a KeltaError carrying the code', async () => {
    const h = host();
    const p = connect({ window });
    h.init();
    const k = await p;
    const err = await k.tickets.list().catch((e: unknown) => e);
    expect(err).toBeInstanceOf(KeltaError);
    expect((err as KeltaError).code).toBe('permission_denied');
  });

  it('subscribes to events and dispatches host pushes', async () => {
    const h = host();
    const p = connect({ window });
    h.init();
    const k = await p;
    const got: string[] = [];
    const off = k.events.on('session.*', (_payload, name) => got.push(name));
    const vis: boolean[] = [];
    k.onVisibility((v) => vis.push(v));
    // MessageChannel delivery is not ordered with timers: wait for it instead of one tick.
    await vi.waitFor(() =>
      expect(h.requests.find((r) => r.method === 'events.subscribe')?.params).toEqual({
        names: ['session.*'],
      }),
    );
    h.channel.port1.postMessage({ type: 'event', name: 'session.bell', payload: {} });
    h.channel.port1.postMessage({ type: 'event', name: 'pr.merged', payload: {} });
    h.channel.port1.postMessage({ type: 'visibility', visible: false });
    h.channel.port1.postMessage({ type: 'theme', tokens: { '--k-fg': '#fff' } });
    await vi.waitFor(() => expect(k.theme['--k-fg']).toBe('#fff'));
    expect(got).toEqual(['session.bell']);
    expect(vis).toEqual([false]);
    off();
    await vi.waitFor(() => expect(h.requests.some((r) => r.method === 'events.unsubscribe')).toBe(true));
  });

  it('maps helpers to host methods', async () => {
    const h = host();
    const p = connect({ window });
    h.init();
    const k = await p;
    await k.fetch('https://api.example.com/x', { method: 'POST', body: '{}' });
    await k.ui.openScreen('other', { x: 1 });
    await k.notify('t', 'b');
    await k.sessions.sendText('s1', 'ls');
    expect(h.requests.map((r) => r.method)).toEqual([
      'net.fetch',
      'ui.open_screen',
      'notify.send',
      'sessions.send_text',
    ]);
    expect(h.requests[0]?.params).toEqual({ url: 'https://api.example.com/x', method: 'POST', body: '{}' });
  });
});

describe('matches', () => {
  it('globs event names', () => {
    expect(matches('ticket.*', 'ticket.started')).toBe(true);
    expect(matches('ticket.*', 'pr.merged')).toBe(false);
    expect(matches('a.b', 'a.b')).toBe(true);
    expect(matches('a.b', 'aXb')).toBe(false);
  });
});

describe('vendored copy', () => {
  // examples/plugins/hello-screen ships the SDK as plain ESM (no build step for plugin authors).
  // Regenerate with KELTA_UPDATE_VENDOR=1 pnpm --filter @kelta/plugin-sdk test.
  it('matches the source', () => {
    // vitest runs with the package directory as cwd.
    const src = resolve(process.cwd(), 'src/index.ts');
    const vendored = resolve(process.cwd(), '../../examples/plugins/hello-screen/dist/kelta-sdk.js');
    const out = ts.transpileModule(readFileSync(src, 'utf8'), {
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        removeComments: false,
      },
    }).outputText;
    const expected = `// Vendored copy of @kelta/plugin-sdk (packages/plugin-sdk/src/index.ts). Generated: do not edit.\n${out}`;
    if (process.env.KELTA_UPDATE_VENDOR) writeFileSync(vendored, expected);
    expect(readFileSync(vendored, 'utf8')).toBe(expected);
  });
});
