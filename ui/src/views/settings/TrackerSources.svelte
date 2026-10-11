<script lang="ts">
  // Settings → Sources → Board: the account and the views of one source (`[sources.tracker]`).
  // Whose tickets, current iteration, a Jira view's board and statuses, removal and the status colours
  // save at once to the global `[[sources]]`.
  import { msg, t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { Column, SourceHit, TrackerView, Who } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { settings, ui } from '$lib/stores';
  import { Badge, Button, IconButton, Select, Toggle } from '$lib/ui';

  import { confirms } from '../../shell/confirm.svelte';
  import ColorPicker from '../onboarding/ColorPicker.svelte';
  import SettingsGroup from './layout/SettingsGroup.svelte';
  import { editSource, iterationWord, viewAccount } from './lib/sources.svelte';

  interface Props {
    sourceId: string;
  }

  let { sourceId }: Props = $props();

  const all = $derived(settings.value()?.sources ?? []);
  const source = $derived(all.find((s) => s.id === sourceId) ?? null);
  const kindOf = (id: string) => settings.value()?.accounts[id]?.kind;

  const WHO: { value: Who | ''; label: string }[] = $derived([
    { value: 'mine', label: t('Mine') },
    { value: 'unassigned', label: t('Unassigned') },
    { value: 'anyone', label: t('Team') },
  ]);
  // Whole phrases: the iteration word takes a different gender / case per language.
  const CURRENT_ONLY: Record<string, string> = {
    sprint: msg('Current sprint only'),
    cycle: msg('Current cycle only'),
    milestone: msg('Current milestone only'),
    version: msg('Current version only'),
    iteration: msg('Current iteration only'),
  };
  // A view without `who` keeps its own query (jql, assigned_to, scope): say so rather than guess,
  // and keep the choice for any view with a query so trying a who can be undone.
  const hasQuery = (v: TrackerView) =>
    !!(v.jql || v.query_id != null || v.assigned_to || v.scope || v.search);
  const whoOptions = (v: TrackerView) =>
    v.who && !hasQuery(v) ? WHO : [{ value: '' as const, label: t('Set by query') }, ...WHO];

  const save = (f: (views: TrackerView[]) => TrackerView[]) =>
    editSource(sourceId, (s) => ({ ...s, tracker: { ...s.tracker, views: f(s.tracker.views) } }));

  const patch = (id: string, p: Partial<TrackerView>) =>
    save((views) => views.map((v) => (v.id === id ? { ...v, ...p } : v)));

  // Jira: boards per account (from the source picker's discovery) and the statuses of the source's
  // columns, which follow its board. A failed load only leaves the choice empty.
  let boards = $state<Record<string, SourceHit[]>>({});
  let columns = $state<Column[]>([]);
  // Strings, so a save that leaves them alone does not reload anything.
  const jiraAccounts = $derived(
    source
      ? [...new Set(source.tracker.views.map((v) => viewAccount(source.tracker, v)))]
          .filter((a) => kindOf(a) === 'jira')
          .join('\n')
      : '',
  );
  $effect(() => {
    const accounts = jiraAccounts.split('\n').filter(Boolean);
    untrack(() => {
      for (const account_id of accounts.filter((a) => !(a in boards))) {
        boards[account_id] = [];
        ipc
          .trackerSources({ account_id, query: '' })
          .then((hits) => {
            boards[account_id] = hits.filter((h) => h.kind === 'board');
          })
          .catch(() => undefined);
      }
    });
  });
  // The columns follow the binding's own columns, the views' boards and their JQL's project.
  const columnsKey = $derived(
    JSON.stringify([source?.tracker.columns, source?.tracker.views.map((v) => [v.board_id, v.jql])]),
  );
  $effect(() => {
    void columnsKey;
    ipc
      .trackerColumns({ source_id: sourceId })
      .then((cols) => (columns = cols))
      .catch(() => (columns = []));
  });
  // Done statuses never reach an open view: offer the others.
  const statusNames = $derived([
    ...new Set(columns.filter((c) => c.category !== 'done').flatMap((c) => c.match_names)),
  ]);
  // Every status of the board, plus the coloured ones it no longer lists (so they can be cleared).
  const colorNames = $derived([
    ...new Set([...columns.flatMap((c) => c.match_names), ...Object.keys(source?.status_colors ?? {})]),
  ]);
  const setColor = (name: string, color: string | null) =>
    editSource(sourceId, (s) => {
      const rest = Object.fromEntries(Object.entries(s.status_colors).filter(([k]) => k !== name));
      return { ...s, status_colors: color ? { ...rest, [name]: color } : rest };
    });

  const boardOptions = (account: string, v: TrackerView) => {
    const own = (boards[account] ?? []).map((h) => ({ value: String(h.view.board_id), label: h.label }));
    const id = v.board_id == null ? '' : String(v.board_id);
    const known = id === '' || own.some((o) => o.value === id);
    // Without a jql or project of its own the board is the view's only scope: it cannot be cleared.
    const scoped = !!(v.jql?.trim() || v.project_id?.trim() || v.project?.trim());
    return [
      ...(scoped || id === '' ? [{ value: '', label: t('Any board') }] : []),
      ...(known ? [] : [{ value: id, label: t('Board {id}', { id }) }]),
      ...own,
    ];
  };
  // The view's own statuses stay listed, so one the board no longer has can still be cleared.
  const statusOptions = (v: TrackerView) => [...new Set([...statusNames, ...(v.statuses ?? [])])];
  const toggleStatus = (v: TrackerView, name: string, on: boolean) => {
    const next = on ? [...(v.statuses ?? []), name] : (v.statuses ?? []).filter((n) => n !== name);
    void patch(v.id, { statuses: next.length > 0 ? next : null });
  };

  /** A view can hold a hand-written query: removal is confirmed (it writes the TOML at once). */
  async function remove(v: TrackerView): Promise<void> {
    const answer = await confirms.ask({
      title: t('Remove {name}?', { name: v.label }),
      body: t('{source} stops listing tickets from this view. Its query is not kept.', {
        source: source?.label ?? t('The source'),
      }),
      tone: 'danger',
      actions: [{ id: 'remove', label: t('Remove view'), variant: 'danger' }],
    });
    if (answer === 'remove') await save((views) => views.filter((x) => x.id !== v.id));
  }
</script>

{#if source}
  {@const b = source.tracker}
  <SettingsGroup heading={t('Board')} data-testid="tracker-block" data-source-id={source.id}>
    {#snippet meta()}<Badge tone="neutral">{b.account}</Badge>{/snippet}
    {#snippet actions()}
      <Button
        size="sm"
        icon="plus"
        onclick={() => ui.openSheet('tracker.source_picker', { sourceId: source.id })}
        data-testid="add-source">{t('Add view…')}</Button
      >
    {/snippet}
    {#if b.views.length === 0}
      <p class="empty">{t('No view: {name} lists no ticket.', { name: source.label })}</p>
    {:else}
      <ul>
        {#each b.views as v (v.id)}
          {@const acc = viewAccount(b, v)}
          {@const word = iterationWord(kindOf(acc), v)}
          <li data-testid="tracker-source" data-view-id={v.id}>
            <span class="label">{v.label}</span>
            <span class="account">{acc}</span>
            <span class="who">
              <label class="k-visually-hidden" for="who-{source.id}-{v.id}"
                >{t('Whose tickets in {name}', { name: v.label })}</label
              >
              <Select
                value={v.who ?? ''}
                options={whoOptions(v)}
                onchange={(who) => void patch(v.id, { who: who || null })}
                id="who-{source.id}-{v.id}"
              />
            </span>
            {#if word}
              <Toggle
                label={t(CURRENT_ONLY[word] ?? word)}
                checked={v.current_iteration}
                onchange={(on) => void patch(v.id, { current_iteration: on })}
              />
            {/if}
            <IconButton
              icon="trash-2"
              label={t('Remove {name}', { name: v.label })}
              size="sm"
              onclick={() => void remove(v)}
            />
            {#if kindOf(acc) === 'jira'}
              <div class="filters">
                <label class="k-visually-hidden" for="board-{source.id}-{v.id}"
                  >{t('Board of {name}', { name: v.label })}</label
                >
                <Select
                  value={v.board_id == null ? '' : String(v.board_id)}
                  options={boardOptions(acc, v)}
                  onchange={(id) => void patch(v.id, { board_id: id ? Number(id) : null })}
                  id="board-{source.id}-{v.id}"
                />
                {#if statusOptions(v).length > 0}
                  <fieldset>
                    <legend>{t('Statuses (none: every open status)')}</legend>
                    {#each statusOptions(v) as name (name)}
                      <Toggle
                        label={name}
                        checked={v.statuses?.includes(name) ?? false}
                        onchange={(on) => toggleStatus(v, name, on)}
                      />
                    {/each}
                  </fieldset>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </SettingsGroup>
  {#if colorNames.length > 0}
    <SettingsGroup heading={t('Status colours')} data-testid="status-colors">
      <p class="empty">{t('A status without a colour keeps the colour of its category.')}</p>
      <ul>
        {#each colorNames as name (name)}
          <li data-status={name}>
            <span class="label">{name}</span>
            <ColorPicker
              label={t('Colour of {status}', { status: name })}
              value={source.status_colors[name] ?? null}
              onchange={(c) => void setColor(name, c)}
            />
          </li>
        {/each}
      </ul>
    </SettingsGroup>
  {/if}
{/if}

<style>
  .empty {
    margin: 0;
    color: var(--k-fg-muted);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-4);
    min-height: var(--k-row-height);
    border-bottom: 1px solid var(--k-border);
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .filters {
    display: flex;
    flex-basis: 100%;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-4);
    padding-bottom: var(--k-space-3);
  }

  fieldset {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2) var(--k-space-4);
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    float: left;
    margin-right: var(--k-space-3);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .account {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }
</style>
