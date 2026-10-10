// settings.open (Mod+,): opens a Settings tab in the active project, or focuses the existing one.
// Optional args: { section?: string } to land on a given section, { toml?: projectId } to land on
// that project's raw TOML (the Edit TOML mode, for what the forms cannot edit).

import { registerAction } from '$lib/actions';
import { toasts, layout, projects } from '$lib/stores';

let tomlRequest: string | null = null;

/** Project whose TOML the next Settings pane opens on, once. */
export function takeTomlRequest(): string | null {
  const id = tomlRequest;
  tomlRequest = null;
  return id;
}

registerAction('settings.open', async (args) => {
  const projectId = projects.activeId ?? projects.home?.id ?? 'home';
  const section = typeof args?.section === 'string' ? args.section : null;
  // PaneContent::Settings only carries a section, so the TOML target travels beside it.
  tomlRequest = typeof args?.toml === 'string' ? args.toml : null;
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
