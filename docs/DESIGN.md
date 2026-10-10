# Kelta design spec: Bezel

Status: accepted direction for the UI redesign (lane `ui-redesign`). It replaces the generic white / `#17181b` + `#2f6fdf` palette.
Sources: three proposals (Bezel, Margin and Spine, Faceplate). Bezel is the base. Grafts are marked *(from Faceplate)* or *(from Spine)*.

## 1. Concept

Kelta is the **bezel around a set of screens**. The terminals are the screens: flush, set-in wells that carry all the contrast and identity (the user's font, their ANSI output, nvim's colours). Everything else (rail, tabs, pane headers, status strip, list chrome) is a matte, low-contrast housing that stays dark until a **lamp** needs you.

Three rules:

1. **Chrome has no brand colour.** Its only accent is the active project's own colour, remapped in oklch to a fixed lightness and chroma. The window takes the hue of the project you are in.
2. **Saturated colour in the chrome means state.** Lamps (needs input, working, error, done, activity) are the only saturated marks, and each has its own silhouette, so colour is never the only signal.
3. **Density without noise.** 1px bezel gaps instead of boxed panes, shadows only on floating surfaces, tabular figures for every count, monospace only for strings you could paste into a terminal.

## 2. Tokens

Every existing `--k-*` name is kept, so components that only read tokens need no edit. New names: `--k-bezel`, `--k-bezel-raised`, `--k-well`, `--k-bg-float`, `--k-fg-chrome`, `--k-project`, `--k-gap`, `--k-pane-header-height`, `--k-lamp-*`, `--k-term-selection`, `--k-measure`, `--k-swatch-*`.

`src/styles/tokens.css` becomes:

