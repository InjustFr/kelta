// Loads the plugin list once the backend answered (projects loaded). This also starts the backend
// plugin host, whose trigger engine subscribes to the bus on first use (see contract-requests/L8.md).

import { plugins, projects } from '$lib/stores';

$effect.root(() => {
  $effect(() => {
    const p = plugins.plugins;
    // Once: a failed load is not retried here (Settings → Plugins retries).
    if (projects.loaded && p.fetchedAt === null && !p.loading && !p.error) void plugins.load();
  });
});
