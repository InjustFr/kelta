// settings.open (Mod+,): opens a Settings tab in the active project, or focuses the existing one.
// Optional args: { section?: string } to land on a given section.

import { registerAction } from '$lib/actions';
import { toasts, layout, projects } from '$lib/stores';

registerAction('settings.open', async (args) => {
  const projectId = projects.activeId ?? projects.home?.id ?? 'home';
  const section = typeof args?.section === 'string' ? args.section : null;
  try {
    await layout.ensure(projectId);
    layout.open(projectId, {
      content: { kind: 'settings', section },
      placement: 'new_tab',
      focus: true,
      tab_title: 'Settings',
      work_item_id: null,
    });
  } catch (err) {
    toasts.error(err, 'Could not open settings');
  }
});
