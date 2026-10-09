<script lang="ts">
  import { trackerSearch } from '$lib/ipc/commands';
  import type { TicketItem } from '$lib/gen';

  import { rank } from './fuzzy';
  import Picker from './Picker.svelte';
  import { buildItems, GROUP_ORDER, itemText, ticketItems, type PaletteItem } from './sources';

  interface Props {
    initialQuery?: string;
    onclose: () => void;
  }

  let { initialQuery = '', onclose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let query = $state(initialQuery);
  let hits = $state<TicketItem[]>([]);
  let searching = $state(false);
  let seq = 0;

  const base = $derived(buildItems());

  const results = $derived.by((): PaletteItem[] => {
    const q = query.trim();
    const local = rank(base, q, itemText, 60);
    if (q === '') {
      // Group order for the empty query; sessions needing input stay first within their group.
      return [...local].sort((a, b) => GROUP_ORDER.indexOf(a.group) - GROUP_ORDER.indexOf(b.group));
    }
    return [...local, ...ticketItems(hits)].slice(0, 80);
  });

  $effect(() => {
    // Tickets come from the cached tracker search; typing arms one debounce timer.
    const q = query.trim();
    hits = [];
    if (q.length < 2) {
      searching = false;
      return;
    }
    const mine = ++seq;
    // one-shot: debounce the ticket search while typing
    const timer = setTimeout(async () => {
      searching = true;
      try {
        const found = await trackerSearch({ scope: { kind: 'all' }, text: q });
        if (mine === seq) hits = found.slice(0, 20);
      } catch {
        // tracker unavailable (no account, offline): the palette still works without tickets
      } finally {
        if (mine === seq) searching = false;
      }
    }, 250);
    return () => {
      clearTimeout(timer);
      seq++; // drop an in-flight search when the query changes or clears
    };
  });

  function pick(id: string): void {
    const item = results.find((r) => r.id === id);
    onclose();
    if (item) void Promise.resolve(item.run());
  }
</script>

<Picker
  title="Command palette"
  placeholder="Search actions, projects, sessions, tickets…"
  items={results.map((r) => ({
    id: r.id,
    label: r.label,
    detail: r.detail,
    icon: r.icon,
    kbd: r.kbd,
    group: r.group,
  }))}
  {query}
  onquery={(q) => (query = q)}
  onpick={pick}
  {onclose}
  busy={searching}
  testid="palette"
  empty="No matching action, project, session or ticket"
/>
