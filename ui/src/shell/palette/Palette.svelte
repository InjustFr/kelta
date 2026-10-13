<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { trackerSearch } from '$lib/ipc/commands';
  import type { TicketItem } from '$lib/gen';
  import { settings } from '$lib/stores';

  import { rank } from './fuzzy';
  import Picker from './Picker.svelte';
  import {
    buildItems,
    GROUP_ORDER,
    itemText,
    parseQuery,
    recentItems,
    ticketItems,
    type PaletteItem,
  } from './sources';

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

  // Settings keys come from the schema, fetched once per app run.
  void settings.loadSchema();

  const base = $derived(buildItems());
  const parsed = $derived(parseQuery(query));
  const wantsTickets = $derived(!parsed.groups || parsed.groups.includes('Tickets'));

  const results = $derived.by((): PaletteItem[] => {
    const { groups, text: q } = parsed;
    const pool = groups ? base.filter((i) => groups.includes(i.group)) : base;
    const local = rank(pool, q, itemText, 60);
    if (q === '') {
      // Group order for the empty query; sessions needing input stay first within their group.
      const sorted = [...local].sort((a, b) => GROUP_ORDER.indexOf(a.group) - GROUP_ORDER.indexOf(b.group));
      return groups ? sorted : [...recentItems(base), ...sorted];
    }
    return [...local, ...(wantsTickets ? ticketItems(hits) : [])];
  });

  $effect(() => {
    // Tickets come from the cached tracker search; typing arms one debounce timer.
    const q = parsed.text;
    hits = [];
    if (q.length < 2 || !wantsTickets) {
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
  title={t('Command palette')}
  placeholder={t('Search actions, projects, sessions, tickets…')}
  items={results.map((r) => ({
    id: r.id,
    label: r.label,
    detail: r.detail,
    icon: r.icon,
    kbd: r.kbd,
    group: r.group,
    lamp: r.lamp,
  }))}
  {query}
  onquery={(q) => (query = q)}
  onpick={pick}
  {onclose}
  busy={searching}
  testid="palette"
  empty={`Nothing matches "${query}".`}
/>
