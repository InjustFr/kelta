// Loads the plugin list once the backend answered (projects loaded): the palette and the new-session
// sheet list plugin commands and screens from it.

import { plugins, projects } from '$lib/stores';

$effect.root(() => {
  $effect(() => {
    const p = plugins.plugins;
    // Once: a failed load is not retried here (Settings → Plugins retries).
    if (projects.loaded && p.fetchedAt === null && !p.loading && !p.error) void plugins.load();
  });
});
