// E2E harness for the L9 views. The real shell belongs to L2, so this page mounts a tiny host
// (active tab → panes via the registries, sheet host, toast host) on the in-memory IPC mock.
// It mirrors src/main.ts: same stores, same action modules, same debug handles.

import '../../../../src/styles/tokens.css';
import '../../../../src/styles/base.css';

import { mount } from 'svelte';

import * as registry from '$app/registry';
import * as actions from '$lib/actions';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import * as stores from '$lib/stores';
import { injectSprite } from '$lib/ui/icons';

import Harness from './Harness.svelte';

import.meta.glob(['../../../../src/shell/actions.ts', '../../../../src/views/*/actions.ts'], { eager: true });

const { transport, controls } = createMockTransport();
setTransport(transport);
window.__keltaMock = controls;
window.__kelta = { stores, registry, actions };

injectSprite();
const target = document.getElementById('app');
if (!target) throw new Error('#app missing');
mount(Harness, { target });
void stores.bootstrap();
