<script lang="ts">
  // Since you left (#248, in the Story panel since #423): where you left a work item, decided when the
  // item is (re)focused; leaving it stamps `left_at` (no timers). A closed panel shows itself for it.
  import { t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { workLeft } from '$lib/ipc/commands';
  import { settings, work } from '$lib/stores';
  import { terminalPool } from '$lib/terminal';
  import { Button, relativeTime } from '$lib/ui';

  import { focusedSessionId } from '../../../shell/nav';
  import { runWorkAction } from '../actions';
  import { claudeOf } from '../live';
  import { returnBrief } from '../phase';
  import { story } from './panel.svelte';
  import Section from './Section.svelte';

  let { workItemId, tabId }: TabHeaderProps & { tabId?: string } = $props();

  const item = $derived(work.get(workItemId));

  let briefFor = $state<string | null>(null);
  let briefEl = $state<HTMLElement>();
  // The strip takes the keyboard once per return, so `x` / `v` never land in the Claude or nvim pane.
  let briefFocus = false;
  $effect(() => {
    const id = workItemId;
    untrack(() => {
      const w = work.get(id);
      const mins = settings.value()?.work.return_brief_after_mins ?? 20;
      briefFor = w && returnBrief(w, claudeOf(w)?.status ?? null, Date.now(), mins) ? id : null;
      briefFocus = briefFor !== null;
      if (briefFor && tabId) story.reveal(tabId);
    });
    return () =>
      void workLeft({ id }).then(
        (w) => work.upsert(w),
        () => {},
      );
  });
  const brief = $derived(
    briefFor === workItemId && item && claudeOf(item)?.status !== 'working' ? item : null,
  );

  $effect(() => {
    const el = briefEl;
    if (el && briefFocus) {
      briefFocus = false;
      // After the Shell's own give-the-keyboard-back-to-the-terminal microtask (closing the switcher).
      setTimeout(() => el.focus());
    }
  });

  function dismissBrief(): void {
    briefFor = null;
    const sid = focusedSessionId();
    if (sid) terminalPool.focus(sid);
  }

  function briefKey(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey || !brief) return;
    if (e.key === 'x') dismissBrief();
    else if (e.key === 'v') void runWorkAction('review_delta', brief);
    else return;
    e.preventDefault();
  }
</script>

{#if brief}
  <Section
    title={t('Since you left')}
    summary={brief.left_at ? relativeTime(Date.parse(brief.left_at)) : ''}
    open
    testid="story-since"
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="brief"
      role="region"
      aria-label={t('Where you left off')}
      data-testid="return-brief"
      tabindex="-1"
      bind:this={briefEl}
      onkeydown={briefKey}
    >
      <div class="brief-text">
        {#if brief.next_note}<div class="note"><b>{t('next:')}</b> {brief.next_note}</div>{/if}
        {#if brief.delta}
          <div class="since">
            <span class="ins">+{brief.delta.insertions}</span>/<span class="del"
              >−{brief.delta.deletions}</span
            >
            {t('since you reviewed')}
          </div>
        {/if}
        {#if brief.claude_message}
          <details>
            <summary>{brief.claude_message.split('\n')[0]}</summary>
            <pre>{brief.claude_message}</pre>
          </details>
        {/if}
      </div>
      {#if brief.delta}
        <Button
          size="sm"
          variant="secondary"
          chord="v"
          onclick={() => void runWorkAction('review_delta', brief)}>{t('Review changes')}</Button
        >
      {/if}
      <Button size="sm" variant="ghost" chord="x" onclick={dismissBrief}>{t('Dismiss')}</Button>
    </div>
  </Section>
{/if}

<style>
  .brief {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--k-space-2);
  }

  .brief-text {
    flex: 1 0 100%;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
  }

  .brief summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  .brief pre {
    max-height: 30vh;
    margin: var(--k-space-1) 0 0;
    overflow: auto;
    white-space: pre-wrap;
    font: inherit;
  }

  .ins {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }
</style>
