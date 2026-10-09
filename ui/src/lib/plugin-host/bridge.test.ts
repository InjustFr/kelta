import { render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import PluginScreenPane from '../../views/plugin-screen/PluginScreenPane.svelte';
import WebToolPane from '../../views/web-tool/WebToolPane.svelte';

import { connectScreen } from './bridge';
import { applyWebToolEvent, rememberWebTool, webTools } from './web.svelte';

function frame(): HTMLIFrameElement {
  const iframe = document.createElement('iframe');
  document.body.appendChild(iframe);
  return iframe;
}

const init = { instance: 'scr-1', plugin: 'hello-screen', project: 'shop', params: null };

function ready(source: Window | null): void {
  window.dispatchEvent(new MessageEvent('message', { data: { type: 'kelta:ready' }, source }));
}

describe('screen bridge', () => {
  it('ignores kelta:ready from a foreign source', () => {
    const iframe = frame();
    const other = frame();
    const post = vi.spyOn(iframe.contentWindow!, 'postMessage');
    const b = connectScreen(iframe, init, () => ({}), vi.fn());
    ready(other.contentWindow);
    ready(window);
    expect(post).not.toHaveBeenCalled();
    b.destroy();
  });

  it('transfers a port to its own iframe and answers requests by id', async () => {
    const iframe = frame();
    const post = vi.spyOn(iframe.contentWindow!, 'postMessage').mockImplementation(() => {});
    const call = vi.fn(async (method: string) => {
      if (method === 'tickets.list') throw { code: 'permission_denied', message: 'missing tickets.read' };
      return { version: '0.1.0' };
    });
    const b = connectScreen(iframe, init, () => ({ '--k-bg': '#000' }), call as never);
    ready(iframe.contentWindow);

    expect(post).toHaveBeenCalledTimes(1);
    const [msg, target, transfer] = post.mock.calls[0] as unknown as [
      Record<string, unknown>,
      string,
      MessagePort[],
    ];
    expect(msg).toMatchObject({
      type: 'kelta:init',
      api: '0.1',
      instance: 'scr-1',
      theme: { '--k-bg': '#000' },
    });
    expect(target).toBe('*');
    const port = transfer[0];

    const replies: unknown[] = [];
    const got = new Promise<void>((resolve) => {
      port.onmessage = (e) => {
        replies.push(e.data);
        if (replies.length === 2) resolve();
      };
    });
    port.postMessage({ id: 1, method: 'app.info', params: {} });
    port.postMessage({ id: 2, method: 'tickets.list', params: {} });
    await got;
    expect(replies).toContainEqual({ id: 1, result: { version: '0.1.0' } });
    expect(replies).toContainEqual({
      id: 2,
      error: { code: 'permission_denied', message: 'missing tickets.read', detail: null },
    });
    port.close();
    b.destroy();
  });
});

describe('web tool registry', () => {
  it('records handles and exits relayed as plugin.event', () => {
    applyWebToolEvent('t-1', 'kelta.tool_handle', {
      handle: { kind: 'web', instance_id: 't-1', url: 'http://127.0.0.1:9/', embed: 'proxy' },
      tool_id: 'isl',
      label: 'ISL',
      project_id: 'shop',
      lifecycle: 'never',
    });
    expect(webTools['t-1']).toMatchObject({ url: 'http://127.0.0.1:9/', embed: 'proxy', lifecycle: 'never' });
    applyWebToolEvent('t-1', 'kelta.tool_exited', { code: 2, log: 'boom' });
    expect(webTools['t-1'].exited).toEqual({ code: 2, log: 'boom' });
    expect(applyWebToolEvent('t-1', 'other', null)).toBe(false);
  });
});

describe('PluginScreenPane', () => {
  it('creates a sandboxed iframe when visible and destroys it when hidden', async () => {
    setTransport(createMockTransport().transport);
    const content = {
      kind: 'plugin_screen' as const,
      plugin_id: 'sprint-burndown',
      screen_id: 'burndown',
      instance_id: 'scr-0',
      params: null,
    };
    const props = { projectId: 'shop', tabId: 't', paneId: 'p', content, visible: true, focused: true };
    const { container, rerender } = render(PluginScreenPane, props);
    await vi.waitFor(() => expect(container.querySelector('iframe')).not.toBeNull());
    const iframe = container.querySelector('iframe')!;
    expect(iframe.getAttribute('sandbox')).toBe('allow-scripts allow-forms');
    expect(iframe.getAttribute('src')).toMatch(/^kelta-plugin:\/\/sprint-burndown\//);

    await rerender({ ...props, visible: false });
    await vi.waitFor(() => expect(container.querySelector('iframe')).toBeNull());
  });
});

describe('WebToolPane', () => {
  it('never frames a non-http(s) tool URL', async () => {
    setTransport(createMockTransport().transport);
    const handle = {
      kind: 'web' as const,
      instance_id: 't-x',
      url: 'kelta-plugin://p/x.html',
      embed: 'iframe' as const,
    };
    rememberWebTool(handle, { toolId: 'p/x', label: 'X', projectId: 'shop' });
    const content = { kind: 'web' as const, tool_instance_id: 't-x' };
    const props = { projectId: 'shop', tabId: 't', paneId: 'p', content, visible: true, focused: true };
    const { container, getByText } = render(WebToolPane, props);
    expect(getByText('Open in browser')).toBeTruthy();
    expect(container.querySelector('iframe')).toBeNull();
  });
});
