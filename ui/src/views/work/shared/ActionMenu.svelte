<script lang="ts" module>
  /** A row action (#397): what a key does, also reachable from the row's `⋯` menu. */
  export interface RowAction {
    id: string;
    label: string;
    /** The key or chord shown on the right of the menu entry. */
    key?: string;
    /** Why it cannot run now: the entry goes under "Not available now" with this under its label (#405). */
    reason?: string | null;
    run: () => unknown;
  }

  /** Where a menu opened from a control goes: under it. */
  export function below(e: Event): { x: number; y: number } {
    const r = (e.currentTarget as HTMLElement | null)?.getBoundingClientRect();
    return { x: r?.left ?? 0, y: (r?.bottom ?? 0) + 2 };
  }
</script>

<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { Menu } from '$lib/ui';

  // The `⋯` menu of a list row: the row's actions that can run, each with its key on the right like a
  // menu bar, then the ones that cannot under "Not available now".
  interface Props {
    actions: readonly RowAction[];
    x: number;
    y: number;
    onclose: () => void;
  }

  let { actions, x, y, onclose }: Props = $props();
</script>

<Menu
  items={actions.map((a) => ({
    id: a.id,
    label: a.label,
    kbd: a.key,
    reason: a.reason,
  }))}
  {x}
  {y}
  label={t('More actions')}
  onselect={(id) => void actions.find((a) => a.id === id)?.run()}
  {onclose}
/>
