<script lang="ts">
  // "Add a ticket source" (SPEC Design): search what an account offers (`tracker_sources`) and add
  // a hit's ready view to the project's tracker binding. Opened as sheet `tracker.source_picker`.
  import type { SheetProps } from '$app/registry';
  import type { SourceHit } from '$lib/gen';
  import { dispatch } from '$lib/actions';
  import { trackerSources } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, settings, toasts } from '$lib/stores';
  import { Button, Kbd, Select, Sheet, Spinner, TextInput } from '$lib/ui';

  import { kindInfo } from '../settings/lib/accounts';
  import { isAdded, saveTracker, trackerAccountIds, withSource } from '../settings/lib/sources.svelte';

  interface Props extends SheetProps {
    projectId: string;
  }

  let { projectId, onclose }: Props = $props();

  const project = $derived(projects.byId(projectId));
  const binding = $derived(project?.tracker ?? null);
  const accountsCfg = $derived(settings.value()?.accounts ?? {});
  const accountIds = $derived(trackerAccountIds(accountsCfg, binding));
  const kindOf = (id: string) => accountsCfg[id]?.kind;
  const accountOptions = $derived(
    accountIds.map((id) => {
      const kind = kindOf(id);
      return { value: id, label: kind ? `${id} (${kindInfo(kind).label})` : id };
    }),
  );

  let picked = $state<string | null>(null);
  const account = $derived(picked ?? binding?.account ?? accountIds[0] ?? '');
  let query = $state('');
  let hits = $state<SourceHit[]>([]);
  // account the shown hits came from: `account` moves first while a new search is pending
  let hitsAccount = $state('');
  let busy = $state(false);
  let failure = $state<{ unsupported: boolean; message: string } | null>(null);
  let selected = $state(0);
  let adding = $state(false);
  let retry = $state(0);
  let seq = 0;

  $effect(() => {
    if (settings.value() === null) void settings.load().catch(() => undefined);
  });

  $effect(() => {
    const acc = account;
    const q = query.trim();
    void retry;
    failure = null;
    if (!acc) return;
    const mine = ++seq;
    // one-shot: debounce typing, last request wins (same as TicketPicker)
    const timer = setTimeout(async () => {
      busy = true;
      try {
        const found = await trackerSources({ account_id: acc, query: q });
        if (mine === seq) {
          hits = found;
          hitsAccount = acc;
          selected = 0;
        }
      } catch (err) {
        if (mine !== seq) return;
        const e = toIpcError('tracker_sources', err);
        hits = [];
        failure = { unsupported: e.code === 'unsupported', message: e.message };
      } finally {
        if (mine === seq) busy = false;
      }
    }, 250);
    return () => {
      clearTimeout(timer);
      seq++;
    };
  });

  async function add(hit: SourceHit | undefined): Promise<void> {
    if (!hit || adding || isAdded(binding, hitsAccount, hit)) return;
    adding = true;
    try {
      await saveTracker(projectId, withSource(binding, hitsAccount, hit));
    } catch (err) {
      toasts.error(err, `Could not add ${hit.label}`);
    } finally {
      adding = false;
    }
  }

  function editToml(): void {
    onclose();
    void dispatch('settings.open', { section: 'projects', toml: projectId });
  }

  function onkeydown(e: KeyboardEvent): void {
    const n = hits.length;
    if (e.key === 'ArrowDown' && n) {
      e.preventDefault();
      selected = (selected + 1) % n;
    } else if (e.key === 'ArrowUp' && n) {
      e.preventDefault();
      selected = (selected - 1 + n) % n;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      void add(hits[selected]);
    }
  }

  const provider = $derived.by(() => {
    const kind = kindOf(account);
    return kind ? kindInfo(kind).label : account;
  });
</script>

<Sheet title="Add a ticket source" {onclose} width={480}>
  <div class="picker" data-testid="source-picker">
    {#if accountIds.length === 0}
      <p class="state">No tracker account yet. Add one in Settings, Accounts.</p>
      <div>
        <Button size="sm" onclick={() => (onclose(), void dispatch('settings.open', { section: 'accounts' }))}
          >Open accounts</Button
        >
      </div>
    {:else}
      <div class="controls">
        <Select label="Account" value={account} options={accountOptions} onchange={(v) => (picked = v)} />
        <TextInput
          label="Search"
          bind:value={query}
          placeholder="Board, project, filter or repository"
          autocomplete="off"
          autofocus
          spellcheck={false}
          {onkeydown}
          aria-controls="src-hits"
          aria-activedescendant={hits.length ? `src-hit-${selected}` : undefined}
          data-testid="source-search"
        />
      </div>

      {#if failure?.unsupported}
        <p class="state" data-testid="source-unsupported">{provider} can't list sources here.</p>
        <div><Button size="sm" icon="file-code" onclick={editToml}>Edit TOML</Button></div>
      {:else if failure}
        <p class="state" role="alert">Could not list sources: {failure.message}</p>
        <div><Button size="sm" onclick={() => retry++}>Try again</Button></div>
      {:else if hits.length === 0}
        <p class="state">
          {#if busy}<Spinner size={12} />{:else if query.trim()}No source matches “{query.trim()}”.{:else}This
            account offers no source.{/if}
        </p>
      {:else}
        <div class="list" id="src-hits" role="listbox" aria-label="Sources" aria-busy={busy}>
          {#each hits as hit, i (hit.view.id)}
            {@const added = isAdded(binding, hitsAccount, hit)}
            <!-- svelte-ignore a11y_click_events_have_key_events (the search field drives the keyboard) -->
            <div
              class="row"
              id="src-hit-{i}"
              class:selected={i === selected}
              class:added
              role="option"
              tabindex="-1"
              aria-selected={i === selected}
              aria-disabled={added}
              data-testid="source-hit"
              onpointermove={() => (selected = i)}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => void add(hit)}
            >
              <span class="kind">{hit.kind}</span>
              <span class="label">{hit.label}</span>
              {#if hit.detail}<span class="detail">{hit.detail}</span>{/if}
              {#if added}<span class="done">Added</span>{:else if i === selected}<Kbd chord="enter" />{/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</Sheet>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .controls {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .state {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0;
    color: var(--k-fg-muted);
  }

  .list {
    margin: 0 calc(var(--k-space-3) * -1);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-row-height);
    padding: 0 var(--k-space-3);
    color: var(--k-fg);
    cursor: pointer;
  }

  .row.selected {
    background: var(--k-bg-selected);
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .row.added {
    cursor: default;
  }

  .kind {
    flex: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 56px;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .kind::first-letter {
    text-transform: uppercase;
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .done,
  .row :global(.k-kbd) {
    margin-left: auto;
    flex: none;
  }

  .done {
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }
</style>