```css
/* Bezel tokens. Chrome = housing, terminals = screens. No web fonts.
   --k-project is set on <html> by the shell from the active project's colour.
   Accent = project hue at fixed oklch L/C so focus marks always meet 3:1. */
:root {
  --k-font-ui: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Cantarell, Ubuntu, 'Noto Sans', sans-serif;
  --k-font-mono: ui-monospace, 'JetBrains Mono', 'SF Mono', Menlo, 'Cascadia Mono', 'DejaVu Sans Mono', monospace;
  --k-font-size-xs: 11px;   /* meta: ages, priorities, counts */
  --k-font-size-sm: 12px;   /* chrome: tabs, pane headers, status strip */
  --k-font-size: 13px;      /* content: list rows, forms, settings */
  --k-font-size-lg: 15px;   /* view and dialog titles */
  --k-font-size-xl: 18px;   /* ticket / review detail title only */
  --k-line-height: 1.38;
  --k-line-height-read: 1.5;
  --k-measure: 72ch;
  --k-weight-strong: 600;

  --k-space-1: 2px;
  --k-space-2: 4px;
  --k-space-3: 8px;
  --k-space-4: 12px;
  --k-space-5: 16px;
  --k-space-6: 24px;

  --k-radius-sm: 2px;       /* in-pane marks: chips, cards, kbd */
  --k-radius: 4px;          /* controls: buttons, inputs, rail tiles */
  --k-radius-lg: 8px;       /* floating: menus, palette, dialogs, sheets, toasts */

  --k-rail-width: 44px;
  --k-tabbar-height: 30px;
  --k-statusbar-height: 22px;
  --k-pane-header-height: 22px;
  --k-control-height: 26px;
  --k-row-height: 26px;
  --k-gap: 1px;             /* bezel visible between panes instead of borders */

  --k-z-pane: 1;
  --k-z-menu: 30;
  --k-z-sheet: 40;
  --k-z-dialog: 50;
  --k-z-toast: 60;

  --k-duration: 90ms;       /* response to an action only; nothing loops */

  --k-project: #4f8fb3;     /* overwritten per active project */

  /* light: putty housing, paper-white screens */
  --k-bezel: #e3e6e9;
  --k-bezel-raised: #eceef0;
  --k-well: #fafbfb;
  --k-bg: var(--k-well);
  --k-bg-elev: var(--k-bezel-raised);
  --k-bg-sunken: var(--k-bezel);
  --k-bg-float: #ffffff;
  --k-bg-hover: rgba(27, 32, 38, 0.05);
  --k-bg-active: rgba(27, 32, 38, 0.09);
  --k-bg-selected: #e3edf3;                     /* hex fallback, overridden below */
  --k-fg: #1b2026;
  --k-fg-muted: #56606b;
  --k-fg-subtle: #5f6873;                       /* >= 4.5:1 on every light surface */
  --k-fg-chrome: #5d6772;
  --k-border: #cfd4d9;
  --k-border-strong: #b6bdc4;
  --k-accent: #2b6f93;                          /* static fallback accent */
  --k-accent-fg: #ffffff;
  --k-danger: #b8352e;
  --k-danger-fg: #ffffff;
  --k-warn: #8a5c00;
  --k-ok: #2e7a4a;
  --k-info: #2e62a8;
  --k-overlay: rgba(20, 24, 29, 0.28);
  --k-shadow: 0 8px 24px rgba(20, 24, 29, 0.14), 0 0 0 1px rgba(20, 24, 29, 0.08);

  /* lamps: colour + shape (see §6.1) */
  --k-lamp-needs-input: #c93a33;
  --k-lamp-working: #1d7a80;
  --k-lamp-error: #a86f00;
  --k-lamp-done: #2e8550;
  --k-lamp-activity: #737c87;
  --k-att-needs-input: var(--k-lamp-needs-input);
  --k-att-error: var(--k-lamp-error);
  --k-att-done: var(--k-lamp-done);
  --k-att-activity: var(--k-lamp-activity);

  /* terminal (mirror of lib/terminal/theme.ts LIGHT, which is the source of truth) */
  --k-term-bg: #fafbfb;
  --k-term-fg: #1b2026;
  --k-term-cursor: #1b2026;
  --k-term-selection: #c9daea;

  /* project colour picker defaults (custom hex still allowed) */
  --k-swatch-harbor: #3e7cb1;
  --k-swatch-moss: #5e8c4a;
  --k-swatch-ochre: #b88a2e;
  --k-swatch-brick: #b5533c;
  --k-swatch-plum: #8a5a9e;
  --k-swatch-teal: #2f8c86;
  --k-swatch-rose: #b85278;
  --k-swatch-slate: #5f6b7a;
  --k-swatch-olive: #85863a;
  --k-swatch-cobalt: #4b5fc4;

  color-scheme: light;
}

@supports (color: oklch(from red l c h)) {
  :root { --k-accent: oklch(from var(--k-project) 0.52 0.11 h); }
}
@supports (color: color-mix(in oklab, red, blue)) {
  :root { --k-bg-selected: color-mix(in oklab, var(--k-accent) 14%, var(--k-well)); }
}
:root { --k-focus: var(--k-accent); }

/* dark: graphite housing, slate screens. Same block twice (media + explicit). */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) {
    --k-bezel: #1c2026;
    --k-bezel-raised: #21262d;
    --k-well: #121519;
    --k-bg-float: #262b33;
    --k-bg-hover: rgba(216, 221, 227, 0.06);
    --k-bg-active: rgba(216, 221, 227, 0.1);
    --k-bg-selected: #1e2a33;
    --k-fg: #d8dde3;
    --k-fg-muted: #9ca6b1;
    --k-fg-subtle: #88929e;
    --k-fg-chrome: #8e98a4;
    --k-border: #2a3038;
    --k-border-strong: #39414b;
    --k-accent: #6fa8c7;
    --k-accent-fg: #0e1418;
    --k-danger: #e0675f;
    --k-danger-fg: #1a0d0c;
    --k-warn: #d9a03f;
    --k-ok: #6fbf8a;
    --k-info: #7fa6d6;
    --k-overlay: rgba(6, 8, 10, 0.55);
    --k-shadow: 0 10px 30px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(216, 221, 227, 0.06);
    --k-lamp-needs-input: #e5534b;
    --k-lamp-working: #5fb3b8;
    --k-lamp-error: #d9a03f;
    --k-lamp-done: #5db37e;
    --k-lamp-activity: #8b95a1;
    --k-term-bg: #121519;
    --k-term-fg: #d3d9df;
    --k-term-cursor: #d3d9df;
    --k-term-selection: #2c3f52;
    --k-swatch-harbor: #6fa6d6;
    --k-swatch-moss: #8bb874;
    --k-swatch-ochre: #d6ac55;
    --k-swatch-brick: #dd7d64;
    --k-swatch-plum: #b48ac7;
    --k-swatch-teal: #5bb7b0;
    --k-swatch-rose: #de7fa2;
    --k-swatch-slate: #8a95a5;
    --k-swatch-olive: #b2b360;
    --k-swatch-cobalt: #8395ef;
    color-scheme: dark;
  }
  @supports (color: oklch(from red l c h)) {
    :root:not([data-theme='light']) { --k-accent: oklch(from var(--k-project) 0.74 0.09 h); }
  }
  @supports (color: color-mix(in oklab, red, blue)) {
    :root:not([data-theme='light']) { --k-bg-selected: color-mix(in oklab, var(--k-accent) 16%, var(--k-well)); }
  }
}

:root[data-theme='dark'] {
  --k-bezel: #1c2026;
  --k-bezel-raised: #21262d;
  --k-well: #121519;
  --k-bg-float: #262b33;
  --k-bg-hover: rgba(216, 221, 227, 0.06);
  --k-bg-active: rgba(216, 221, 227, 0.1);
  --k-bg-selected: #1e2a33;
  --k-fg: #d8dde3;
  --k-fg-muted: #9ca6b1;
  --k-fg-subtle: #88929e;
  --k-fg-chrome: #8e98a4;
  --k-border: #2a3038;
  --k-border-strong: #39414b;
  --k-accent: #6fa8c7;
  --k-accent-fg: #0e1418;
  --k-danger: #e0675f;
  --k-danger-fg: #1a0d0c;
  --k-warn: #d9a03f;
  --k-ok: #6fbf8a;
  --k-info: #7fa6d6;
  --k-overlay: rgba(6, 8, 10, 0.55);
  --k-shadow: 0 10px 30px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(216, 221, 227, 0.06);
  --k-lamp-needs-input: #e5534b;
  --k-lamp-working: #5fb3b8;
  --k-lamp-error: #d9a03f;
  --k-lamp-done: #5db37e;
  --k-lamp-activity: #8b95a1;
  --k-term-bg: #121519;
  --k-term-fg: #d3d9df;
  --k-term-cursor: #d3d9df;
  --k-term-selection: #2c3f52;
  --k-swatch-harbor: #6fa6d6;
  --k-swatch-moss: #8bb874;
  --k-swatch-ochre: #d6ac55;
  --k-swatch-brick: #dd7d64;
  --k-swatch-plum: #b48ac7;
  --k-swatch-teal: #5bb7b0;
  --k-swatch-rose: #de7fa2;
  --k-swatch-slate: #8a95a5;
  --k-swatch-olive: #b2b360;
  --k-swatch-cobalt: #8395ef;
  color-scheme: dark;
}
@supports (color: oklch(from red l c h)) {
  :root[data-theme='dark'] { --k-accent: oklch(from var(--k-project) 0.74 0.09 h); }
}
@supports (color: color-mix(in oklab, red, blue)) {
  :root[data-theme='dark'] { --k-bg-selected: color-mix(in oklab, var(--k-accent) 16%, var(--k-well)); }
}
```

