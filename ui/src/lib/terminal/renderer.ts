// Renderer selection (ARCHITECTURE §9.2): `terminal.renderer` = auto (WebGL on macOS, DOM on Linux)
// | webgl | dom. The WebGL addon is a dynamic import; at most 8 WebGL contexts live at once (browsers
// drop the oldest beyond ~16); context loss disposes the addon and falls back to the DOM renderer.

import type { Renderer } from '$lib/gen';

export type RendererChoice = 'webgl' | 'dom';
export type RenderPlatform = 'macos' | 'linux';

/** Hard cap of concurrent WebGL contexts. */
export const MAX_WEBGL_CONTEXTS = 8;

export function chooseRenderer(
  setting: Renderer,
  platform: RenderPlatform,
  webglAvailable: boolean,
  liveContexts: number,
  cap: number = MAX_WEBGL_CONTEXTS,
): RendererChoice {
  if (!webglAvailable || liveContexts >= cap) return 'dom';
  switch (setting) {
    case 'dom':
      return 'dom';
    case 'webgl':
      return 'webgl';
    case 'auto':
      return platform === 'macos' ? 'webgl' : 'dom';
  }
}

let contexts = 0;

/** Reserves a WebGL context slot. Returns false when the cap is reached. */
export function acquireWebglContext(cap: number = MAX_WEBGL_CONTEXTS): boolean {
  if (contexts >= cap) return false;
  contexts += 1;
  return true;
}

export function releaseWebglContext(): void {
  contexts = Math.max(0, contexts - 1);
}

export function activeWebglContexts(): number {
  return contexts;
}

/** Test helper. */
export function resetWebglContextsForTests(): void {
  contexts = 0;
}

let webglProbe: boolean | null = null;

/** WebGL capability probe (cached): can a WebGL2 context be created at all? */
export function webglSupported(): boolean {
  if (webglProbe !== null) return webglProbe;
  try {
    const canvas = document.createElement('canvas');
    const gl = canvas.getContext('webgl2');
    webglProbe = gl !== null;
    // Release the probe context right away (it counts towards the browser's context limit).
    (gl as WebGL2RenderingContext | null)?.getExtension('WEBGL_lose_context')?.loseContext();
  } catch {
    webglProbe = false;
  }
  return webglProbe;
}

/** Test helper. */
export function setWebglSupportedForTests(value: boolean | null): void {
  webglProbe = value;
}

/**
 * Linux first-run probe verdict (§9.2): suggest WebGL when it renders ≥ 55 fps while DOM stays
 * under 45 fps in the scroll test.
 */
export function probeVerdict(webglFps: number, domFps: number): 'suggest-webgl' | 'keep-dom' {
  return webglFps >= 55 && domFps < 45 ? 'suggest-webgl' : 'keep-dom';
}
