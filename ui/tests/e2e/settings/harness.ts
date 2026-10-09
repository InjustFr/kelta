// Mounts the L4 views on their own against the in-memory IPC mock, so the specs do not depend on
// the shell. `?view=settings|diagnostics|onboarding|project_new&section=<id>`.
import '../../../src/styles/tokens.css';
import '../../../src/styles/base.css';

import { mount } from 'svelte';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import * as stores from '$lib/stores';
import { injectSprite } from '$lib/ui/icons';

import DiagnosticsPane from '../../../src/views/diagnostics/DiagnosticsPane.svelte';
import OnboardingSheet from '../../../src/views/onboarding/OnboardingSheet.svelte';
import ProjectNewSheet from '../../../src/views/onboarding/ProjectNewSheet.svelte';
import SettingsPane from '../../../src/views/settings/SettingsPane.svelte';

const { transport, controls } = createMockTransport();
setTransport(transport);
window.__keltaMock = controls;
(window as unknown as { __stores: typeof stores }).__stores = stores;
injectSprite();

const params = new URLSearchParams(location.search);
const view = params.get('view') ?? 'settings';
const target = document.getElementById('app')!;
const pane = { projectId: 'shop', tabId: 't1', paneId: 'p1', visible: true, focused: true };

void stores.bootstrap().then(() => {
  const onclose = (): void => {
    document.body.dataset.closed = view;
  };
  if (view === 'settings') {
    mount(SettingsPane, {
      target,
      props: { ...pane, content: { kind: 'settings', section: params.get('section') } },
    });
  } else if (view === 'diagnostics') {
    mount(DiagnosticsPane, { target, props: { ...pane, content: { kind: 'diagnostics' } } });
  } else if (view === 'onboarding') {
    mount(OnboardingSheet, { target, props: { onclose } });
  } else if (view === 'project_new') {
    mount(ProjectNewSheet, { target, props: { onclose } });
  }
});