### 2.1 Project hue

- `Shell.svelte` sets `document.documentElement.style.setProperty('--k-project', color)` in an `$effect` keyed on the active project. No timer.
- If the project has no colour, a colour the browser cannot parse, an oklch chroma below 0.03 (greys, black, white), or a hue within 30° of the needs-input lamp (brick, rose, any red), the shell removes the property so the static fallback accent is used. One JS check in the shell (`shell/accent.ts`): an achromatic colour has no hue to borrow, an invalid one would void every focus ring, and a red accent would make every selected row read as an alarm.
- Switching project never re-pushes the terminal palette: the cursor and selection are neutral.

### 2.2 Contrast floor

| Pair | Light | Dark |
|---|---|---|
| `--k-fg` on `--k-well` | 15.8:1 | 13.4:1 |
| `--k-fg-muted` on `--k-well` / `--k-bg-float` | 6.2 / 6.4:1 | 7.4 / 5.8:1 |
| `--k-fg-subtle` on well / bezel-raised / bezel / float (lowest) | 4.51:1 (bezel) | 4.51:1 (float) |
| `--k-fg-chrome` on `--k-bezel-raised` / `--k-bg-float` | 4.95 / 5.8:1 | 5.2 / 4.9:1 |
| accent (any hue, fixed L) on well | at least 3:1 (focus marks, non-text) | at least 3:1 |

Every text token clears 4.5:1 on every surface it can sit on, so 11-12px labels and state words may use it.
Measured (WCAG 2 relative luminance). Recheck with a script after any token edit.

## 3. Typography

No web fonts and no bundled woff2. Terminals already render in the user's chosen mono font, so the identity lives there.

