import { layout, projects, toasts } from '$lib/stores';

/** Opens the Diagnostics pane in a new tab of the active project. */
export async function openDiagnostics(): Promise<void> {
  const projectId = projects.activeId ?? projects.home?.id ?? 'home';
  try {
    await layout.ensure(projectId);
    layout.open(projectId, {
      content: { kind: 'diagnostics' },
      placement: 'new_tab',
      focus: true,
      tab_title: 'Diagnostics',
      work_item_id: null,
    });
  } catch (err) {
    toasts.error(err, 'Could not open diagnostics');
  }
}
