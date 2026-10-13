<script lang="ts">
  // The Story panel's Claude section (#427): status with its age, the task list, the last message
  // (folded after 4 lines, `e` expands), cost against the budget, context and model.
  import { untrack } from 'svelte';

  import { t, tn } from '$lib/i18n/index.svelte';

  import type { ClaudeTaskState, WorkItem } from '$lib/gen';
  import { sessions, settings } from '$lib/stores';
  import { Button, Icon, relativeTime } from '$lib/ui';

  import { statusLabel } from '../../../shell/labels';
  import { ctxHot, itemCost, overBudget, usd } from '../../../shell/usage';
  import { blockedReason, runWorkAction } from '../actions';
  import { claudeOf } from '../live';
  import { questionOf } from '../phase';
  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  const claude = $derived(claudeOf(item));
  const meta = $derived(claude?.claude ?? null);
  const tasks = $derived(meta?.tasks ?? []);
  const done = $derived(tasks.filter((k) => k.state === 'completed').length);
  const spent = $derived(itemCost(item, sessions.all));
  const budget = $derived(settings.value(item.project_id)?.claude.budget_usd);
  const ctx = $derived(meta?.usage?.context_pct ?? null);
  const canPark = $derived(blockedReason('park', item) === null);

  // The session has no "working since": the age counts from the moment this window saw it start.
  // shortcut: a panel opened mid-work shows no age, until a status-change time reaches SessionInfo.
  let workingSince = $state<{ id: string; at: number } | null>(null);
  $effect(() => {
    const id = claude?.id;
    const working = claude?.status === 'working';
    untrack(() => {
      workingSince = working && id ? (workingSince?.id === id ? workingSince : { id, at: Date.now() }) : null;
    });
  });

  const status = $derived.by(() => {
    if (!claude) return t('Not started');
    if (questionOf(claude)) return t('Asks you');
    const label = statusLabel(claude.status);
    if (claude.status === 'working') {
      const mins = workingSince ? Math.max(1, Math.round((Date.now() - workingSince.at) / 60_000)) : 0;
      return workingSince ? `${label} ${t('{n} min', { n: mins })}` : label;
    }
    return [label, item.claude_at ? relativeTime(Date.parse(item.claude_at)) : null]
      .filter(Boolean)
      .join(' ');
  });
  const summary = $derived(
    claude
      ? [
          status,
          tasks.length ? tn('{done} of {n} task', '{done} of {n} tasks', tasks.length, { done }) : null,
          spent > 0 ? usd(spent) : null,
        ]
          .filter(Boolean)
          .join(' · ')
      : status,
  );

  const MARK: Record<ClaudeTaskState, { icon: string; label: () => string }> = {
    completed: { icon: 'circle-check', label: () => t('Completed') },
    in_progress: { icon: 'circle-dot', label: () => t('In progress') },
    pending: { icon: 'circle', label: () => t('Pending') },
  };

  let expanded = $state(false);
  let overflows = $state(false);
  let messageEl = $state<HTMLElement>();
  $effect(() => {
    const el = messageEl;
    void meta?.preview;
    if (el && !expanded) overflows = el.scrollHeight > el.clientHeight + 1;
  });

  function onkey(e: KeyboardEvent): void {
    if (e.key !== 'e' || e.metaKey || e.ctrlKey || e.altKey || !meta?.preview) return;
    expanded = !expanded;
    e.preventDefault();
  }
</script>

{#if claude}
  <Section title={t('Claude')} {summary} {open} testid="story-claude">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="body" role="group" aria-label={t('Claude')} tabindex="-1" onkeydown={onkey}>
      <div class="status" data-testid="claude-status">{status}</div>
      {#if tasks.length}
        <ul class="tasks" aria-label={t('Tasks')} data-testid="claude-tasks">
          {#each tasks as k, i (k.id ?? i)}
            <li class={k.state} aria-current={k.state === 'in_progress' ? 'step' : undefined}>
              <Icon name={MARK[k.state].icon} size={14} label={MARK[k.state].label()} />
              <span>{k.title}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if meta?.preview}
        <div class="msg">
          <div class="text" class:clamped={!expanded} bind:this={messageEl} data-testid="claude-message">
            {meta.preview}
          </div>
          {#if overflows || expanded}
            <Button size="sm" variant="ghost" chord="e" onclick={() => (expanded = !expanded)}
              >{expanded ? t('Show less') : t('Show more')}</Button
            >
          {/if}
        </div>
      {/if}
      <div class="meta">
        {#if spent > 0 || budget != null}
          <span class:over={overBudget(spent, budget)} data-testid="claude-cost"
            >{usd(spent)}{budget != null ? ` / ${usd(budget)}` : ''}</span
          >
        {/if}
        {#if ctx != null}
          <span class:over={meta?.usage && ctxHot(meta.usage)}
            >{t('Context {pct}%', { pct: Math.round(ctx) })}</span
          >
        {/if}
        {#if meta?.model}<span>{meta.model}</span>{/if}
      </div>
      <div class="actions">
        <Button size="sm" variant="secondary" chord="g" onclick={() => void runWorkAction('go_claude', item)}
          >{t('Go to Claude')}</Button
        >
        {#if canPark}
          <Button size="sm" variant="ghost" chord="P" onclick={() => void runWorkAction('park', item)}
            >{t('Park')}</Button
          >
        {/if}
      </div>
    </div>
  </Section>
{:else}
  <Section title={t('Claude')} {summary} {open} testid="story-claude" />
{/if}

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .status {
    font-weight: 600;
  }

  .tasks {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tasks li {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-2);
    color: var(--k-fg-muted);
  }

  .tasks li.in_progress {
    color: var(--k-fg);
    font-weight: 600;
  }

  .tasks li.completed {
    text-decoration: line-through;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .text.clamped {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    overflow: hidden;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-3);
    color: var(--k-fg-muted);
  }

  .over {
    color: var(--k-danger);
  }

  .actions {
    display: flex;
    gap: var(--k-space-2);
  }
</style>