- **UI face**: `system-ui` stack (SF Pro, Cantarell or Adwaita Sans, the distro sans on Hyprland/Sway, Segoe). All chrome and all list/detail content.
- **Mono**: `ui-monospace` first. Only for strings you could paste into a terminal: ticket keys (`PROJ-142`, `#381`, `!57`), branch names, SHAs, session ids, paths, diffstat numbers, kbd hints, the palette's typed query. Never for labels.

| Step | Size / line | Weight | Use |
|---|---|---|---|
| xs | 11 / 15 | 400 | ages, priorities, counts, status strip meta |
| sm | 12 / 16 | 400, 600 | tabs, pane headers, status strip, group headers (600, `--k-fg-muted`) |
| base | 13 / 18 | 400 | list rows, forms, settings, menu rows |
| lg | 15 / 20 | 600 | view titles, dialog and sheet titles |
| xl | 18 / 23 | 600 | ticket / review detail title only |
| read | 13 / 19.5 | 400 | HtmlContent bodies, max-width `--k-measure` (72ch) |

Rules:
- Weights 400 and 600 only (500 renders poorly in Linux system fonts). 600 is reserved for the active tab, the focused pane title, view titles and group headers.
- `font-variant-numeric: tabular-nums` on the status strip, rail counts, inbox counts, PR +/- stats, ticket ages (`.k-num` in base.css).
- Sentence case everywhere. No all-caps, no tracked labels, no eyebrows. Board lanes keep the tracker's own casing.
- No middle-dot joins (`A · B`). Segments are separated by space (12px) and, where meaning needs it, an icon prefix. No trailing arrows on buttons or links.
- Chrome text (12, `--k-fg-chrome`) reads one step quieter than content (13, `--k-fg`). The terminal is the large type.

## 4. Space, radius, elevation

- **Spacing**: 2 / 4 / 8 / 12 / 16 / 24. Row inner padding is 8px horizontal. Detail panes pad 16px; dialogs 16px; nothing pads a terminal.
- **Radius encodes layer**: in-pane marks 2px, controls 4px, floating surfaces 8px. Panes and terminal wells are square (0).
- **Elevation has three levels and only the top one casts a shadow**:
  1. Bezel (`--k-bezel`): rail, gaps between panes, board lanes.
  2. Bezel-raised (`--k-bezel-raised`): tab bar, status strip, list toolbars.
  3. Well (`--k-well`): terminals, lists, detail panes, pane headers, inputs.
  4. Float (`--k-bg-float` + `--k-shadow` + `--k-radius-lg`): menus, palette, dialogs, sheets, toasts. The only shadows in the app.
- Borders only where they separate data (board cards, inputs, table rows in settings when needed). Panes have no border.

## 5. Terminal palette

`src/lib/terminal/theme.ts` is the source of truth (Rust needs `#rrggbb`). Tokens mirror bg / fg / cursor / selection; a unit test asserts that `--k-term-bg` and `--k-term-fg` in tokens.css equal theme.ts. `terminal_set_palette` re-push on theme change is unchanged.

| | fg | bg | cursor | selection |
|---|---|---|---|---|
| Dark | `#d3d9df` | `#121519` | `#d3d9df` | `#2c3f52` |
| Light | `#1b2026` | `#fafbfb` | `#1b2026` | `#c9daea` |

ANSI 0-15:

```
DARK   #1e232a #e0675f #79b88a #d6b163 #6e9bd1 #b48bcb #5fb3b8 #c4cbd3
       #5e6873 #f08a82 #96d2a6 #e9ca86 #92b6e6 #cba8de #84cbcf #eef1f4
LIGHT  #1b2026 #b8352e #2f7a47 #8a6400 #2e62a8 #87459e #1d7a80 #b9c0c7
       #5f6873 #d24a42 #3e9259 #a87b00 #3f78c2 #a05cb8 #2a9299 #858e98
```

The cursor is neutral, not project-hued and not acid-green. ANSI red/green/yellow/cyan sit near the lamp hues so Claude's own output does not clash with the chrome.

## 6. Components

### 6.1 Lamps (`shell/AttentionDot.svelte`)

One span, silhouette from CSS only (no SVG). Same shapes everywhere: rail, tabs, status strip, inbox, palette, board cards. Nothing pulses or blinks.

