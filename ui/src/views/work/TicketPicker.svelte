<script lang="ts">
  // Ticket search over `tracker_search` (all bound trackers), used by New work item and Link to ticket.
  import type { TicketItem } from '$lib/gen';
  import { trackerSearch } from '$lib/ipc/commands';

  import Picker from '../../shell/palette/Picker.svelte';

  interface Props {
    title: string;
    onpick: (hit: TicketItem) => void;
    onclose: () => void;
  }

  let { title, onpick, onclose }: Props = $props();

  let query = $state('');
  let hits = $state<TicketItem[]>([]);
  let busy = $state(false);
  let failed = $state<string | null>(null);
  let seq = 0;

  $effect(() => {
    const q = query.trim();
    hits = [];
    failed = null;
    if (q.length < 2) {
      busy = false;
      return;
    }
    const mine = ++seq;
    // one-shot: debounce the search while typing (same as the palette)
    const timer = setTimeout(async () => {
      busy = true;
      try {
        const found = await trackerSearch({ scope: { kind: 'all' }, text: q });
        if (mine === seq) hits = found.slice(0, 30);
      } catch (err) {
        if (mine === seq) failed = err instanceof Error ? err.message : String(err);
      } finally {
        if (mine === seq) busy = false;
      }
    }, 250);
    return () => {
      clearTimeout(timer);
      seq++;
    };
  });

  const key = (h: TicketItem): string => `${h.ticket.ref.account}:${h.ticket.ref.key}`;
</script>

<Picker
  {title}
  placeholder="Search tickets by key or title"
  items={hits.map((h) => ({
    id: key(h),
    label: `${h.ticket.ref.key} ${h.ticket.title}`,
    detail: h.ticket.status.name,
    icon: 'ticket',
  }))}
  {query}
  onquery={(q) => (query = q)}
  onpick={(id) => {
    const hit = hits.find((h) => key(h) === id);
    if (hit) onpick(hit);
  }}
  {onclose}
  {busy}
  testid="ticket-picker"
  empty={failed
    ? `Ticket search failed: ${failed}`
    : query.trim().length < 2
      ? 'Type a ticket key or words from its title.'
      : busy
        ? 'Searching…'
        : 'No ticket matches.'}
/>
