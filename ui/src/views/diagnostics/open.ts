import { t } from '$lib/i18n/index.svelte';
import { layout, projects, toasts } from '$lib/stores';

import { activateProject } from '../../shell/nav';

/** Opens the Diagnostics pane in a new tab of the active project. */
export async function openDiagnostics(): Promise<void> {
  const projectId = projects.activeId ?? projects.home?.id ?? 'home';
  try {
    await layout.ensure(projectId);
    await activateProject(projectId); // from a global view the pane would open out of sight
    layout.open(projectId, {
      content: { kind: 'diagnostics' },
      placement: 'new_tab',
      focus: true,
      tab_title: 'Diagnostics',
      work_item_id: null,
    });
  } catch (err) {
    toasts.error(err, t('Could not open diagnostics'));
  }
}