| State | Shape | Size | Token |
|---|---|---|---|
| `needs_input` | filled dot with a 2px halo of the same colour at 35% (`box-shadow: 0 0 0 2px color-mix(...)`) | 8px | `--k-lamp-needs-input` |
| `working` *(from Faceplate)* | hollow ring, 1.5px stroke | 8px | `--k-lamp-working` |
| `error` | filled square rotated 45° (diamond) | 7px | `--k-lamp-error` |
| `done` | filled dot, no halo | 6px | `--k-lamp-done` |
| `activity` | small dot | 4px | `--k-lamp-activity` |
| `none` | renders nothing (the slot keeps its width in tabs) | | |

- `working` is not an Attention level: it is derived from `sessions[].status === 'working'` in the store (a getter, no IPC change). Precedence on an aggregate (project, status strip): needs_input > error > working > done > activity.
- `aria-label` stays as today; lamps are never the only accessible name.
- Lamps in the rail use a 10px socket so the diamond stays sharp on 1x screens.

### 6.2 Rail (`shell/ProjectRail.svelte`)

- 44px wide, `--k-bezel`. Inbox at top, home / settings / add pinned at the bottom. Inbox, home, settings and add are flat ghost icons; only the current destination (a project, home or the inbox) wears the `--k-well` tile face and the full-height index bar.
- Project tile: 28px square, radius 4, `--k-well` face, initial(s) in `--k-fg-chrome`, a 3px project-colour index bar on the tile's left edge (raw project colour, not the clamped accent). No fill with the project colour, no text-shadow.
- Active: index bar runs full tile height, initial goes to `--k-fg` 600.
- Lamp socket at the top-right corner, 10px.
- **Lit tile** *(from Faceplate)*: when any session in the project needs input, the tile face takes `color-mix(in oklab, var(--k-lamp-needs-input) var(--k-lit-mix), var(--k-well))` (`--k-lit-mix` 14% light, 30% dark, where 14% vanished into the graphite) and the socket shows the lamp plus a tabular count (10px/600). That makes Claude waiting the brightest thing in the window, readable in peripheral vision.
- Focus: 2px `--k-focus` ring with a 2px `--k-bezel` gap (`box-shadow: 0 0 0 2px var(--k-bezel), 0 0 0 4px var(--k-focus)`).
- Drag-over: 2px accent insertion line.

### 6.3 Tab bar (`shell/TabBar.svelte`)

- 30px, `--k-bezel-raised`. Text tabs, no background box or pill.
- Layout per tab *(from Faceplate)*: `[lamp slot 10px][title][×]`. The lamp slot is fixed width even when empty, so the states of all tabs line up in one scannable column.
- Session tab titles (session / branch) use mono 12; view tabs (Tickets, Reviews, Inbox) use the UI face 12.
- Inactive: `--k-fg-chrome`. Active: `--k-fg` 600 and a 2px accent underline.
- A tab whose session needs input gets a 2px `--k-lamp-needs-input` top edge, visible even when the tab is scrolled half out of view *(from Faceplate)*.
- Close button shows on hover, on `:focus-visible`, and on the active tab, so it is keyboard reachable.
- Window chrome: macOS custom chrome reserves a 72px leading inset for the traffic lights and the empty bar area is `data-tauri-drag-region`. GNOME custom chrome adds a 30px controls cluster on the right. Hyprland/Sway: no title bar, the tab bar is the top edge.

### 6.4 Panes (`shell/PaneHost.svelte`)

- No border, no radius. Panes are separated by `--k-gap` (1px) showing `--k-bezel`. If the gap is too faint on a real panel, raise `--k-gap` to 2px; never add borders back.
- Pane header 22px, background `--k-well`, so header and terminal read as one screen. Unfocused title `--k-fg-subtle`; focused title `--k-fg` 600 plus `box-shadow: inset 0 2px 0 var(--k-accent)` on the header.
- Terminal content is never dimmed. Inside a terminal pane the focus indicator is the header tick, never an outline around xterm.
- Splitter: visually the 1px gap, 5px hit area, `--k-border-strong` on hover / drag.
- "Fix hooks" and similar header actions are accent text buttons.

### 6.5 Status strip (`shell/StatusBar.svelte`)

- 22px, `--k-bezel-raised`, 11-12px, tabular-nums, segments spaced 12px, no dots.
- Left: project (3x12px colour bar + name), branch (git-branch icon + mono), focused session (lamp + name + state word).
- Right: exceptions only. **Lamp meter** *(from Faceplate)*: one ghost button per non-zero state, lamp + count (e.g. `● 2`); clicking focuses the next session in that state. Also prefix-armed, restart-needed. Perf HUD toggle is a 16px icon at the far right. Zero counts render nothing.

