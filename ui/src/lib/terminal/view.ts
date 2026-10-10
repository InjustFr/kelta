// TerminalView: one xterm instance bound to one session (ARCHITECTURE §9). Created lazily by the
// pool; owns the DOM host element (re-parented on show/hide), the channel attachment, frame
// decoding with batched acks, resize coalescing, renderer selection, clipboard, search and the
// terminal-specific key handling.

import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal, type ITheme } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';

import type { SessionId } from '$lib/gen';
import {
  clipboardRead,
  clipboardWrite,
  openExternal,
  sessionAck,
  sessionAttach,
  sessionDetach,
  sessionResize,
  sessionWrite,
} from '$lib/ipc/commands';

import { fontStack, type TerminalConfig } from './config';
import { AckBatcher, FrameHandler } from './frames';
import { encodeKittyKey } from './kitty';
import { baseCharacter, macOptionIsMeta, optionMetaSequence, shiftEnterFor } from './keymap';
import { needsPasteConfirmation, sanitizePaste } from './paste';
import { installQueryHandlers } from './queries';
import { frames } from './raf';
import {
  acquireWebglContext,
  activeWebglContexts,
  chooseRenderer,
  releaseWebglContext,
  webglSupported,
} from './renderer';
import { xtermTheme } from './theme';

import type { SearchAddon } from '@xterm/addon-search';
import type { PoolView } from './index';

export interface TerminalViewDeps {
  config: () => TerminalConfig;
  platform: 'macos' | 'linux';
  /** Session kind name (`claude`, `shell`, `editor`, `tool`, …): selects the Shift+Enter mapping. */
  kindOf: (sessionId: SessionId) => string;
  /** Routes a keydown through the key manager; true = consumed (xterm must not see it). */
  onKey: (event: KeyboardEvent) => boolean;
  /** Reports failures the user should see (clipboard, links). */
  onError?: (err: unknown, context: string) => void;
}

export interface ViewState {
  attaching: boolean;
  attached: boolean;
  exited: boolean;
  exitCode: number | null;
  /** The attach failed (spawn error, missing binary…). */
  error: { code: string; message: string } | null;
}

/** Requests a view makes to the pane that shows it. */
export type ViewRequest = 'restart' | 'close';

const SEARCH_DECORATIONS = {
  matchBackground: '#6a5a1d',
  matchOverviewRuler: '#d7ba3a',
  activeMatchBackground: '#a8731a',
  activeMatchColorOverviewRuler: '#f0a020',
};

export class TerminalView implements PoolView {
  readonly id: SessionId;
  readonly host: HTMLDivElement;
  readonly term: Terminal;

  /** Confirms a multi-line paste into a prompt without bracketed paste. */
  confirmPaste: ((text: string) => Promise<boolean>) | null = null;
  /** Receives restart/close requests from the exit banner keys. */
  onRequest: ((request: ViewRequest) => void) | null = null;
  /** Bytes received and not acked yet (kelta-bench flood scenario). */
  inflight = 0;

  #deps: TerminalViewDeps;
  #fit = new FitAddon();
  #search: SearchAddon | null = null;
  #searchLoading: Promise<SearchAddon> | null = null;
  #container: HTMLElement | null = null;
  #observer: ResizeObserver | null = null;
  #opened = false;
  #disposed = false;
  #acks: AckBatcher;
  #handler: FrameHandler | null = null;
  #generation: number | null = null;
  #attachSeq = 0;
  #attachPromise: Promise<void> | null = null;
  #sent = { cols: 0, rows: 0 };
  #state: ViewState = { attaching: false, attached: false, exited: false, exitCode: null, error: null };
  #listeners = new Set<(state: ViewState) => void>();
  #webgl: { dispose(): void } | null = null;
  #leftOption = false;
  #rightOption = false;
  #focused = false;
  #cfg: TerminalConfig;
  #disposers: (() => void)[] = [];
  /** Kitty keyboard flags of the session (Keyboard frames; 0 = legacy encoding). */
  #kittyFlags = 0;
  /** Keys whose keydown a Kelta shortcut took: their keyup is not reported either. */
  #consumed = new Set<string>();

