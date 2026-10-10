<script lang="ts">
  import { openExternal } from '$lib/ipc/commands';

  interface Props {
    /** HTML already sanitized in Rust (ammonia, ARCH §11.2). Never pass unsanitized HTML. */
    html: string;
    /** Reports a failed `open_external` (defaults to console.error). */
    onerror?: (err: unknown) => void;
    class?: string;
  }

  let {
    html,
    onerror = (err) => console.error('[kelta] open_external failed', err),
    class: className = '',
  }: Props = $props();

  const ALLOWED = /^(https?|mailto):/i;

  /** Every link opens in the system browser via `open_external`; the webview never navigates. */
  function intercept(e: MouseEvent): void {
    const target = e.target instanceof Element ? e.target.closest('a') : null;
    if (!target) return;
    e.preventDefault();
    if (e.type === 'auxclick' && e.button !== 1) return;
    const href = target.getAttribute('href') ?? '';
    if (!ALLOWED.test(href)) return;
    openExternal({ url: href }).catch(onerror);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="k-html k-selectable {className}" onclick={intercept} onauxclick={intercept}>
  <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized in Rust -->
  {@html html}
</div>

<style>
  .k-html {
    max-width: var(--k-measure);
    line-height: var(--k-line-height-read);
    overflow-wrap: anywhere;
  }

  .k-html :global(p) {
    margin: 0 0 0.75em;
  }

  .k-html :global(a) {
    color: var(--k-accent);
    text-decoration: underline;
  }

  .k-html :global(pre),
  .k-html :global(code) {
    font-family: var(--k-font-mono);
    font-size: 0.92em;
    background: var(--k-bezel-raised);
    border-radius: var(--k-radius-sm);
  }

  .k-html :global(pre) {
    padding: var(--k-space-3);
    overflow-x: auto;
  }

  .k-html :global(code) {
    padding: 0 3px;
  }

  .k-html :global(pre code) {
    padding: 0;
    background: none;
  }

  .k-html :global(blockquote) {
    margin: 0 0 0.75em;
    padding-left: var(--k-space-4);
    border-left: 2px solid var(--k-border-strong);
    color: var(--k-fg-muted);
  }

  .k-html :global(img) {
    max-width: 100%;
  }

  .k-html :global(table) {
    border-collapse: collapse;
  }

  .k-html :global(td),
  .k-html :global(th) {
    border: 1px solid var(--k-border);
    padding: 2px 6px;
  }
</style>
