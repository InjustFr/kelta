import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

export default {
  'linux.graphics': { label: msg('Graphics workarounds') },
  'linux.graphics.profile': {
    order: 0,
    label: msg('Workarounds'),
    help: msg('Safe turns every workaround below on.'),
  },
  'linux.graphics.auto_nvidia': {
    order: 1,
    label: msg('Fix the NVIDIA driver automatically'),
    help: msg('With the NVIDIA proprietary driver, turns off DMA-BUF rendering and explicit sync.'),
  },
  'linux.graphics.disable_dmabuf': {
    order: 2,
    label: msg('Turn off DMA-BUF rendering'),
    help: msg('Try it if the window stays blank or garbled.'),
  },
  'linux.graphics.nvidia_disable_explicit_sync': {
    order: 3,
    label: msg('Turn off NVIDIA explicit sync'),
    help: msg('Try it if the window freezes or flickers on Wayland with NVIDIA.'),
  },
  'linux.graphics.disable_compositing': {
    order: 4,
    label: msg('Turn off web view compositing'),
    help: msg('Try it if the window flickers. Scrolling gets slower.'),
  },
  'linux.graphics.gdk_backend': {
    order: 5,
    label: msg('Display server'),
    help: msg('Auto uses Wayland when it runs, else X11.'),
  },
} satisfies CopyTable;
