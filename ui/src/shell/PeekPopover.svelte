<script lang="ts">
  // Peek & reply (ticket #139): the waiting session's last lines, its menu as buttons, a reply field.
  import { sessionTextTail, sessionWrite } from '$lib/ipc/commands';
  import { projects, sessions, toasts } from '$lib/stores';
  import { Button, Kbd } from '$lib/ui';

  import { sessionLabel } from '../views/work/live';
  import { flushJump } from './jumplist';
  import { revealSession } from './nav';
  import { peek, PEEK_WIDTH, sendReply } from './peek.svelte';
  import { detectMenu } from './peekMenu';

  const TAIL_LINES = 15;
  /** Full popover: 15 tail lines, preview, a 3-option menu, reply and footer. */
  const PEEK_HEIGHT = 500;

  let root = $state<HTMLElement>();
  let input = $state<HTMLInputElement>();
  let reply = $state('');
  let tail = $state('');

  const s = $derived(peek.id ? sessions.get(peek.id) : null);
  const live = $derived(s?.lifecycle === 'live');
  // The hook said permission / elicitation (needs_input): only then is a numbered list a menu.
  const menu = $derived(live && s?.status === 'needs_input' ? detectMenu(tail) : null);
  const style = $derived.by(() => {
    if (!peek.at) return `left: calc(50% - ${PEEK_WIDTH / 2}px); top: 48px;`;
    const top = Math.max(8, Math.min(peek.at.y, window.innerHeight - PEEK_HEIGHT - 8));
    return `left: ${peek.at.x}px; top: ${top}px;`;
  });

  $effect(() => {
    // Reload on every status change (the menu appears or goes away), not on each output frame.
    const id = peek.id;
    void s?.status;
    void s?.lifecycle;
    if (!id) return;
    sessionTextTail({ id, max_lines: TAIL_LINES })
      .then((t) => {
        if (peek.id === id) tail = t;
      })
      .catch(() => {
        if (peek.id === id) tail = '';
      });
  });

  $effect(() => {
    // Another session (Tab, hover): its own reply and tail.
    void peek.id;
    reply = '';
    tail = '';
  });

  $effect(() => {
    if (peek.id && !s) peek.close();
  });

  $effect(() => {
    if (!peek.focused || !root) return;
    (input ?? root.querySelector<HTMLElement>('button') ?? root).focus();
  });

  $effect(() => {
    if (!peek.id) return;
    // A click outside closes it without taking focus back.
    const onDown = (e: PointerEvent): void => {
      if (root && e.target instanceof Node && !root.contains(e.target)) peek.close(false);
    };
    window.addEventListener('pointerdown', onDown, true);
    return () => window.removeEventListener('pointerdown', onDown, true);
  });

  async function run(write: () => Promise<void>): Promise<void> {
    try {
      await write();
      peek.close();
    } catch (err) {
      toasts.error(err, 'Sending to the session failed');
    }
  }

  /** `Mod+Enter`, Resume: go to the pane for real (recorded in the jumplist). */
  function jump(): void {
    const id = peek.id;
    if (!id) return;
    peek.close(false);
    flushJump();
    void revealSession(id);
  }

  function onkeydown(e: KeyboardEvent): void {
    const id = peek.id;
    if (!id) return;
    if (e.key === 'Escape') peek.close();
    else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) jump();
    else if (e.key === 'Tab' && !e.shiftKey) peek.next();
    // Never paste over a menu: it ignores the paste and the \r confirms the highlighted option.
    else if (e.key === 'Enter' && live && !menu && reply.trim()) void run(() => sendReply(id, reply));
    else if (menu && !reply && menu.some((o) => o.key === e.key))
      void run(() => sessionWrite(id, e.key)); // digits go raw
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

{#if peek.id && s}
  <div
    bind:this={root}
    class="peek"
    {style}
    role="dialog"
    aria-label="Peek: {s.name}"
    tabindex="-1"
    aria-modal={peek.focused}
    data-testid="peek"
    data-session-id={s.id}
    {onkeydown}
    onmouseenter={() => peek.hold()}
    onmouseleave={() => peek.leave()}
  >
    <header>
      <span class="proj">{projects.byId(s.project_id)?.name ?? ''}</span>
      <span class="name">{sessionLabel(s)}</span>
    </header>
    <pre class="tail" data-testid="peek-tail">{tail}</pre>
    {#if s.claude?.preview}<p class="preview" title={s.claude.preview}>{s.claude.preview}</p>{/if}
    {#if !live}
      <Button variant="primary" size="sm" onclick={jump} data-testid="peek-resume">Resume</Button>
    {:else}
      {#if menu}
        <div class="menu" data-testid="peek-menu">
          {#each menu as o (o.key)}
            <Button size="sm" onclick={() => void run(() => sessionWrite(s.id, o.key))}>
              <Kbd chord={o.key} />
              {o.label}
            </Button>
          {/each}
        </div>
      {/if}
      <input
        bind:this={input}
        bind:value={reply}
        class="k-filter reply"
        placeholder="Reply"
        aria-label="Reply to {s.name}"
        data-testid="peek-reply"
      />
    {/if}
    <footer>
      {#if live && !menu}<span><Kbd chord="enter" /> Send</span>{/if}
      <span><Kbd chord="tab" /> Next waiting</span>
      <span><Kbd chord="mod+enter" /> Go to</span>
      <span><Kbd chord="escape" /> Close</span>
    </footer>
  </div>
{/if}

<style>
  .peek {
    position: fixed;
    z-index: var(--k-z-menu);
    width: 480px;
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
  }

  header {
    display: flex;
    gap: var(--k-space-3);
    min-width: 0;
  }

  .proj,
  .preview,
  footer {
    color: var(--k-fg-muted);
  }

  .name,
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tail {
    margin: 0;
    padding: var(--k-space-2);
    max-height: 16lh;
    overflow: auto;
    border-radius: var(--k-radius);
    background: var(--k-bg-sunken);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
    white-space: pre;
  }

  .preview {
    margin: 0;
  }

  .menu {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--k-space-2);
  }

  .menu :global(.k-button) {
    justify-content: flex-start;
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-4);
    font-size: var(--k-font-size-xs);
  }
</style>
