import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { ActionButtonDef, PluginInfo, TicketItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { plugins, projects, reviews, ui, work } from '$lib/stores';
import { idle } from '$lib/stores/loadable';

import ReviewDetailPane from '../reviews/ReviewDetailPane.svelte';
import ActionBar from '../tickets/ActionBar.svelte';

let mock: MockControls;

const toast: ActionButtonDef = {
  id: 'say',
  title: 'Say',
  do: [{ action: 'toast', text: '{ticket.key} {branch}', level: null }],
};
const screenAction: ActionButtonDef = {
  id: 'open',
  title: 'Open my screen',
  do: [{ action: 'open_screen', plugin: null, screen: 's', params: null, placement: null }],
};

function withPlugins(
  ...list: { id: string; enabled?: boolean; ticket?: ActionButtonDef[]; review?: ActionButtonDef[] }[]
) {
  const base = mock.state.plugins[0] as PluginInfo;
  const data = list.map((p) => ({
    ...base,
    id: p.id,
    name: `Plugin ${p.id}`,
    enabled: p.enabled ?? true,
    contributes: {
      ...base.contributes,
      commands: [],
      ticket_actions: p.ticket ?? [],
      review_actions: p.review ?? [],
    },
  }));
  plugins.plugins = { ...idle(), data, fetchedAt: Date.now() };
}

const item = (key: string): TicketItem =>
  structuredClone(mock.state.tickets.find((i) => i.ticket.ref.key === key)!);
const bar = (it: TicketItem, projectId: string | null = 'shop') =>
  render(ActionBar, { props: { item: it, branch: null, resume: false, onrun: () => {}, projectId } });
const runs = () =>
  mock.calls
    .filter((c) => c.cmd === 'command_run')
    .map((c) => c.args as { command_id: string; ctx: Record<string, unknown> });

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  reviews.details = {};
  ui.view = null;
  await projects.load();
  await work.load();
});

describe('plugin ticket/review actions', () => {
  it('runs a ticket action with the ticket, its work item and the work item project', async () => {
    withPlugins({ id: 'qa', ticket: [toast] });
    bar(item('SHOP-142'));
    const toolbar = screen.getByRole('toolbar', { name: 'Ticket actions' });
    await fireEvent.click(within(toolbar).getByRole('button', { name: 'Say' }));
    await waitFor(() => expect(runs()).toHaveLength(1));
    const { command_id, ctx } = runs()[0]!;
    expect(command_id).toBe('qa/say');
    expect(ctx.ticket).toMatchObject({ key: 'SHOP-142' });
    expect(ctx.work_item_id).toBe('0199a6b2-0000-7000-8000-00000000a001');
    expect(ctx.extra).toEqual({ project_id: 'shop' });
  });

  it('runs in the project of a work item that lives in another project', async () => {
    withPlugins({ id: 'qa', ticket: [toast] });
    const w = work.get('0199a6b2-0000-7000-8000-00000000a001')!;
    work.byId = { ...work.byId, [w.id]: { ...w, project_id: 'billing' } };
    bar(item('SHOP-142'), null);
    await fireEvent.click(screen.getByRole('button', { name: 'Say' }));
    await waitFor(() => expect(runs()[0]?.ctx.extra).toEqual({ project_id: 'billing' }));
  });

  it('shows the target project before a pane-opening action, never for a toast', async () => {
    withPlugins({ id: 'qa', ticket: [toast, screenAction] });
    ui.view = 'now';
    bar(item('4567'), null); // billing, not the active project
    const open = async (name: string) => {
      const btn = await waitFor(() => screen.getByRole('button', { name: 'Plugins ▾' })); // not busy
      await fireEvent.click(btn);
      await fireEvent.click(await screen.findByRole('menuitem', { name: new RegExp(name) }));
    };
    await open('Say');
    await waitFor(() => expect(runs()).toHaveLength(1));
    expect(ui.view).toBe('now');
    await open('Open my screen');
    await waitFor(() => expect(runs()).toHaveLength(2));
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('billing');
    expect(runs()[1]!.ctx.extra).toEqual({ project_id: 'billing' });
  });

  it('renders one button, a keyboard menu for several, nothing for none or a disabled plugin', async () => {
    withPlugins({ id: 'off', enabled: false, ticket: [toast] });
    const { unmount } = bar(item('SHOP-120'));
    expect(screen.queryByRole('button', { name: 'Say' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Plugins ▾' })).toBeNull();
    unmount();

    withPlugins(
      { id: 'a', ticket: [toast] },
      { id: 'b', ticket: [screenAction] },
      { id: 'off', enabled: false, ticket: [toast] },
    );
    bar(item('SHOP-120'));
    await fireEvent.click(screen.getByRole('button', { name: 'Plugins ▾' }));
    const menu = screen.getByRole('menu', { name: 'Plugin actions' });
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((b) => b.textContent?.replace(/\s+/g, ' ').trim()),
    ).toEqual(['Say Plugin a', 'Open my screen Plugin b']);
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'Enter' });
    await waitFor(() => expect(runs()[0]?.command_id).toBe('a/say'));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('shows review actions on the review detail with the PR ref', async () => {
    withPlugins({ id: 'qa', review: [toast] });
    render(ReviewDetailPane, {
      props: {
        projectId: 'shop',
        tabId: 't',
        paneId: 'p',
        content: {
          kind: 'review_detail',
          review: { account: 'github-acme', repo: 'acme/shop-api', number: 311 },
        },
        visible: true,
        focused: true,
      },
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Say' }));
    await waitFor(() => expect(runs()).toHaveLength(1));
    const { ctx } = runs()[0]!;
    expect(ctx.review).toEqual({ account: 'github-acme', repo: 'acme/shop-api', number: 311 });
    expect(ctx.extra).toEqual({ project_id: 'shop' });
  });
});
