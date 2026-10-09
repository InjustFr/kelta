<script lang="ts">
  import { projects, toasts } from '$lib/stores';

  import { activateProject } from '../nav';
  import { rank } from './fuzzy';
  import Picker from './Picker.svelte';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let query = $state('');

  const results = $derived(
    rank(
      projects.list.map((p) => ({
        id: p.id,
        label: p.name,
        detail: p.builtin ? 'Home' : p.open ? (p.active ? 'active' : 'open') : 'not open',
        icon: p.builtin ? 'house' : 'folder',
      })),
      query,
      (i) => `${i.label} ${i.id}`,
    ),
  );

  async function pick(id: string): Promise<void> {
    onclose();
    try {
      const p = projects.byId(id);
      if (p && !p.open) await projects.open(id);
      await activateProject(id);
    } catch (err) {
      toasts.error(err, 'Opening the project failed');
    }
  }
</script>

<Picker
  title="Switch project"
  placeholder="Project name…"
  items={results}
  {query}
  onquery={(q) => (query = q)}
  onpick={(id) => void pick(id)}
  {onclose}
  testid="switcher"
  empty="No matching project"
/>