### 6.6 Command palette

- Top-anchored at 14vh, `min(600px, 100vw - 32px)`, `--k-bg-float`, radius 8, `--k-shadow`.
- No title bar or close button: the 36px input is the top edge, Esc or the backdrop closes it. Query in mono. Results 26px rows: 10px lamp slot (session and tab hits), 14px kind icon (no text label), title, meta, right-aligned kbd hint. Results are grouped (one header per group, groups in order of their best hit).
- Selected row: `--k-bg-selected` plus a 2px accent left bar.

### 6.7 Sheets, dialogs, menus, toasts

- All float: `--k-bg-float`, `--k-shadow`, radius 8 (one radius for every floating surface).
- Dialogs are top-anchored like the palette (not vertically centred), title 15/600, actions right-aligned, primary last.
- Sheets slide from the right edge (one 90ms transform on open/close, removed under reduced motion), width `min(480px, 100vw)`.
- Menus: 26px rows, kbd hints right in `--k-fg-subtle` mono.
- Toasts: bottom-right, one line + optional action, a lamp shape for kind (error diamond, done dot). No auto-dismiss timer is added beyond what the toast store already does; verify against check-no-timers.sh.

### 6.8 Lists (tickets, reviews, inbox)

One row grammar for all three, so they read as one instrument:
`lamp | id (mono 12, --k-fg-muted, 76px) | title (13, fills) | meta (11, right-aligned, tabular)`.

- Rows `ROW_HEIGHT` (32px, `$lib/ui`), no row borders, no zebra.
- Selected: `--k-bg-selected` plus `box-shadow: inset 2px 0 0 var(--k-accent)`. Hover: `--k-bg-hover`.
- Tickets meta, right to left of the title: PR chip, sprint chip (when not grouped by sprint), status chip (a button), age badge, source chip, assignee initials (20px circle on `--k-bezel`, hidden on Mine), updated (11, tabular).
- Age badge: `7d` tabular text after 7 days in a non-done status, `--k-fg-subtle`; `--k-warn` text from 14, `--k-danger` text from 21. No fill, no border (a lamp must stay the only coloured shape). Its slot is reserved so titles and chips line up.
- Sprint chip: flat chip (§6.11), name in `--k-fg-subtle`, `--k-fg-muted` for the active sprint.
- Compact row actions (Move, pull request, Start) appear in the row on the selected row, on hover and on focus-within; they are icon buttons with a tooltip carrying the key, and take no space when hidden.
- Multi-select: marked rows get `--k-bg-selected` and an accent lamp; `Esc` clears.
- Reviews meta: `+214 −38` in mono tabular (`--k-ok` / `--k-danger`), checks lamp.
- Inbox: grouped by lamp (needs input first) under 12/600 `--k-fg-muted` sentence-case headers that carry the lamp shape.
- Toolbar 30px `--k-bezel-raised`: filter input left (most-used control), then Who tabs (Mine, Unassigned, Anyone, with tabular counts, omitted when unknown), then the source menu, the Group select and the list/board toggle on the right.
- Tickets are grouped (Flow by default: Doing, Waiting, Ready, Backlog, Done in the last 7 days; Status orders in progress, in review, to do, unknown, done; Done collapsed) under 12/600 `--k-fg-muted` sentence-case headers with a tabular count, 26px. The status chip is flat text with a 2px left bar in the category colour (`--k-info`, `--k-warn`, `--k-ok`, `--k-border-strong`) and always shows the tracker's own status name. The PR chip is mono `#N` / `!N` with one lamp for CI and review (changes requested `needs_input`, CI failed `error`, running `working`, approved or passed without required review `done`, awaiting review `activity`); its label names both. The source chip appears only with more than one source and a grouping other than Source.
- Doing header: turns `--k-warn` with a short note ("Above your limit of 3") when the count passes `tickets.wip_limit`; nothing else changes.
- Split view: below ~720px pane width there is no detail column. Wider, the detail sits on the right (`--k-well`, §6.9) and the side with the keyboard shows a 2px `--k-focus` inset bar on its left edge.
- StatusPicker (`m`, status chips, detail, work bar, palette): the header is the ticket's workflow on one line of native status names in category order, the current one `--k-fg` 600 with its category bar, the rest `--k-fg-subtle`. Below, the legal transitions numbered `1`-`9` with the digit as a mono kbd hint on the right; the digit moves at once. The same menu filters as you type, shows the tracker's message and Open in browser on an error, and with several tickets lists only the statuses they all share.
- Source picker sheet (§6.7, `min(480px, 100vw)`, from the right): account Select, search input, rows of kind (12 muted), label (13), detail (11 muted); added hits read "Added".
- No key shadows or per-row effects: rows live inside VirtualList and must stay cheap to paint.

