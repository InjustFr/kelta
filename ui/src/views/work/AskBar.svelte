<script lang="ts">
  // MCP `ask` (#356): the session's question with one button per option. A button types the option
  // and Enter; while Claude shows its own prompt (needs_input) they wait, or Enter would answer it.
  import { t } from '$lib/i18n/index.svelte';
  import type { SessionInfo } from '$lib/gen';
  import { Button, HtmlContent } from '$lib/ui';

  import { answer } from '../../shell/peek.svelte';
  import { questionOf } from './phase';

  interface Props {
    session: SessionInfo;
    /** Peek: the question and its Markdown body above the buttons. Rows: the buttons alone. */
    full?: boolean;
    /** The Queue's brief: `1`–`9` on the first nine buttons (the pane handles the keys). */
    numbered?: boolean;
    onanswer?: () => void;
  }

  let { session, full = false, numbered = false, onanswer }: Props = $props();

  const q = $derived(questionOf(session));
  const busy = $derived(session.status === 'needs_input');
</script>

{#if q}
  <div class="ask" class:full data-testid="ask" data-session-id={session.id}>
    {#if full}
      <p class="q">{q.question}</p>
      {#if q.body_html}<HtmlContent html={q.body_html} class="body" />{/if}
    {/if}
    <div class="opts" role="group" aria-label={q.question}>
      {#each q.options as o, i (i)}
        <Button
          size="sm"
          disabled={busy}
          chord={numbered && i < 9 ? String(i + 1) : undefined}
          title={busy ? t('Answer the prompt open in this session first') : undefined}
          onclick={(e: MouseEvent) => {
            e.stopPropagation();
            void answer(session.id, o).then((ok) => ok && onanswer?.());
          }}>{o}</Button
        >
      {/each}
    </div>
  </div>
{/if}

<style>
  .ask {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .q {
    margin: 0;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .ask.full :global(.body) {
    max-height: 12lh;
    overflow-y: auto;
  }

  .opts {
    display: flex;
    gap: var(--k-space-2);
    min-width: 0;
    overflow: hidden;
  }

  .ask.full .opts {
    flex-wrap: wrap;
  }
</style>