  constructor(id: SessionId, deps: TerminalViewDeps) {
    this.id = id;
    this.#deps = deps;
    this.#cfg = deps.config();
    this.#acks = new AckBatcher((generation, bytes) => {
      this.inflight = Math.max(0, this.inflight - bytes); // snapshots are acked but not counted
      void sessionAck({ id: this.id, generation, bytes }).catch(() => {});
    });

    this.host = document.createElement('div');
    this.host.className = 'k-term-host';
    this.host.dataset.terminal = '';
    this.host.dataset.session = id;
    this.host.dataset.kind = deps.kindOf(id);
    this.host.style.cssText = 'position:absolute;inset:4px 4px 2px 6px;';

    this.term = new Terminal({
      allowProposedApi: true,
      ...this.#termOptions(this.#cfg),
      cursorBlink: false,
      cursorInactiveStyle: 'outline',
      scrollOnUserInput: true,
      macOptionClickForcesSelection: true,
      linkHandler: {
        activate: (event, uri) => this.#openLink(event, uri),
        allowNonHttpProtocols: false,
      },
    });
    this.term.loadAddon(this.#fit);
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = '11';
    this.term.loadAddon(new WebLinksAddon((event, uri) => this.#openLink(event, uri)));
    for (const d of installQueryHandlers(this.term.parser)) this.#disposers.push(() => d.dispose());

    this.term.attachCustomKeyEventHandler((event) => this.#handleKey(event));
    const onData = this.term.onData((data) => this.#write(data));
    const onBinary = this.term.onBinary((data) => {
      // Binary mouse reports (X10 coordinates > 127) are raw bytes, not UTF-8.
      const bytes = new Uint8Array(data.length);
      for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i) & 0xff;
      this.#writeBytes(bytes);
    });
    this.#disposers.push(
      () => onData.dispose(),
      () => onBinary.dispose(),
    );

    this.#listen(this.host, 'mouseup', (e) => this.#onMouseUp(e as MouseEvent), true);
    this.#listen(this.host, 'mousedown', (e) => this.#onMouseDown(e as MouseEvent), true);
    this.#listen(this.host, 'focusin', () => this.#setFocusState(true));
    this.#listen(this.host, 'focusout', () => this.#setFocusState(false));
  }

  // ---- state ---------------------------------------------------------------------------------

  get state(): ViewState {
    return this.#state;
  }

  onState(listener: (state: ViewState) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #setState(patch: Partial<ViewState>): void {
    this.#state = { ...this.#state, ...patch };
    for (const l of [...this.#listeners]) l(this.#state);
  }

  // ---- mounting ------------------------------------------------------------------------------

  mount(container: HTMLElement): void {
    if (this.#disposed) return;
    this.#container = container;
    container.appendChild(this.host);
    if (!this.#opened) {
      this.#opened = true;
      this.term.open(this.host);
      void this.#loadRenderer();
    } else {
      this.term.refresh(0, Math.max(0, this.term.rows - 1));
    }
    this.#observer?.disconnect();
    if (typeof ResizeObserver !== 'undefined') {
      this.#observer = new ResizeObserver(() => this.requestFit());
      this.#observer.observe(container);
    }
    this.requestFit();
  }

  unmount(): void {
    this.#observer?.disconnect();
    this.#observer = null;
    frames.cancel(this.#fitKey);
    this.host.remove();
    this.#container = null;
    this.#setFocusState(false);
  }

  focus(): void {
    if (!this.#disposed && this.#container) this.term.focus();
  }

  blur(): void {
    this.term.blur();
  }

  // ---- attach / detach -----------------------------------------------------------------------

  attach(): Promise<void> {
    if (this.#disposed) return Promise.resolve();
    if (this.#state.attached) return Promise.resolve();
    if (this.#attachPromise) return this.#attachPromise;
    const seq = ++this.#attachSeq;
    this.#setState({ attaching: true, error: null });
    const p: Promise<void> = this.#doAttach(seq).finally(() => {
      if (this.#attachPromise === p) this.#attachPromise = null;
    });
    this.#attachPromise = p;
    return p;
  }

  async #doAttach(seq: number): Promise<void> {
    const { cols, rows } = this.#dimensions();
    if (this.term.cols !== cols || this.term.rows !== rows) this.term.resize(cols, rows);
    const handler = new FrameHandler({
      term: this.term,
      acks: this.#acks,
      onExit: (code) => this.#setState({ exited: true, exitCode: code }),
      onSnapshot: () => {
        this.#kittyFlags = 0;
      },
      onKeyboard: (flags) => {
        this.#kittyFlags = flags;
      },
      onData: (n) => (this.inflight += n),
    });
    this.inflight = 0;
    this.#handler = handler;
    try {
      const info = await sessionAttach({ id: this.id, cols, rows }, (frame) => {
        if (this.#attachSeq === seq && !this.#disposed) handler.handle(frame);
      });
      if (this.#disposed || this.#attachSeq !== seq) {
        // Superseded while attaching: release the attachment that was just created.
        void sessionDetach({ id: this.id, generation: info.generation }).catch(() => {});
        return;
      }
      this.#generation = info.generation;
      this.#sent = { cols, rows };
      this.#acks.setGeneration(info.generation);
      this.#setState({ attaching: false, attached: true });
      if (info.cols !== this.term.cols || info.rows !== this.term.rows)
        this.term.resize(info.cols, info.rows);
      this.requestFit();
    } catch (err) {
      if (this.#attachSeq !== seq) return;
      handler.close();
      const e = err as { code?: string; message?: string };
      this.#setState({
        attaching: false,
        attached: false,
        error: { code: e.code ?? 'internal', message: e.message ?? String(err) },
      });
    }
  }

  /** Detaches from the session (the process keeps running). */
  detach(): void {
    this.#attachSeq += 1;
    this.#attachPromise = null;
    this.#handler?.close();
    this.#handler = null;
    const generation = this.#generation;
    this.#generation = null;
    this.#acks.reset();
    this.#setState({ attached: false, attaching: false });
    if (generation !== null) void sessionDetach({ id: this.id, generation }).catch(() => {});
  }

  /** Detach + attach again (after a restart): the backend replies with a fresh snapshot. */
  async reattach(): Promise<void> {
    this.detach();
    this.#setState({ exited: false, exitCode: null });
    await this.attach();
  }

  dispose(): void {
    if (this.#disposed) return;
    this.detach();
    this.#disposed = true;
    frames.cancel(this.#fitKey);
    this.#observer?.disconnect();
    for (const d of this.#disposers) d();
    this.#disposers = [];
    this.#releaseWebgl();
    this.term.dispose();
    this.host.remove();
    this.#listeners.clear();
  }

  // ---- sizing --------------------------------------------------------------------------------

  readonly #fitKey = {};

  /** Coalesces fit + resize IPC to one per animation frame. */
  requestFit(): void {
    if (this.#disposed || !this.#container) return;
    frames.schedule(this.#fitKey, () => this.#fitNow());
  }

  #dimensions(): { cols: number; rows: number } {
    const proposed = this.#opened && this.#container ? this.#fit.proposeDimensions() : undefined;
    if (proposed && Number.isFinite(proposed.cols) && Number.isFinite(proposed.rows) && proposed.cols > 0) {
      return { cols: proposed.cols, rows: proposed.rows };
    }
    return { cols: this.term.cols || 80, rows: this.term.rows || 24 };
  }

  #fitNow(): void {
    if (this.#disposed || !this.#container) return;
    const { cols, rows } = this.#dimensions();
    if (cols !== this.term.cols || rows !== this.term.rows) this.term.resize(cols, rows);
    if (
      this.#generation !== null &&
      (this.#sent.cols !== this.term.cols || this.#sent.rows !== this.term.rows)
    ) {
      this.#sent = { cols: this.term.cols, rows: this.term.rows };
      void sessionResize({ id: this.id, cols: this.term.cols, rows: this.term.rows }).catch(() => {});
    }
  }

  // ---- configuration -------------------------------------------------------------------------

  #termOptions(cfg: TerminalConfig) {
    return {
      fontFamily: fontStack(cfg.fontFamily),
      fontSize: cfg.fontSize,
      lineHeight: cfg.lineHeight,
      letterSpacing: cfg.letterSpacing,
      cursorStyle: cfg.cursorStyle,
      scrollback: cfg.scrollback,
      minimumContrastRatio: cfg.minimumContrastRatio,
      macOptionIsMeta: this.#deps.platform === 'macos' && macOptionIsMeta(cfg.optionAsMeta),
      theme: xtermTheme(cfg.theme) as ITheme,
    };
  }

  /** Applies new settings / theme to a live view (font changes re-fit visible views). */
  applyConfig(cfg: TerminalConfig): void {
    if (this.#disposed) return;
    const fontChanged =
      cfg.fontFamily !== this.#cfg.fontFamily ||
      cfg.fontSize !== this.#cfg.fontSize ||
      cfg.lineHeight !== this.#cfg.lineHeight ||
      cfg.letterSpacing !== this.#cfg.letterSpacing;
    this.#cfg = cfg;
    const o = this.#termOptions(cfg);
    const t = this.term.options;
    t.fontFamily = o.fontFamily;
    t.fontSize = o.fontSize;
    t.lineHeight = o.lineHeight;
    t.letterSpacing = o.letterSpacing;
    t.cursorStyle = o.cursorStyle;
    t.scrollback = o.scrollback;
    t.minimumContrastRatio = o.minimumContrastRatio;
    t.macOptionIsMeta = o.macOptionIsMeta;
    t.theme = o.theme;
    t.cursorBlink = this.#focused && cfg.cursorBlink;
    this.host.dataset.kind = this.#deps.kindOf(this.id);
    if (fontChanged) this.requestFit();
  }

  /** Cursor blink only on the focused pane and only when enabled. */
  #setFocusState(focused: boolean): void {
    this.#focused = focused;
    this.term.options.cursorBlink = focused && this.#cfg.cursorBlink;
  }

  // ---- renderer ------------------------------------------------------------------------------

  async #loadRenderer(): Promise<void> {
    const choice = chooseRenderer(
      this.#cfg.renderer,
      this.#deps.platform,
      webglSupported(),
      activeWebglContexts(),
    );
    if (choice !== 'webgl' || !acquireWebglContext()) return;
    try {
      const { WebglAddon } = await import('@xterm/addon-webgl');
      if (this.#disposed) {
        releaseWebglContext();
        return;
      }
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        // Context lost (GPU reset, too many contexts): fall back to the DOM renderer.
        this.#releaseWebgl();
      });
      this.term.loadAddon(addon);
      this.#webgl = addon;
    } catch {
      releaseWebglContext();
    }
  }

  #releaseWebgl(): void {
    const addon = this.#webgl;
    if (!addon) return;
    this.#webgl = null;
    try {
      addon.dispose();
    } catch {
      // already disposed by the context loss path
    }
    releaseWebglContext();
  }

  /** Renderer in use (`webgl` or `dom`), for the perf HUD and tests. */
  get renderer(): 'webgl' | 'dom' {
    return this.#webgl ? 'webgl' : 'dom';
  }

  // ---- input ---------------------------------------------------------------------------------

  #write(data: string): void {
    if (this.#state.exited) return;
    void sessionWrite(this.id, data).catch((err) => this.#deps.onError?.(err, 'Writing to the terminal'));
  }

  #writeBytes(bytes: Uint8Array): void {
    if (this.#state.exited) return;
    void sessionWrite(this.id, bytes).catch((err) => this.#deps.onError?.(err, 'Writing to the terminal'));
  }

  /** Sends text as if typed (used for Shift+Enter and by tests). */
  send(text: string): void {
    this.#write(text);
  }

  /** xterm custom key handler: returns false when the key must not reach xterm. */
  #handleKey(event: KeyboardEvent): boolean {
    if (event.code === 'AltLeft') this.#leftOption = event.type === 'keydown';
    else if (event.code === 'AltRight') this.#rightOption = event.type === 'keydown';
    if (event.type === 'keyup') {
      if (!this.#consumed.delete(event.code)) this.#sendKitty(event);
      return true;
    }
    if (event.type !== 'keydown') return true;

    if (this.#deps.onKey(event)) {
      this.#consumed.add(event.code);
      return false;
    }

    if (this.#state.exited && !event.ctrlKey && !event.altKey && !event.metaKey) {
      if (event.key === 'Enter') {
        event.preventDefault();
        this.onRequest?.('restart');
        return false;
      }
      if (event.key === 'x') {
        event.preventDefault();
        this.onRequest?.('close');
        return false;
      }
    }

    if (this.#sendKitty(event)) {
      event.preventDefault();
      return false;
    }

    const shiftEnter = shiftEnterFor(event, this.#cfg.shiftEnter, this.#deps.kindOf(this.id));
    if (shiftEnter !== null) {
      event.preventDefault();
      this.#write(shiftEnter);
      return false;
    }

    if (this.#deps.platform === 'macos') {
      const meta = optionMetaSequence(event, this.#cfg.optionAsMeta, this.#leftOption, this.#rightOption);
      if (meta !== null) {
        event.preventDefault();
        this.#write(meta);
        return false;
      }
    }
    return true;
  }

  /** Encodes the key with the kitty keyboard protocol when the session enabled it; true = sent. */
  #sendKitty(event: KeyboardEvent): boolean {
    if (this.#kittyFlags === 0 || this.#state.exited || event.isComposing || event.keyCode === 229)
      return false;
    const macos = this.#deps.platform === 'macos';
    // shortcut: Cmd stays the macOS app/menu layer (copy, paste, quit…), never reported as super.
    if (macos && event.metaKey) return false;
    const mode = this.#cfg.optionAsMeta;
    const optionIsAlt =
      !macos ||
      mode === 'both' ||
      (mode === 'left' && this.#leftOption) ||
      (mode === 'right' && this.#rightOption);
    // Option-as-Meta on macOS: report the physical key, not the character Option composed.
    const key =
      macos && event.altKey && optionIsAlt && (event.key.length === 1 || event.key === 'Dead')
        ? (baseCharacter(event.code, event.shiftKey) ?? event.key)
        : event.key;
    const seq = encodeKittyKey(
      {
        type: event.type,
        key,
        code: event.code,
        location: event.location,
        repeat: event.repeat,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
      },
      this.#kittyFlags,
      optionIsAlt,
    );
    if (seq === null) return false;
    this.#write(seq);
    return true;
  }

  // ---- clipboard -----------------------------------------------------------------------------

  /** Copies the selection to the clipboard. Returns false when nothing is selected. */
  async copy(): Promise<boolean> {
    const text = this.term.getSelection();
    if (!text) return false;
    try {
      await clipboardWrite({ kind: 'clipboard', text });
      return true;
    } catch (err) {
      this.#deps.onError?.(err, 'Copy failed');
      return false;
    }
  }

  /** Pastes the clipboard (or Linux PRIMARY) honouring bracketed paste and the multi-line confirm. */
  async paste(kind: 'clipboard' | 'primary' = 'clipboard'): Promise<void> {
    let text: string;
    try {
      text = await clipboardRead({ kind });
    } catch (err) {
      this.#deps.onError?.(err, 'Paste failed');
      return;
    }
    await this.pasteText(text);
  }

  async pasteText(raw: string): Promise<void> {
    const text = sanitizePaste(raw);
    if (!text || this.#state.exited) return;
    const confirm = needsPasteConfirmation(
      text,
      this.term.modes.bracketedPasteMode,
      this.#cfg.confirmMultilinePaste,
    );
    if (confirm && this.confirmPaste && !(await this.confirmPaste(text))) return;
    this.term.paste(text);
  }

  #onMouseDown(e: MouseEvent): void {
    // Middle click pastes PRIMARY on Linux (unless the application tracks the mouse).
    if (e.button === 1 && this.#middleClickPastes()) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  #onMouseUp(e: MouseEvent): void {
    if (e.button === 1 && this.#middleClickPastes()) {
      e.preventDefault();
      e.stopPropagation();
      void this.paste('primary');
      return;
    }
    if (e.button !== 0 || !this.term.hasSelection()) return;
    const text = this.term.getSelection();
    if (!text) return;
    if (this.#deps.platform === 'linux' && this.#cfg.primarySelection) {
      void clipboardWrite({ kind: 'primary', text }).catch(() => {});
    }
    if (this.#cfg.copyOnSelect) void clipboardWrite({ kind: 'clipboard', text }).catch(() => {});
  }

  #middleClickPastes(): boolean {
    return (
      this.#deps.platform === 'linux' &&
      this.#cfg.primarySelection &&
      this.term.modes.mouseTrackingMode === 'none'
    );
  }

  #openLink(event: MouseEvent, uri: string): void {
    // Opening needs Cmd (macOS) / Ctrl (Linux) so a stray click in an application does not navigate.
    const held = this.#deps.platform === 'macos' ? event.metaKey : event.ctrlKey;
    if (!held) return;
    openExternal({ url: uri }).catch((err) => this.#deps.onError?.(err, 'Opening link failed'));
  }

  // ---- search --------------------------------------------------------------------------------

  async #searchAddon(): Promise<SearchAddon> {
    if (this.#search) return this.#search;
    this.#searchLoading ??= import('@xterm/addon-search').then(({ SearchAddon }) => {
      const addon = new SearchAddon();
      this.term.loadAddon(addon);
      this.#search = addon;
      return addon;
    });
    return this.#searchLoading;
  }

  /** Lazy-loads the search addon on first use. Returns whether the query matched. */
  async find(
    query: string,
    opts: { backwards?: boolean; caseSensitive?: boolean; regex?: boolean } = {},
  ): Promise<boolean> {
    const addon = await this.#searchAddon();
    if (query === '') {
      addon.clearDecorations();
      return false;
    }
    const options = {
      caseSensitive: opts.caseSensitive ?? false,
      regex: opts.regex ?? false,
      incremental: !opts.backwards,
      decorations: SEARCH_DECORATIONS,
    };
    return opts.backwards ? addon.findPrevious(query, options) : addon.findNext(query, options);
  }

  clearSearch(): void {
    this.#search?.clearDecorations();
    this.term.clearSelection();
  }

  // ---- misc ----------------------------------------------------------------------------------

  #listen(target: EventTarget, type: string, handler: (e: Event) => void, capture = false): void {
    target.addEventListener(type, handler, capture);
    this.#disposers.push(() => target.removeEventListener(type, handler, capture));
  }
}