### 6.9 Detail panes (ticket, review) *(meta grid and measure from Spine)*

- Key in mono 13 (copyable), title 18/600, then a label/value meta grid: `display: grid; grid-template-columns: auto 1fr auto 1fr`, labels 12 `--k-fg-muted`, values 13. Stacks to `auto 1fr` under 480px pane width.
- Section heads (Description, Discussion, Checks) 13/600.
- `HtmlContent`: 13/1.5, `max-width: var(--k-measure)`, links `--k-accent` underlined, code/pre mono on `--k-bezel-raised` radius 2, blockquote 2px `--k-border-strong` left rule.
- Reviews: `feat/x into main` in mono 12 under the title, diffstat tabular.
- Tickets: a `Pull request` row (PR chip, CI lamp, review state in words); the status value is a button opening the StatusPicker of §6.8. Order: key, title, status, action bar, meta grid (assignee, priority, sprint, estimate, due, labels, updated; assignee `u` and priority `!` are buttons opening a filtered picker), Pull request rows, description, comments with a compose box.
- Pull request rows: one line each, `#N` chip, title filling, then CI lamp and review state in words; wraps under the title when narrow.
- Action bar: ghost `sm` Buttons with the key as chord hint: Start / Resume work (primary), Move `m`, Open PR `p`, Assign to me `a` (Unassign `A` when assigned), Comment `c`, Refine with Claude `r`, Copy branch `y`, Open in browser `o`. A refine shows its proposal under the sub-tasks as mono pre-wrapped Markdown on `--k-bezel-raised`, with Post as comment (primary), Copy and Discard. An action that cannot run keeps its place at 55% opacity with `aria-disabled` and the reason as tooltip; it stays reachable with Tab.
- Embedded in the split view the detail drops the pane padding it would double, and shows the 2px `--k-focus` bar when it holds the keys.

### 6.10 Board

- Lanes 264px, `--k-bezel` trays (radius 2) on a `--k-well` board with 8px gutters, so each lane and an empty drop target has an edge. Header = name (12/600) + count (11, tabular, `--k-fg-subtle`).
- Cards `--k-well`, radius 2, 1px `--k-border`, no shadow. Line 1 (fixed 18px, chips centred in it, so titles line up across lanes): key (mono 11) + lamp of the linked work item's sessions, if any *(from Faceplate)*. Lines 2-3: title 13, clamped. Bottom: avatar + age.
- Drag target: 2px accent insertion line. Focused card: focus ring.

### 6.11 Chips and badges

- Flat: radius 2, `--k-bezel-raised` fill, `--k-fg-muted` text 11-12, no coloured fill (a filled colour pill would compete with lamps). Status category may add a 2px left bar in ok / warn / info.
- Counts in chips are tabular.

### 6.12 Forms (Button, TextInput, Select, Toggle, Tabs, Kbd)

- Button: height 26, radius 4. Default: `--k-bezel-raised` + 1px `--k-border`. Primary: `--k-accent` fill + `--k-accent-fg`. Danger: `--k-danger` fill. Ghost: text only until hover. Labels name the action ("Start work", "Request changes").
- TextInput / Select: `--k-well` face, 1px `--k-border`, radius 4; they read as small screens on raised toolbars. Focus = focus ring, border unchanged.
- Toggle: track `--k-bezel`, thumb `--k-well`; on = accent track. The thumb carries a 1px notch on the on side so state is not colour-only *(from Faceplate)*.
- Tabs (in-view segmented): text only, active = `--k-fg` 600 + 2px accent underline, same grammar as the tab bar.
- Kbd: mono 11, 1px `--k-border-strong` bottom border only, no box.
- Settings (schema-generated): all UI face, 26px rows, labels left, controls right-aligned in a column, help text 12 `--k-fg-muted` under the label.
- Project colour picker: the ten `--k-swatch-*` chips plus a custom hex field *(from Spine)*; value stored in `ProjectInfo.color` as today.

