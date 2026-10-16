<script lang="ts">
  // Make this a work item (#502, FLOW §4.3): a running Claude session becomes a ticket item (the item
  // its MCP `claim_ticket` would make) or a scratch item on a worktree. It keeps its conversation and cwd.
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import type { SessionId, TicketItem } from '$lib/gen';
  import { sessionClaim } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, sessions, toasts, work } from '$lib/stores';
  import { Button, currentPlatform, IconButton, Select, Sheet, TextInput } from '$lib/ui';

  import { goToWork } from './nav';
  import TicketPicker from './TicketPicker.svelte';

  interface Props extends SheetProps {
    sessionId: SessionId;
  }
  let { sessionId, onclose }: Props = $props();

  const s = $derived(sessions.get(sessionId));
  let ticket = $state<TicketItem | null>(null);
  let picking = $state(false);
  let choice = $state(sessions.get(sessionId)?.cwd ?? '');
  let custom = $state('');
  let busy = $state(false);
  let failure = $state<string | null>(null);

  // shortcut: offers the session's folder and the repo checkouts only (no `git worktree list` command);
  // another worktree is typed and Core refuses a path that is not one. Upgrade with a worktree-list command.
  const options = $derived.by(() => {
    const out: { value: string; label: string }[] = s
      ? [{ value: s.cwd, label: t('Session folder: {path}', { path: s.cwd }) }]
      : [];
    for (const r of (s && projects.byId(s.project_id)?.repos) || [])
      if (!out.some((o) => o.value === r.path)) out.push({ value: r.path, label: `${r.id} (${r.path})` });
    out.push({ value: '__custom', label: t('Other folder…') });
    return out;
  });
  const worktree = $derived(choice === '__custom' ? custom.trim() : choice);

  async function submit(): Promise<void> {
    if (!s || busy || worktree === '') return;
    busy = true;
    failure = null;
    try {
      const item = await sessionClaim({
        id: sessionId,
        ticket: ticket ? { ...ticket.ticket.ref } : null,
        worktree,
      });
      work.upsert(item);
      toasts.info(t('{branch} is now a work item', { branch: item.branch }));
      onclose();
      await goToWork(item); // opening the item moves the session beside the editor
    } catch (err) {
      failure = toIpcError('session_claim', err).message;
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      void submit();
    }
  }
</script>

<Sheet title={t('Make this a work item')} width={480} {onclose}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    id="make-work-form"
    class="form"
    data-testid="make-work"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}
    {onkeydown}
  >
    <p class="lede">
      {t(
        'Claude keeps its conversation and its folder. The editor and git of the work item use the worktree below.',
      )}
    </p>
    <div class="ticket">
      <span class="label">{t('Ticket')}</span>
      {#if ticket}
        <span class="picked" data-testid="make-work-ticket">
          <code>{ticket.ticket.ref.key}</code>
          <span class="ttl">{ticket.ticket.title}</span>
          <IconButton icon="x" label={t('Remove ticket')} size="sm" onclick={() => (ticket = null)} />
        </span>
      {:else}
        <Button size="sm" variant="ghost" icon="ticket" onclick={() => (picking = true)}
          >{t('None. Link one…')}</Button
        >
      {/if}
    </div>
    <Select label={t('Worktree')} {options} bind:value={choice} />
    {#if choice === '__custom'}
      <TextInput
        label={t('Folder')}
        class="mono"
        bind:value={custom}
        hint={t('A git worktree of one of the project repos')}
        spellcheck={false}
      />
    {/if}
    {#if failure}<p class="fail" role="alert" data-testid="make-work-error">{failure}</p>{/if}
  </form>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>{t('Cancel')}</Button>
    <Button
      variant="primary"
      type="submit"
      form="make-work-form"
      loading={busy}
      disabled={worktree === ''}
      data-testid="make-work-submit"
      chord={currentPlatform() === 'macos' ? 'cmd+enter' : 'ctrl+enter'}>{t('Make work item')}</Button
    >
  {/snippet}
</Sheet>

{#if picking}
  <TicketPicker
    title={t('Link a ticket')}
    onpick={(hit) => {
      ticket = hit;
      picking = false;
    }}
    onclose={() => (picking = false)}
  />
{/if}

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .lede {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .form :global(.mono) {
    font-family: var(--k-font-mono);
  }

  .ticket {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-1);
  }

  .label {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .picked {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    max-width: 100%;
  }

  .ttl {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fail {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    color: var(--k-danger);
  }
</style>
