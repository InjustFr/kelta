<script lang="ts">
  import { projects, toasts } from '$lib/stores';

  import { activateProject } from '../nav';
  import { rank } from './fuzzy';
  import Picker from './Picker.svelte';

  interface Props {
    onclose: () => void;
    /** Picker mode (`ui.pickProject`): Home is left out and the pick is handed over, not activated. */
    placeholder?: string;
    onpick?: (id: string) => void;
  }

  let { onclose, placeholder, onpick }: Props = $props();

  let query = $state('');

  const results = $derived(
    rank(
      projects.list
        .filter((p) => !onpick || !p.builtin)
        .map((p) => ({
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
    if (onpick) return onpick(id);
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
  title={onpick ? 'Choose a project' : 'Switch project'}
  placeholder={placeholder ?? 'Project name…'}
  items={results}
  {query}
  onquery={(q) => (query = q)}
  onpick={(id) => void pick(id)}
  {onclose}
  testid="switcher"
  empty="No matching project"
/>