### 6.13 Empty and error states

- One sentence stating the condition plus one action button, left-aligned at the top-left of the pane, 16px padding. No illustrations, no centring.
- Examples: `No tickets match "auth".` [Clear filter]. `Jira rejected the token (401).` [Open tracker settings]. `No tracker bound to this project.` [Bind a tracker].
- Errors say what happened and how to fix it; they do not apologise.

## 7. Focus and keyboard

- `:focus-visible`: 2px `--k-focus` outline, offset 1px (inherits the project hue). On bezel surfaces where an outline would merge with an edge, use the two-ring box-shadow from §6.2.
- Terminals: focus shown by the pane header tick only.
- Every hover-only affordance (tab close, row actions) also appears on `:focus-visible` / `:focus-within`.
- Selection (`::selection`) uses `--k-bg-selected` outside terminals.
- Scrollbars 8px, `--k-border-strong` thumb, transparent track.

## 8. Motion

- Only motion that answers an action: menus, sheets, dialogs, palette open/close, hover/press background changes. Duration `--k-duration` (90ms), `ease-out`.
- No looping or infinite animations, no idle timers, no pulsing lamps, no entrance sequences. `scripts/check-no-timers.sh` must stay green; reviewers reject any `infinite` keyframes.
- `prefers-reduced-motion: reduce` sets `--k-duration: 0ms` and removes the sheet transform.

## 9. Decisions

- **Bezel over Faceplate and Spine.** It is the only direction that makes terminals the brightest, most coloured surface and pushes everything else back, which is what people staring at Claude and nvim for hours need. It also needs no fonts, no new assets and almost no component rewrites.
- **Project hue as the only accent.** With many projects open, "which project am I in" is the most frequent question; answering it through the focus ring, tab underline, pane tick and selected-row bar costs no space and no label. A fixed oklch L/C keeps any user-picked hue legible. Spine's separate 3px spine was dropped: same information, one more decoration.
- **Static fallbacks for oklch and color-mix.** Older WKWebView (macOS 14) and WebKitGTK (Ubuntu 22.04) get a steel accent and a hex selected tint through `@supports`, so nothing breaks; only the per-project hue degrades.
- **Low-chroma projects fall back to the static accent.** Grey has no hue to borrow; a forced chroma would invent one.
- **Five lamps with five silhouettes** (Bezel's lamps + Faceplate's `working` ring). Colour-blind users and anyone reading the rail out of the corner of their eye get the state by shape. `working` closes the gap between "Claude is busy" and "Claude needs me".
- **Lit rail tile for needs-input** (Faceplate). The single most important signal gets the single strongest treatment; everything else stays a small lamp.
- **Lamp-first fixed slot in tabs** (Faceplate). Eight tabs' states scan as one column.
- **No serif reading voice** (Spine rejected). A bundled serif breaks the "no fonts" rule's spirit and system serifs on Linux are uneven at small sizes; Spine's 68-72ch measure and label/value meta grid were kept because they help reading without a font.
- **No key shadows or press travel** (Faceplate rejected). Paint cost on WebKitGTK and a skeuomorphic look that competes with terminal content. Kept only the toggle notch and kbd styling.
- **Ink accent rejected** (Faceplate). Removing hue from focus and selection loses the project-at-a-glance signal and makes links ambiguous.
- **Neutral terminal cursor.** Project-hued or amber cursors would force a palette re-push on every project switch and clash with nvim themes.
- **No pane borders.** A 1px bezel gap separates panes with less ink and gives terminals every pixel.
- **Mono only for pasteable strings, no middle dots, no uppercase.** Each mark encodes data; template chrome is gone.
- **Top-anchored dialogs and palette.** The eye stays near the tab bar where the keyboard user already looks.

## 10. Rollout and checks

1. Replace `tokens.css`; update `theme.ts` arrays; add the token/theme parity unit test.
2. Edit AttentionDot, ProjectRail, TabBar, PaneHost, StatusBar, Button, Toggle, Kbd, HtmlContent, list rows, Board; set `--k-project` in Shell.
3. Run `VITE_IPC=mock pnpm --filter @kelta/ui dev` and screenshot both themes with Playwright; `pnpm --filter @kelta/ui run e2e`; `bash scripts/ci-local.sh` (bundle budgets, no-timers).
4. Check the 1px gap and lamp shapes at 100% zoom on a 1x Linux display.
