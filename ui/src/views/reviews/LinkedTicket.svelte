<script lang="ts">
  // A linked ticket key as a button. A span, not a <button>: in the Reviews list it sits inside the row button.
  import type { ProjectId } from '$lib/gen';
  import { Badge } from '$lib/ui';

  import { openLinkedTicket } from './linkedTicket';

  interface Props {
    ticketKey: string;
    projectId: ProjectId;
    /** -1 in the list (the pane owns the keyboard), 0 where the badge is the only way in. */
    tabindex?: 0 | -1;
  }

  let { ticketKey, projectId, tabindex = 0 }: Props = $props();

  function open(e: Event): void {
    e.stopPropagation();
    void openLinkedTicket(ticketKey, projectId);
  }
</script>

<span
  class="link"
  role="button"
  {tabindex}
  title={`Open ${ticketKey}`}
  aria-label={`Open ${ticketKey}`}
  onclick={open}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      open(e);
    }
  }}
>
  <Badge tone="info">{ticketKey}</Badge>
</span>

<style>
  .link {
    display: inline-flex;
    cursor: pointer;
    border-radius: var(--k-radius-sm);
  }

  .link:hover :global(.k-badge) {
    text-decoration: underline;
  }

  .link:focus-visible {
    outline: 1px solid var(--k-focus);
    outline-offset: 1px;
  }
</style>
