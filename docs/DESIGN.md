# Kelta design spec: Bezel

Status: built. The direction of the UI redesign (lane `ui-redesign`), since scaled up for readability (larger type, rounded panes, a labelled sidebar). It replaces the generic white / `#17181b` + `#2f6fdf` palette. Where this page and `ui/src/styles/tokens.css` differ, the file wins.
Sources: three proposals (Bezel, Margin and Spine, Faceplate). Bezel is the base. Grafts are marked _(from Faceplate)_ or _(from Spine)_.

## 1. Concept

Kelta is the **bezel around a set of screens**. The terminals are the screens: flush, set-in wells that carry all the contrast and identity (the user's font, their ANSI output, nvim's colours). Everything else (rail, tabs, pane headers, status strip, list chrome) is a matte, low-contrast housing that stays dark until a **lamp** needs you.

Three rules:

1. **Chrome has no brand colour.** Its only accent is the active project's own colour, remapped in oklch to a fixed lightness and chroma. The window takes the hue of the project you are in. List rows are content, not chrome: their keys wear one fixed hue (`--k-info`, #382), which never stands for a state.
2. **Saturated colour in the chrome means state.** Lamps (needs input, working, error, done, activity) are the only saturated marks, and each has its own silhouette, so colour is never the only signal.
3. **Density without noise.** a bezel gap and a thin ring instead of boxed panes, shadows only on floating surfaces, tabular figures for every count, monospace only for strings you could paste into a terminal.

### 1.1 The map

Three places, one sidebar, five keys. Every screen is one of them; FLOW §1 says why.

- **Queue** (`views/inbox/InboxPane.svelte`, §6.8): all work, across projects, ranked by whose move it is: **Your move**, **Claude working**, **Waiting on others**. The selected row's brief sits beside the list; Claude can be answered from it.
- **Desk** (a work tab): Claude and nvim in panes (§6.4), under a header naming the item with its stage track and next step (`views/work/WorkContext.svelte`, §6.3), beside the Story panel, the item's story in stage order (§6.4).
- **Library**: **Tickets** and **Pull requests** (§6.8), the lists to pick from, under one project scope set in the top bar (§6.3).
- **Sidebar of work** (`shell/ProjectRail.svelte`, §6.2): the three places, then each project's unfinished work items in the order they started and its Claude sessions without one; its shells and list or tool tabs fold into one row. It folds to its lamps while the Queue is shown, which lists the same work.
- **Five keys** before any other: `↵` the next step, `⌘.` the actions of whatever is selected, `⌘K` find anything, `⌘J` the next thing that needs you, `⌘0` the Queue and back. The prefix (`⌃⇧Space`) reaches every action and, after a pause, shows the which-key panel (§6.6).
- **Attention budget**: one call signal. The top bar's "needs you" chip counts only Claudes blocked on you, with `⌘J`; lit sidebar rows and lamps say the rest without asking. By default a finished Claude does not notify, a blocked one does (FLOW §2.3), and an info toast closes when you leave the view that raised it.
- **Focus mode** (`view.focus`, `⇧⌘F`): the sidebar and the Story panel hide and the notifications about other work wait; a blocked Claude still calls. The top bar keeps only `‹ ›`, the search field and the "needs you" chip in its quiet form (the lamp as an outline); the scope, `+ New` and the breadcrumb hide (§6.3). The work header adds a ghost **Leave focus** `Esc` `Esc` beside Actions. A pill at the bottom right counts what waits ("2 updates held until you leave focus"); leaving (`Esc` `Esc` or the button) sums it up in one toast and the pill goes.

## 2. Tokens

Every earlier `--k-*` name is kept, so components that only read tokens need no edit. Names the redesign added: `--k-fg-strong` (#382), `--k-bezel`, `--k-bezel-raised`, `--k-well`, `--k-bg-float`, `--k-fg-chrome`, `--k-project`, `--k-gap`, `--k-gutter-hit`, `--k-pane-header-height`, `--k-lamp-*`, `--k-lit` and `--k-lit-mix` (the call light), `--k-term-selection`, `--k-measure`, `--k-radius-screen`, and for the sidebar `--k-rail-width`, `--k-rail-width-compact`, `--k-sidebar-row-height`, `--k-rail-item`. (`--k-swatch-*` was dropped by #253: project colours come from the theme's ANSI slots, §2.1.)

`src/styles/tokens.css` is:

```css
/* Bezel tokens. Chrome = housing, terminals = screens. No web fonts.
   --k-project is set on <html> by the shell from the active project's colour.
   Accent = project hue at fixed oklch L/C so focus marks always meet 3:1. */
:root {
  --k-font-ui:
    system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Cantarell, Ubuntu, 'Noto Sans', sans-serif;
  --k-font-mono:
    ui-monospace, 'JetBrains Mono', 'SF Mono', Menlo, 'Cascadia Mono', 'DejaVu Sans Mono', monospace;
  --k-font-size-xs: 12px; /* floor: ages, counts, kbd, badges, status bar, sidebar sub-lines */
  --k-font-size-sm: 13px; /* chrome: tabs, pane headers, toolbars, small buttons, group headers */
  --k-font-size: 14px; /* content: rows, forms, menus, sidebar names, empty-state body */
  --k-font-size-lg: 16px; /* view, dialog and empty-state titles; palette input */
  --k-font-size-xl: 20px; /* ticket / review detail title */
  --k-font-size-display: 26px; /* welcome heading only */
  --k-line-height: 1.43;
  --k-line-height-read: 1.55;
  --k-measure: 68ch;
  --k-weight-medium: 500;
  --k-weight-strong: 600;

  --k-space-1: 2px;
  --k-space-2: 4px;
  --k-space-3: 8px;
  --k-space-4: 12px;
  --k-space-5: 16px;
  --k-space-6: 24px;
  --k-space-7: 32px;
  --k-space-8: 48px;

  --k-radius-sm: 3px; /* marks: chips, badges, kbd */
  --k-radius: 6px; /* controls: buttons, inputs, rows, tabs, tiles */
  --k-radius-screen: 8px; /* panes: screens set into the housing */
  --k-radius-lg: 10px; /* floating: menus, palette, dialogs, sheets, toasts */

  --k-rail-width: 288px; /* labelled sidebar, default (resizable: shell/sidebar.svelte.ts) */
  --k-rail-width-compact: 64px; /* collapsed sidebar, and below 1400px window width */
  --k-sidebar-row-height: 44px; /* two lines: name + status word (global views) */
  --k-rail-item: 32px; /* rail session and tool rows: one line */
  --k-contextbar-height: 30px; /* the bar above the panes; the window's title bar in custom chrome */
  --k-toolbar-height: 40px; /* .k-toolbar: a view's filters */
  --k-statusbar-height: 28px;
  --k-pane-header-height: 32px;
  --k-control-height: 32px;
  --k-control-height-sm: 28px; /* smallest target anywhere */
  --k-row-height: 32px; /* list rows, menu items, picker rows; mirrored by ROW_HEIGHT in lib/ui */
  --k-gap: 6px; /* housing visible between screens */
  --k-gutter-hit: 10px;

  --k-z-pane: 1;
  --k-z-menu: 30;
  --k-z-sheet: 40;
  --k-z-dialog: 50;
  --k-z-toast: 60;

  --k-duration: 90ms; /* response to an action only; nothing loops */

  --k-project: #4f8fb3; /* overwritten per active project */

  /* light: putty housing, paper-white screens */
  --k-bezel: #e1e5e9;
  --k-bezel-raised: #eaedf0;
  --k-well: #fafbfb; /* must equal lib/terminal/theme.ts LIGHT bg (terminal.spec asserts #fafbfb) */
  --k-bg: var(--k-well);
  --k-bg-elev: var(--k-bezel-raised);
  --k-bg-sunken: var(--k-bezel);
  --k-bg-float: #ffffff;
  --k-bg-hover: rgba(22, 27, 33, 0.06);
  --k-bg-active: rgba(22, 27, 33, 0.1);
  --k-bg-selected: #dde9f1; /* hex fallback, overridden below */
  --k-fg: #161b21; /* 13.7 on bezel */
  --k-fg-strong: var(--k-fg); /* list and detail titles: a step past --k-fg, mixed below */
  --k-fg-muted: #46505b; /* 6.5 on bezel, 5.3 on lit */
  --k-fg-subtle: #535d68; /* 5.3 on bezel; never on lit */
  --k-fg-chrome: #434d57; /* 6.8 on bezel: chrome now carries labels */
  --k-border: #cbd1d7;
  --k-border-strong: #77818c; /* 3.1 on bezel: inputs, gutter grip */
  --k-accent: #28698c; /* static fallback accent, 4.75 on bezel */
  --k-accent-fg: #ffffff;
  --k-danger: #b0302a;
  --k-danger-fg: #ffffff;
  --k-warn: #7f5500;
  --k-ok: #287043;
  --k-info: #2a5c9f;
  --k-overlay: rgba(20, 24, 29, 0.32);
  --k-shadow: 0 12px 32px rgba(20, 24, 29, 0.16), 0 0 0 1px rgba(20, 24, 29, 0.08);

  /* lamps: colour + shape */
  --k-lamp-needs-input: #c4352e;
  --k-lamp-working: #1a7379;
  --k-lamp-error: #946000; /* 3:1 on lit as a non-text mark */
  --k-lamp-done: #2a7a49;
  --k-lamp-activity: #626b76;
  --k-lit-mix: 14%; /* call light: needs-input share of the surface */
  --k-lit: #ddcccf; /* call light fallback; color-mix version below */

  /* terminal (mirror of lib/terminal/theme.ts LIGHT, which is the source of truth) */
  --k-term-bg: #fafbfb;
  --k-term-fg: #1b2026;
  --k-term-cursor: #1b2026;
  --k-term-selection: #c9daea;

  color-scheme: light;
}

@supports (color: oklch(from red l c h)) {
  :root {
    --k-accent: oklch(from var(--k-project) 0.5 0.11 h);
  }
}
@supports (color: color-mix(in oklab, red, blue)) {
  :root {
    --k-bg-selected: color-mix(in oklab, var(--k-accent) 14%, var(--k-well));
    /* one declaration serves both themes: the vars resolve per theme on :root */
    --k-lit: color-mix(in oklab, var(--k-lamp-needs-input) var(--k-lit-mix), var(--k-bezel));
    /* mixed from the theme's own --k-fg, so every theme gets its brighter step */
    --k-fg-strong: color-mix(in oklab, var(--k-fg) 60%, #000);
  }
}
:root {
  --k-focus: var(--k-accent);
}

/* dark: graphite housing, slate screens. Same block twice (media + explicit). */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) {
    --k-bezel: #1a1e24;
    --k-bezel-raised: #22272e;
    --k-well: #121519; /* must equal theme.ts DARK bg (terminal.spec asserts #121519) */
    --k-bg-float: #282e36;
    --k-bg-hover: rgba(225, 230, 236, 0.07);
    --k-bg-active: rgba(225, 230, 236, 0.11);
    --k-bg-selected: #1d2b35;
    --k-fg: #e1e6ec; /* 13.3 on bezel, 8.4 on lit */
    --k-fg-muted: #aab4bf; /* 8.0 on bezel, 5.0 on lit */
    --k-fg-subtle: #97a1ad; /* 5.2 on float; never on lit */
    --k-fg-chrome: #a6b0bb;
    --k-border: #2d343d;
    --k-border-strong: #6a7582;
    --k-accent: #78b0cf;
    --k-accent-fg: #0f161b;
    --k-danger: #ec7a72;
    --k-danger-fg: #1a0d0c;
    --k-warn: #e0ab4e;
    --k-ok: #79c793;
    --k-info: #8fb2df;
    --k-overlay: rgba(6, 8, 10, 0.58);
    --k-shadow: 0 12px 32px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(225, 230, 236, 0.07);
    --k-lamp-needs-input: #ef6a62;
    --k-lamp-working: #62b9be;
    --k-lamp-error: #e0ab4e;
    --k-lamp-done: #62bb84;
    --k-lamp-activity: #8b95a1;
    --k-lit-mix: 30%;
    --k-term-bg: #121519;
    --k-term-fg: #d3d9df;
    --k-term-cursor: #d3d9df;
    --k-term-selection: #2c3f52;
    color-scheme: dark;
  }
  @supports (color: oklch(from red l c h)) {
    :root:not([data-theme='light']) {
      --k-accent: oklch(from var(--k-project) 0.74 0.09 h);
    }
  }
  @supports (color: color-mix(in oklab, red, blue)) {
    :root:not([data-theme='light']) {
      --k-bg-selected: color-mix(in oklab, var(--k-accent) 16%, var(--k-well));
      --k-fg-strong: color-mix(in oklab, var(--k-fg) 60%, #fff);
    }
  }
}

:root[data-theme='dark'] {
  --k-bezel: #1a1e24;
  --k-bezel-raised: #22272e;
  --k-well: #121519; /* must equal theme.ts DARK bg (terminal.spec asserts #121519) */
  --k-bg-float: #282e36;
  --k-bg-hover: rgba(225, 230, 236, 0.07);
  --k-bg-active: rgba(225, 230, 236, 0.11);
  --k-bg-selected: #1d2b35;
  --k-fg: #e1e6ec; /* 13.3 on bezel, 8.4 on lit */
  --k-fg-muted: #aab4bf; /* 8.0 on bezel, 5.0 on lit */
  --k-fg-subtle: #97a1ad; /* 5.2 on float; never on lit */
  --k-fg-chrome: #a6b0bb;
  --k-border: #2d343d;
  --k-border-strong: #6a7582;
  --k-accent: #78b0cf;
  --k-accent-fg: #0f161b;
  --k-danger: #ec7a72;
  --k-danger-fg: #1a0d0c;
  --k-warn: #e0ab4e;
  --k-ok: #79c793;
  --k-info: #8fb2df;
  --k-overlay: rgba(6, 8, 10, 0.58);
  --k-shadow: 0 12px 32px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(225, 230, 236, 0.07);
  --k-lamp-needs-input: #ef6a62;
  --k-lamp-working: #62b9be;
  --k-lamp-error: #e0ab4e;
  --k-lamp-done: #62bb84;
  --k-lamp-activity: #8b95a1;
  --k-lit-mix: 30%;
  --k-term-bg: #121519;
  --k-term-fg: #d3d9df;
  --k-term-cursor: #d3d9df;
  --k-term-selection: #2c3f52;
  color-scheme: dark;
}
@supports (color: oklch(from red l c h)) {
  :root[data-theme='dark'] {
    --k-accent: oklch(from var(--k-project) 0.74 0.09 h);
  }
}
@supports (color: color-mix(in oklab, red, blue)) {
  :root[data-theme='dark'] {
    --k-bg-selected: color-mix(in oklab, var(--k-accent) 16%, var(--k-well));
    --k-fg-strong: color-mix(in oklab, var(--k-fg) 60%, #fff);
  }
}
```

### 2.1 Project hue

- A project colour (`ProjectInfo.color`) is a **theme slot** or a **custom hex**. `theme:<slot>` names one of the 12 ANSI hues every theme has (`red`, `green`, `yellow`, `blue`, `magenta`, `cyan` = ANSI 1-6, `bright-red` .. `bright-cyan` = ANSI 9-14) and becomes the active theme's colour, re-resolved live on every theme or appearance change. A `#rrggbb` hex is a custom colour and never changes with the theme. `resolveProjectColor` (`lib/theme/projectColor.ts`) turns either into the hex to paint, with the active theme from `ui.theme`.
- The ten pre-#253 Bezel swatch hexes were theme picks, so they read as the slot closest in oklch hue (harbor → blue, moss → green, ochre → yellow, brick → red, plum → magenta, teal → cyan, rose → bright-magenta, cobalt → bright-blue, olive → bright-yellow). Slate has no slot and stays a hex. This is a read-time table; no config file is rewritten.
- The picker offers the active theme's slot colours minus the ones the theme paints alike (#364): a slot whose colour is within 0.05 in oklab of an earlier slot is hidden (Nord and Catppuccin give a bright slot its base hex), unless it is the selected one. A stored `theme:<slot>` that is hidden stays selected and visible.
- Everything paints the resolved colour: the raw bars (sidebar, tiles, Queue rows, start sheet) and `--k-project` below.
- `Shell.svelte` sets `document.documentElement.style.setProperty('--k-project', color)` (the resolved colour) in an `$effect` keyed on the active project. No timer.
- If the project has no colour, a colour the browser cannot parse, an oklch chroma below 0.03 (greys, black, white), or a hue within 30° of the needs-input lamp (brick, rose, any red), the shell removes the property so the static fallback accent is used. One JS check in the shell (`shell/accent.ts`): an achromatic colour has no hue to borrow, an invalid one would void every focus ring, and a red accent would make every selected row read as an alarm.
- Switching project never re-pushes the terminal palette: the cursor and selection are neutral.

### 2.2 Contrast floor

| Pair                                                            | Light                                | Dark           |
| --------------------------------------------------------------- | ------------------------------------ | -------------- |
| `--k-fg` on `--k-well`                                          | 16.7:1                               | 14.6:1         |
| `--k-fg-muted` on `--k-well` / `--k-bg-float`                   | 7.9 / 8.2:1                          | 8.7 / 6.5:1    |
| `--k-fg-subtle` on well / bezel-raised / bezel / float (lowest) | 5.29:1 (bezel)                       | 5.23:1 (float) |
| `--k-fg-chrome` on `--k-bezel-raised` / `--k-bg-float`          | 7.3 / 8.6:1                          | 6.8 / 6.2:1    |
| accent (any hue, fixed L) on well                               | at least 3:1 (focus marks, non-text) | at least 3:1   |

Every text token clears 4.5:1 on every surface it can sit on, so 12-13px labels and state words may use it. Exception: `--k-fg-subtle` is never used on the call light (`--k-lit`); the lit pane header switches it to `--k-fg-muted`.
Lamp colours are non-text marks (3:1); as text, such as the phase word on a ticket row, they only appear through `color-mix(in oklab, var(--k-lamp-*) 75%, var(--k-fg))`.
Measured (WCAG 2 relative luminance). Recheck with a script after any token edit.

Every theme meets the floor too, custom and imported ones included (`lib/theme/floor.ts`, #387). When a theme paints `--k-bezel` and `--k-bezel-raised` within 0.015 in oklab of `--k-well`, they (and a collapsed `--k-bg-float`) are re-derived from well toward fg like a theme that leaves them out. `--k-fg-muted`, `--k-fg-subtle` and `--k-fg-chrome` move toward `--k-fg` until 4.5:1 on the four surfaces, and `--k-border-strong` until 3:1 on well and bezel and stronger than `--k-border`. Settings → Theme lists each fix on the theme's card, or that some pair still fails (when `--k-fg` itself does). The built-ins need no fix.

## 3. Typography

No web fonts and no bundled woff2. Terminals already render in the user's chosen mono font, so the identity lives there.

- **UI face**: `system-ui` stack (SF Pro, Cantarell or Adwaita Sans, the distro sans on Hyprland/Sway, Segoe). All chrome and all list/detail content.
- **Mono**: `ui-monospace` first. Only for strings you could paste into a terminal: ticket keys (`PROJ-142`, `#381`, `!57`), branch names, SHAs, session ids, paths, diffstat numbers, kbd hints, the palette's typed query. Never for labels.

| Step    | Size / line | Weight        | Use                                                                                                    |
| ------- | ----------- | ------------- | ------------------------------------------------------------------------------------------------------ |
| xs      | 12 / 1.43   | 400           | the floor: ages, priorities, counts, kbd, badges, chips, status strip, sidebar sub-lines               |
| sm      | 13 / 1.43   | 400, 500, 600 | tabs, pane headers, toolbars, small buttons, group headers (600, `--k-fg-muted`)                       |
| base    | 14 / 1.43   | 400, 500      | list rows (titles 500 `--k-fg-strong`, 600 on the selected row), forms, settings, menus, sidebar names |
| lg      | 16 / 1.43   | 600           | view titles, dialog and sheet titles, palette input                                                    |
| xl      | 20 / 1.43   | 600           | ticket / review detail title only, `--k-fg-strong`                                                     |
| display | 26          | 600           | welcome heading only                                                                                   |
| read    | 14 / 1.55   | 400           | HtmlContent bodies, max-width `--k-measure` (68ch); headings inside stay 1em/600                       |

Rules:

- Weights 400, 500 and 600. 600 is for the focused pane title, view titles, group headers and the selected list row's title; 500 (`--k-weight-medium`) for unfocused pane titles, palette group headers and list row titles (#382).
- Titles in lists and detail panes take `--k-fg-strong`, a step past `--k-fg` (mixed 40% toward black in light, white in dark, from the theme's own `--k-fg`), so a row's title stands out from its other text.
- Nothing is set below 12px, bar the initials in a 20px avatar (10px). A label that does not fit is shortened or dropped, never shrunk (§6.14).
- `font-variant-numeric: tabular-nums` is on `body`, so every count, age and diffstat lines up; `.k-num` stays for explicit cases.
- Sentence case everywhere. No all-caps, no tracked labels, no eyebrows. Board lanes keep the tracker's own casing.
- No middle-dot joins (`A · B`) in new chrome. Segments are separated by space (12px) and, where meaning needs it, an icon prefix. No trailing arrows on buttons or links.
- Chrome text (`--k-fg-chrome`) reads one step quieter than content (`--k-fg`). The terminal is the large type.

## 4. Space, radius, elevation

- **Spacing**: 2 / 4 / 8 / 12 / 16 / 24 / 32 / 48. Row inner padding is 8px horizontal. Detail panes pad 16px; dialogs 16px; nothing pads a terminal.
- **Radius encodes layer**: marks (chips, badges, kbd) 3px, controls (buttons, inputs, rows, tabs, tiles) 6px, panes 8px (`--k-radius-screen`, screens set into the housing), floating surfaces 10px.
- **Elevation has three levels and only the top one casts a shadow**:
  1. Bezel (`--k-bezel`): rail, the housing around and between panes, board lanes, the split view's detail column (so list and detail read as two surfaces). In dark themes the rail (`--k-rail`) sits one step below the well (`--k-well` mixed 30% toward black), so the content reads as the lit part of the window.
  2. Bezel-raised (`--k-bezel-raised`): top bar, desk header, pane headers, status strip, list toolbars, list group headers (Queue, Tickets, Pull requests: a full-width band with a 1px `--k-border` rule under it, pinned at the top of the list while its group is in view).
  3. Well (`--k-well`): terminals, lists, detail panes, inputs.
  4. Float (`--k-bg-float` + `--k-shadow` + `--k-radius-lg`): menus, palette, dialogs, sheets, toasts. The only shadows in the app.
- Borders only where they separate data (board cards, inputs, table rows in settings when needed). A pane has a 1px `--k-border` ring instead (§6.4).

## 5. Terminal palette

`src/lib/terminal/theme.ts` holds the Bezel base palettes and is their source of truth (Rust needs `#rrggbb`). Tokens mirror bg / fg / cursor / selection; a unit test asserts that `--k-term-bg` and `--k-term-fg` in tokens.css equal theme.ts. A selected theme (`app.dark_theme` / `app.light_theme`, SETTINGS `[themes.<id>]`) is resolved by `src/lib/theme`: it overrides `--k-*` tokens inline on `<html>` and hands its terminal colours to xterm and `terminal_set_palette` in place of the base palette. `terminal_set_palette` re-push on theme change is unchanged.

|       | fg        | bg        | cursor    | selection |
| ----- | --------- | --------- | --------- | --------- |
| Dark  | `#d3d9df` | `#121519` | `#d3d9df` | `#2c3f52` |
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

### 6.1 Lamps (`$lib/ui/Lamp.svelte`, shown by `shell/AttentionDot.svelte`)

One span, silhouette from CSS only (no SVG). Same shapes everywhere: sidebar, pane headers, the Queue, palette, board cards. Nothing pulses or blinks.

| State                        | Shape                                                                                         | Size | Token                  |
| ---------------------------- | --------------------------------------------------------------------------------------------- | ---- | ---------------------- |
| `needs_input`                | filled dot with a 2px halo of the same colour at 35% (`box-shadow: 0 0 0 2px color-mix(...)`) | 10px | `--k-lamp-needs-input` |
| `working` _(from Faceplate)_ | hollow ring, 1.5px stroke                                                                     | 10px | `--k-lamp-working`     |
| `error`                      | filled square rotated 45° (diamond)                                                           | 9px  | `--k-lamp-error`       |
| `done`                       | filled dot, no halo                                                                           | 8px  | `--k-lamp-done`        |
| `activity`                   | small dot                                                                                     | 6px  | `--k-lamp-activity`    |
| `parked` (#142)              | hollow ring like `working`, quiet                                                             | 10px | `--k-fg-muted`         |
| `none`                       | renders nothing (the slot keeps its width in tabs)                                            |      |                        |

- `working` and `parked` are not Attention levels: `working` is derived from `sessions[].status === 'working'` in the store, `parked` from a parked work item (FLOW §2.2). Precedence on an aggregate (project): needs_input > error > working > done > activity.
- `aria-label` stays as today; lamps are never the only accessible name.
- Lamps in the sidebar sit in a 10px socket so the diamond stays sharp on 1x screens.

### 6.2 Sidebar of work (`shell/ProjectRail.svelte`)

The sidebar lists work, not tabs: the three places, then each project's unfinished work items and Claude sessions, everything else folded. Action ids and test ids still say `rail`.

- **Width**: 288px by default (`--k-rail-width`), set by dragging its right edge between 200 and 480px (`sidebar.svelte.ts`). The edge is a 10px hit area with a 2px accent line on hover, drag and focus; `←` `→` move it by 16px (`⇧` 64px), `Enter` collapses. `rail.toggle` (`⌘B`, `Ctrl+B`) or the panel button at the bottom collapses it to the 64px icon column (`--k-rail-width-compact`); a window under 1400px starts collapsed and the button still opens it until the next start. Width and collapsed state survive a restart (webview storage). `--k-bezel`.
- **The Queue folds it**: while the Queue is shown (Settings closed) the sidebar is compact, since the Queue lists the same work. Opened by hand there, it stays open until the view changes.
- Top to bottom (no search row: the search field is in the top bar, §6.3): the three places, 44px two-line rows (`--k-sidebar-row-height`), each with one count on its sub line: **Queue** (`inbox` icon, `9 your move`), **Tickets** (`ticket` icon, `10 assigned to you`), **Pull requests** (`git-pull-request` icon, `3 ask for your review`); then the bands; **Open project…** when closed projects exist; **Other Claude sessions**; at the foot **Settings** and the collapse button. **Add project** is in each band's `⋯` menu and the palette (`project.new`). Only the current destination (a row, a folded row hiding it, or a place) wears the selected row style and the full-height index bar (a place's bar is the accent); rows drop theirs while a place is shown.
- Counts: no red badge. The count is the sub line in the wide sidebar; the compact one shows it as a `--k-bg-selected` pill with `--k-fg-muted` digits in the badge slot. Lit rows say what waits on you.
- **Bands**: one per open project except Home. A caption (an 8px project-colour mark, the project name in `--k-fg-muted` 13/500, the band's row count in `--k-fg-subtle` 11px, `⋯` on hover) over the project's **work rows**, then its **Claude rows**, then one **n more** row. No project tile, status word or lamp on the caption: its rows carry them.
- **Work rows** (`--k-rail-item`, 32px, one line): one per unfinished work item of the project whose tab is open, or whose phase is your move or Claude working (not the Queue's **Waiting on others**, #443): closing the tab of work that waits on a reviewer, checks or a deploy drops its row (the Queue, Pull requests and `⌘K` still list it), and the row comes back by itself, in its slot, when the work needs you again: `[lamp socket 10px][key][title][stage word]`, the key and the stage word (Claude, PR, Review, Merge, Deploy) in `--k-fg-muted` 11px. The title is the ticket's or PR's, whichever way the tab was opened. Rows keep the order their work started in, so a phase change never moves one. A row whose tab is closed shows its lamp and stage word like the others, in `--k-fg-muted`, with no `×`; a click resumes it (`work_resume`, as the palette's Work items group does). `⌘1`–`⌘9` (`project.goto.N`) jump to the nth work row across bands; its digit sits at the right end. A 2px project-colour bar on the left (raw resolved colour, §2.1; 55% mix), full colour and 3px on the selected row, which also wears `--k-bg-selected` and a 600 title. `×` on hover and focus, over the stage word and digit. A tab in its own window adds the `app-window` glyph before its key. A workflow run shows its phase on a line under its session's row.
- **Claude rows**: a Claude session without a work item (started with New session, `⌘T`), shown in a tab or live in the background: `[lamp][✻][session name][status word]`, after the work rows. It lights like a work row when it needs you; a click shows its pane.
- **n more**: the other tabs of the project (shells, web tools, diagnostics, detail panes) and its background sessions other than Claude fold into one row, `[lamp][chevron][n more]`; its lamp is the highest of the folded tabs. A click opens it in place: the folded tabs as `[lamp][glyph][title]` rows in tab order, then the background sessions as `--k-fg-muted` rows.
- **Lit row** _(from Faceplate)_: a row whose session needs input takes `--k-lit` (`color-mix(in oklab, var(--k-lamp-needs-input) var(--k-lit-mix), var(--k-bezel))`, `--k-lit-mix` 14% light, 30% dark), the brightest thing in the window, readable in peripheral vision. The selected row keeps its face; its lamp speaks.
- **Other Claude sessions**: Home's Claude rows (and its work rows, if any), then the Claude sessions Kelta did not start, dimmed rows `[lamp][bot glyph][title]`, under their own heading; a click jumps to their window or tmux pane. Home's other tabs fold into one **n shells** row below them (at the foot of the bands when the heading is not there).
- Tools are not places: a terminal tool opens zoomed in the current tab (§6.4), from its key (`⌥⌘1`–`⌥⌘9`, a tool's own key) or the palette (`⌘K`).
- **Compact** (collapsed, < 1400px, or folded by the Queue): the caption becomes a 1px rule in the project colour and each work or Claude row a tile (bar, lamp, glyph; a work row's key and stage word hide); the **n more** rows and their tabs are not shown; names are in the tooltips and the accessibility tree; sub lines, `×` and `⋯` are hidden.
- **Drag and drop**: rows do not reorder (their order is the work's start). A pane dragged by its header (§6.4) over a row joins that tab (`pane-over`: accent drop line); over a band's empty area it becomes a tab of its own; never across projects. A row or pane dropped outside the window moves to its own window. A caption drags to reorder the projects.
- Focus: 2px `--k-focus` outline inset 2px.
- Drag-over: 2px accent insertion line.

### 6.3 Top bar and desk header

**Top bar** (`shell/ContextBar.svelte`): one bar across the top of the window, above the sidebar and the panes: `--k-contextbar-height` (30px), `--k-bezel`, `--k-font-size-sm`. It holds what is window-level: going back, finding things, being called, starting something. The status strip stays app-wide, the pane header per pane, the sidebar keeps the lamps. It lists no tabs and no tools: the sidebar lists the work (§6.2), the palette and the tool keys open the tools.

- Left to right: `‹ ›` (`nav.back` / `nav.forward` on the jumplist; `‹`'s tooltip names its target), the breadcrumb, the search field in the centre, the project scope, the "needs you" chip, `+ New`.
- Breadcrumb: `▌project › row`: a 3px project-colour bar, the project name in `--k-fg-muted` 500, `›` in `--k-fg-subtle`, the row in `--k-fg` 600 (ellipsis). It is a pure drag region (double-click zooms the window), never clickable.
- Search field: `search` icon, "Search or run…" and the `palette.open` chord, on `--k-well` with a 1px `--k-border` (`--k-border-strong` on hover), up to 320px; a click opens the palette (§6.6).
- Scope: `All projects ▾` (`shell/ScopeSelect.svelte`), the one scope of the Queue, Tickets and Pull requests (§6.8). The lists' toolbars do not carry it.
- "Needs you" chip, the one call signal: the Claudes blocked on you (the Queue's Claude needs you rows that are sessions), `n needs you` with its lamp and `⌘J` (`attention.next`); hidden at 0. One waiting: a click goes to it; several: a menu of them. Focus mode never hides it: it goes quiet, the lamp drawn as an outline with no pulse.
- `+ New`: a menu of **New work item** (`work.new`) and **New session** (`session.new`).
- Claude session: `✻ name`, then the cwd and the model on the right. Terminal: `❯ title` (OSC 2) and the cwd. Home: `Home › ❯ zsh` and the cwd (`Home` alone on the welcome screen). A session running workflows adds a `⚙` chip that opens its runs.
- A place (Queue, Tickets, Pull requests): its name, and on the right the "underneath" chip `‹ project › row` that leaves it. The place's own 40px `.k-toolbar` (filters) stays under the bar.
- A detached window (§6.4) has no top bar; the native title bar sits above its tab.
- Never in the bar: session status, ctx % and session cost (pane header); rate limits, Claude slots, restart, prefix (status strip); row lamps and peek (sidebar).
- Narrow bars (container queries) drop, in order: the cwd/model, the search text and chord (its icon stays). `‹ ›`, the search icon and `+ New` never drop.
- Window chrome: native decorations keep the system title bar, whose title follows the breadcrumb. macOS custom chrome draws the traffic lights over a 72px leading inset of the bar (overlay title bar); Linux custom chrome adds minimize, maximize and close after `+ New` and the edge resize handles. Empty bar area is `data-tauri-drag-region`. Hyprland/Sway: no title bar, the top bar is the top edge.

**Desk header** (`views/work/WorkContext.svelte`): on a work tab the top bar shows no breadcrumb; the header above the tab's panes names the item. It sits outside the pane area, so it stays over a zoomed pane, on `--k-bg` with the screen radius.

- Line 1: the mono key, `▌project` and the title.
- Line 2: one sentence of why, `<Your move | Claude working | Waiting on others>: <phase label>, <detail>` (the Queue's zone, FLOW §2.2); a click opens the Story panel.
- The stage track (`workStage`: Ticket, Claude, PR, Review, Merge, Deploy) with all six stages named under their dots (`labels`: the current one `--k-fg` 600, the others `--k-fg-subtle`; a long name widens its column; below 260px of track only the current one stays; list rows keep the compact track); the mark is filled when it is your move, a ring while it waits, a diamond when something broke.
- The primary button (the next step, `↵`; its blocked reason in the tooltip) and `Actions ⌘.` (the work menu, FLOW §2.5).
- A narrow header drops the track, then the `Actions` label. Status, PR, branch, behind and cost live in the Story panel's summaries (§6.4). The native window title stays `project › KEY title`.

### 6.4 Panes and the Story panel (`shell/PaneHost.svelte`)

- A pane is a screen set into the housing: `--k-radius-screen` corners, a 1px `--k-border` ring, `--k-well` face. Panes are separated by `--k-gap` (6px) of `--k-bezel`. The focused pane swaps its ring for a 2px `--k-accent` ring.
- Pane header 32px (`--k-pane-header-height`), `--k-bezel-raised`, 13px `--k-fg-chrome`; the focused header takes `--k-bg-selected`, `--k-fg` and a 600 title. An unfocused pane whose Claude asks wears the call light (`--k-lit`) on its header.
- Terminal content is never dimmed. Inside a terminal pane the focus indicator is the ring and the header tint, never an outline around xterm.
- **Tool pane**: a terminal tool opens as a pane split off the focused one and zoomed over the tab, in the tab's worktree (or the project folder). Its key or `Esc` `Esc` closes the pane and gives the focus back (the previous layout); the session keeps running and the key brings it back zoomed.
- Splitter: visually the gap, `--k-gutter-hit` (10px) hit area, `--k-border-strong` on hover / drag.
- "Fix hooks" and similar header actions are accent text buttons.
- **Drag a pane** by its header. While it moves, the pane under the pointer shows a drop indicator: a 2px `--k-accent` border with an 18% accent fill and `--k-radius-screen` corners, covering the half it would take. The pointer within a quarter of an edge picks that edge (`left`, `right`, `top`, `bottom`: the pane splits there); the middle half picks `center` (the two panes swap, the indicator covers the whole pane). The dragged pane keeps its session and view, and ends focused. `Esc` or a drop on nothing cancels. Sidebar targets are in §6.2.
- **Story panel** (`views/work/StoryPanel.svelte`): the Desk's right side, docked on a work tab outside the pane tree, 340px by default and 260–640px by dragging its left edge (a 10px hit area in the gap, 2px accent line on hover, drag and focus; `←` `→` move it by 16px, `⇧` 64px). `work.story` (`⌘I`, `Ctrl+I`) shows or hides it; the header's why line (§6.3) shows it. Open by default in a window at least 1280px wide; the open state is kept per tab, and it and the width survive a restart (webview storage, `story/panel.svelte.ts`). Focus mode hides it. It sets into the housing like a pane (`--k-radius-screen`, 1px `--k-border`, `--k-bg`, `--k-gap` around it), `--k-font-size-sm`. Sections in stage order, each a component in `views/work/story/`:
  - **Since you left** (`ReturnBrief`): where you left the item, only while it applies; a closed panel shows itself for it.
  - **Ticket**: source, status (the StatusPicker) and the ticket's acceptance criteria, `2 of 5 criteria checked`.
  - **Claude**: status with its age, Claude's task list, the last message (folded after 4 lines, `e` expands), cost against the budget, context and model.
  - **Changes**: `+a −b, n files` since you looked, file by file (a click or `]f` / `[f` opens the delta at that file), Review, Mark reviewed and a note to Claude, then the review notes.
  - **Pull request**: number, title, draft or ready; the checks (failed first), the reviewers and who it waits on, the unresolved threads; the buttons its phase offers.
  - **Log**: Kelta's start steps; it opens by itself when a step failed, with **Retry** and **Skip step** on it.

  A section is a 1px `--k-border`-ruled header, the title in 600 and a one-line `--k-fg-muted` summary on the right (ellipsis); the section of the item's current stage (the stage track, `stage.ts`) is open and its title takes `--k-fg`, the others fold to their header. The start saga's progress pane (`work_item`) is the same panel; a tab that shows it docks none.
- **Own window**: a pane menu item, a tab's row menu, or a drag out of the window moves a tab to a second window. That window shows the tab alone under the native title bar: no sidebar, top bar or status strip, the accent of the tab's project. Its sessions keep running. Closing the window, or "Move back to main window", brings the tab back. Meanwhile the main window shows that tab as an empty state, "This tab is in its own window", with **Show window** (primary) and **Move back here**.

### 6.5 Status strip (`shell/StatusBar.svelte`)

- 28px (`--k-statusbar-height`), `--k-bezel-raised`, 12px, tabular-nums, segments spaced 12px, no dots.
- No key hints: a list's keys are in its `?` panel (§6.8), and the global chords live in its `? All keys` dialog, the which-key panel and the palette.
- Right, exceptions only: prefix armed, "Restart needed", Claude slots (`Claude 3/4 · 1 queued`), the 5h and 7d rate windows. No count of waiting work: the Queue's header and the top bar's "needs you" chip carry it (§1.1).

### 6.6 Command palette and which-key panel

- **Palette** (`shell/palette/`): `palette.open` (`⌘K`) or the top bar's search field. Top-anchored at 14vh, `min(640px, 100vw - 32px)`, `--k-bg-float`, `--k-radius-lg`, `--k-shadow`; the list stops at 52vh.
- No title bar or close button: the 48px input (16px text) is the top edge, a hint line under the list names Move, Run and Close, Esc or the backdrop closes it. Query in mono. Results `--k-row-height` rows: 10px lamp slot (session and tab hits), 14px kind icon (no text label), title, meta, right-aligned kbd hint. Results are grouped, one header per group: with an empty query in the order Recent (the last places you were), Work items, Sessions, Projects, Actions, Tools, Commands, Tickets, Pull requests, Settings (every key, opening its section); with a query, groups in order of their best hit.
- Prefixes narrow the groups: `#` Tickets, `!` Pull requests, `>` Actions, Commands and Tools, `,` Settings (`# 311`, `>split`).
- Selected row: `--k-bg-selected` plus a 2px accent left bar.
- **Which-key panel** (`shell/WhichKey.svelte`): after the prefix (`⌃⇧Space`) and a pause of `keys.prefix_hint_ms` (400ms) with no second key, a panel at the bottom of the window lists a curated few of the keys that follow, at most 6 per group in plain words: Go to, This work (`KEY`, on a work tab; `↵` carries the next step's label), Panes, Tools here. The table of groups and labels is `HINT_TABLE` in `lib/keys/manager.ts`. The prefix stays armed until a key or `Esc`; `?` opens Settings › Keys.

### 6.7 Sheets, dialogs, menus, toasts

- All float: `--k-bg-float`, `--k-shadow`, radius 8 (one radius for every floating surface).
- Dialogs are top-anchored like the palette (not vertically centred), title 15/600, actions right-aligned, primary last.
- Sheets slide from the right edge (`--k-duration` in, `--k-duration-fast` out, no movement under reduced motion), width `min(<width>px, 100vw - 32px)`: 560 by default, 480 for the narrow ones (Fix with Claude, the source picker).
- Menus: 26px rows, kbd hints right in `--k-fg-subtle` mono.
- Toasts: bottom-right, above the status strip, one line + optional action, a lamp shape for kind (error diamond, done dot). No auto-dismiss timer is added beyond what the toast store already does; verify against check-no-timers.sh.

### 6.8 Lists: the Queue and the Library (Queue, Tickets, Pull requests)

One row grammar for all three, so they read as one instrument:
`lamp | id (mono 13, --k-info, 76px) | title (14/500, --k-fg-strong, fills) | meta (12, right-aligned, tabular)`.

- Scope: the top bar (§6.3) holds `All projects ▾` (`shell/ScopeSelect.svelte`), a menu of the open projects setting the one scope the three share (`shell/scope.svelte.ts`, kept in the webview's storage), so switching place keeps it. Home, or a project closed since, reads as All projects. The Queue keeps the scoped project's rows (a Claude session Kelta did not start has none) and its header counts them; the Tickets source menu and the Pull requests repo filter narrow inside the scope. The three render in `shell/GlobalHost.svelte`, outside any project; a project's tickets or pull requests are the same place scoped to it, never a tab (`tickets.open` / `tickets.board` / `reviews.open` with a project set the scope).

- Hierarchy: the title is the brightest text in the row (500, 600 on the selected row), the key wears one hue in every list (`--k-info`, never a lamp colour) so the eye runs down the key column, the state word takes its lamp's colour, and everything else stays muted.

- Rows `ROW_HEIGHT` (32px, `$lib/ui`), no row borders, no zebra. Pull requests rows have two lines: lamp, id and title on line 1, then the chips and meta left-aligned under the title, in full, wrapping to a third line rather than cut (`VirtualList` `measure`). A Compact switch in their toolbars (saved in the pane's `density`) brings back the one-line 32px row (no `measure`).
- Selected: `--k-bg-selected` (accent 14% into the well in light, 28% in dark: at least 1.6:1 on the well) plus `box-shadow: inset 2px 0 0 var(--k-accent)`. Hover: `--k-bg-hover`.
- Tickets rows are a CSS grid on fixed tracks every row shares (`TicketRow.svelte`), so optional data never shifts a column: lamp (16px) | key (mono 13 `--k-info`, as wide as the longest key in view, capped at 12 characters) | title (14, `1fr`, takes all spare width) | signals (90px) | status (112px) | age (44px, right-aligned, tabular) | source (124px, 12 `--k-fg-subtle`, truncated, only with more than one source and a grouping other than Source) | assignee initials (20px circle on `--k-bezel`, 10px letters, Team and Unassigned only). Each cell carries its own left padding inside its track, so a dropped track takes no gap with it.
- Chips in the title cell. Before the title: **Blocked** (#348, 1px `--k-danger` border and text, radius 3, 12px) and the **parent** key (#350, `--k-bg-hover` fill, `--k-fg-muted`, 13px, 16ch at most; not shown when the list is grouped by Parent). After the title: the source's chosen tracker **fields** (#351) as `label: value` chips (`--k-bg-hover`, 12px, 4 to 20ch, ellipsised; the value alone when the label is empty), then the local **tags** (#349). A tag is the one pill in the app: radius 999, 1px `--k-border`, a 6px dot in the tag's colour (`--k-fg-subtle` without one), 120px at most, never shrunk. The title ellipsises first, then the field chips; the row's compact actions lay over the end of the title and never reflow it.
- One baseline: text cells align on their baseline and the line is centred in the row (`--k-row-height`); lamps, signal icons and status dots are centred on the row.
- Title first: the sprint, the work phase in words, the PR state, who it waits on and the updated time live in the row tooltip and the detail column, not in the row.
- Signals: fixed slots, icons with tooltips (and the words for screen readers), `--k-fg-subtle`; the phase word (#373, wide lists) takes its lamp's colour (needs input, working, error, done), the others stay `--k-fg-subtle`: local work (`git-branch`; `circle-alert` in `--k-lamp-error` when the work failed), the pull request (`git-pull-request` plus the PR lamp, a button opening it), waiting on someone (`clock`, `--k-warn` when late), marked for a batch start (`layers`, `--k-accent`). An error keeps the error lamp in the lamp track too.
- Status: a 7px dot in the source's colour for that status (`status_colors`, SETTINGS §2), else the category colour (`--k-info`, `--k-warn`, `--k-ok`, `--k-border-strong`), centred on the cap height, then the tracker's own status name (12, `--k-fg-muted`); a button opening the StatusPicker. No chip, no bar: the bordered pill appears only in the detail header, where it is the picker.
- Narrow lists: below 720px of list width the source and assignee tracks and the field chips drop (the split view at 1440 leaves the list about 670px, so the age stays); below 480px the age drops; below 420px the signals drop too (they stay in the row tooltip and the lamp's). Key, title, status, Blocked, parent and tags never drop. An optional mark hides whole; it is never cut in half (§6.14).
- Age: `7d` tabular text after 7 days in a non-done status, `--k-fg-subtle`; `--k-warn` text from 14, `--k-danger` text from 21. No fill, no border (a lamp must stay the only coloured shape). Empty below 7 days; its track stays.
- Row actions appear on the selected row only, after the title and its chips (the title gives way, never covered): the next step (accent outline), or Start / Resume work, then Move, both outlined with an icon and a word (`.k-row-act`), keys in their tooltips, then `⋯`: a menu of every other key of the row, each entry with its key on the right like a menu bar. Row `⋯` menus, the detail More menus and the work menu (FLOW §2.5) list what can run now first; an entry that cannot run goes to a last group, **Not available now (n)** (12 `--k-fg-subtle` header after a rule), label in `--k-fg-muted` with its reason on a second line (12 `--k-fg-subtle`), at most 5, and its key flashes the reason (`Menu` `reason`, `hidden`). Nothing is keyboard only. Pull requests rows show a Next step column: Open review as a button (what `↵` runs), and under Yours the Queue's step as quiet text (Fix with Claude, Nudge Bob); the selected row also shows `⋯`; board cards show `⋯` on their first line. Signals (work, PR lamp, waiting) stay in their own column, never mixed with actions.
- Multi-select: marked rows get `--k-bg-selected`; while a selection exists, the lamp track of every row is a checkbox (Select in `⋯`, `x` or Cmd/Ctrl+click starts one); `Esc` or Clear selection in `⋯` clears.
- Section and group headers say what they hold: a one-line hint after the count (`.k-group-hint`, 12 `--k-fg-subtle`, ellipsised first). Who and Pull requests tabs carry the same in their tooltip.
- Pull requests meta (line 2): `+214 −38` in mono tabular (`--k-ok` / `--k-danger`), checks lamp. Toolbar: filter, then the For me / Team / Mine tabs (tabular counts, omitted when unknown). Team rows show `@author` (or "you") in place of the name; grouped by ticket (`G`), each group has a 13/600 `--k-fg-muted` header `SHOP-142 Checkout timeout` with a tabular count, not selectable (`j` `k` skip it).
- Pull requests: rows under Yours are dimmed (0.6, as Elsewhere in the Queue), and their brief's next step is outlined, not filled: that step lives in the Queue. Someone else's brief says under its buttons what Review locally does, in one sentence.
- **Queue** (`views/inbox/`): a header (`Queue`, then one summary line `9 your move, 2 Claude working, 2 waiting on others` in tabular figures, `Updated 5m ago` when a source is stale, and `+` New work item), the filter bar, then the list beside the selected row's brief.
  - Zones: **Your move**, **Claude working**, **Waiting on others** (FLOW §3.1). Each zone header is sticky, 13/600 sentence case (`--k-fg-strong`; Waiting on others `--k-fg-muted`), with its tabular count and a one-line hint (`.k-group-hint`). Inside a zone the rows run in section order with no section header; only a folded section (Waiting to deploy) has a header line with a chevron and its count.
  - Row: a grid of id (76px, mono `--k-info`) | the title over its why line (the reason in `--k-fg-muted`, a warning in `--k-warn`) | the stage track (160px; the column header names its ends, Ticket and Deploy) | the next step as an outlined button (180px; the column header "Next step") | last change (64px, right-aligned). A 3px project-hue bar on the left edge (raw resolved colour). Rows measure to their content (`VirtualList` `measure`). A row whose Claude asks has its option buttons on a line under it. Rows of Claude sessions Kelta did not start (Elsewhere) are dimmed. Next up rows drag to reorder (2px accent line on the row they land on).
  - Brief (`views/work/shared/Brief.svelte`), a 340px column on the right (300px below 1115px of pane, stacked under the list below 835px, keeping at least half the height): the id and project, the title, the labelled stage track, one sentence of why (the phase and its detail; Claude asks you to choose when it asks), the next step (primary, `↵`) and the two likely after it, the rest behind **n more actions** (`.`), Claude's last message (folded after 6 lines, `e` unfolds it) and the facts. When Claude asks or replied, its question's options are numbered (`1`–`9` answer) and a **Reply to Claude** field sits under the message.
  - Narrow panes (container queries): below 1115px the next step moves under the title; below 699px the column headers drop and the stage track moves under the title.
  - First run (`views/onboarding/YourLoop.svelte`): while the Queue is empty and no item has shipped, **Your loop** replaces the empty state: the stage track and five steps (Add a project, Connect a ticket source, Start work on a ticket, Review what Claude changed, Ship), each checked from real state, the current one with its button.
- Toolbar `--k-bezel-raised`, at most six controls, in two groups. Left, what you look at: the source menu (`v`, only when there is a choice), then the Who tabs (Mine, Unassigned, Team, with tabular counts, omitted when unknown). Right, how it is arranged: one View menu (a floating panel holding Person, Group, Sort and the Current sprint toggle; an accent dot on the button while a person or the sprint narrows the list), the list/board toggle (one source in effect), the filter input and an icon-only Refresh. Keys are unchanged (`/`, `1`-`3`, `v`, `Shift+G`, `f s`, `Shift+R`).
- No key hint line under the lists: rows name their next step on a button and `⌘.` lists every action. `?` in Tickets, Pull requests or the Queue (`ListKeys`) opens the which-key panel's floating panel (`shell/KeyPanel.svelte`, §6.6) over the bottom of the list, filled with that list's keys from the shared keymap (`views/work/shared/keymap.ts`) in the groups of its verb table: Move (`j k`), Act on the row (`↵` with what it will do, `s`, `v`, `o`, `m`, `c`), Select (`b`, `x`, start marked), Filter (`/`, Who, `f` chords). The next key closes it and acts; `Esc` only closes it. `?` again, or its `? All keys` button, opens the full key list in a dialog, the global chords (Commands, New session, Split, Close pane, Next waiting) last. `keys.hints` is deprecated and ignored.
- Tickets are grouped (Flow by default: Doing, Waiting, Ready, Backlog, Done in the last 7 days; Status orders in progress, in review, to do, unknown, done; Done collapsed) under 13/600 `--k-fg-muted` sentence-case headers with a tabular count, 26px. The PR lamp is one lamp for CI and review (changes requested `needs_input`, CI failed `error`, running `working`, approved or passed without required review `done`, awaiting review `activity`); its label names both, with the `#N` / `!N` number.
- Doing header: turns `--k-warn` with a short note ("Above your limit of 3") when the count passes `tickets.wip_limit`; nothing else changes.
- Split view: below ~720px pane width there is no detail column. Wider, the detail sits on the right (`--k-well`, §6.9) and the side with the keyboard shows a 2px `--k-focus` inset bar on its left edge.
- StatusPicker (`m`, status chips, detail, work bar, palette): the header is the ticket's workflow on one line of native status names in category order, the current one `--k-fg` 600 with its category bar, the rest `--k-fg-subtle`. Below, the legal transitions numbered `1`-`9` with the digit as a mono kbd hint on the right; the digit moves at once. The same menu filters as you type, shows the tracker's message and Open in browser on an error, and with several tickets lists only the statuses they all share.
- Source picker sheet (§6.7, `min(480px, 100vw)`, from the right): account Select, search input, rows of kind (12 muted), label (13), detail (11 muted); added hits read "Added".
- No key shadows or per-row effects: rows live inside VirtualList and must stay cheap to paint.

### 6.9 Detail panes (ticket, review) _(meta grid and measure from Spine)_

- Key in mono 13 (copyable), title 20/600 `--k-fg-strong`, then a label/value meta grid: `display: grid; grid-template-columns: auto 1fr auto 1fr`, labels 12 `--k-fg-muted`, values 13. Stacks to `auto 1fr` under 480px pane width.
- Section heads (Description, Discussion, Checks) are `.k-section-title`: content size, 600, a 1px `--k-border` rule above with 12px padding and 24px margin. They are the largest headings in the pane after the title.
- `HtmlContent`: 13/1.5, `max-width: var(--k-measure)`, links `--k-accent` underlined, code/pre mono on `--k-bezel-raised` radius 2, blockquote 2px `--k-border-strong` left rule.
- `HtmlContent` headings never outgrow the section heads: `h1`-`h6` at 1em/600 (`h4`-`h6` `--k-fg-muted`), `margin: 1em 0 0.4em`, none above a first child. Lists 1.5em indent, 0.2em between items; task-list checkboxes hang in the bullet gutter, read-only. `hr` is a 1px `--k-border` rule. A ticket description sits in an indented block with a 1px `--k-border` left rule, so its headings read as content.
- Pull requests: `feat/x into main` in mono 12 under the title, diffstat tabular.
- Tickets: a `Pull request` row (PR chip, CI lamp, review state in words); the status value is a button opening the StatusPicker of §6.8. Order: key, title, status, action bar, meta grid (assignee, priority, sprint, estimate, due, labels, updated; assignee `u` and priority `!` are buttons opening a filtered picker), Pull request rows, description, comments with a compose box.
- Pull request rows: one line each, `#N` chip, title filling, then CI lamp and review state in words; wraps under the title when narrow.
- Action bar: the primary `sm` Button (Start / Resume work, always the primary; when blocked it shows the reason), then one row of outlined ghost `sm` Buttons: Move `m`, Comment `c`, Open PR `p` only when the ticket has a PR, and More (`Cmd+.`, `Ctrl+.` on Linux), a menu with Assign to me `a`, Unassign `Shift+A`, Block / Unblock ticket `b`, Refine with Claude `r`, Copy branch `y`, Open in browser `o`. Each row button shows its key as a chord cap, and the menu shows its keys; every key works without the bar. Plugin actions follow. A refine shows its proposal under the sub-tasks as mono pre-wrapped Markdown on `--k-bezel-raised`, with Post as comment (primary), Copy and Discard. An action that cannot run keeps its place at 55% opacity with `aria-disabled` and the reason as tooltip; it stays reachable with Tab.
- Embedded in the split view (Tickets and Pull requests, list | detail on `--k-well` with a 1px `--k-border` gap, 720px and wider) the detail drops the pane padding it would double, and shows the 2px `--k-focus` bar when it holds the keys.

### 6.10 Board

- Lanes 264px, `--k-bezel` trays (radius 3) on a `--k-well` board with 8px gutters, so each lane and an empty drop target has an edge. Header = name (13/600) + count (12, tabular, `--k-fg-subtle`).
- Cards `--k-well`, radius 2, 1px `--k-border`, no shadow. Line 1 (fixed 18px, chips centred in it, so titles line up across lanes): key (mono 11) + lamp of the linked work item's sessions, if any _(from Faceplate)_. Lines 2-3: title 13, clamped. Bottom: avatar + age.
- Drag target: 2px accent insertion line. Focused card: focus ring.

### 6.11 Chips and badges

- Flat: radius 3, `--k-bezel-raised` fill, `--k-fg-muted` text 12, no coloured fill (a filled colour pill would compete with lamps). The row chips of §6.8 (Blocked, parent, fields) follow it; a tag is the pill exception, and its colour sits in a 6px dot, not in the fill.
- Badges and chips that are optional hide when the room runs out (container queries); their text is never truncated to a few letters.
- Status: no chip. A 7px category dot (ok / warn / info / `--k-border-strong`) before the status name; in the detail header it sits inside the bordered Button that opens the picker.
- Counts in chips are tabular.

### 6.12 Forms (Button, TextInput, Select, Toggle, Tabs, Kbd)

- Button: height 32 (28 for `sm`), radius 6. Default: `--k-bezel-raised` + 1px `--k-border`. Primary: `--k-accent` fill + `--k-accent-fg`. Danger: `--k-danger` fill. Ghost: transparent with a 1px `--k-border-strong` outline, so it reads as a control (#384). Labels name the action ("Start work", "Request changes").
- TextInput / Select: `--k-well` face, 1px `--k-border`, radius 6; they read as small screens on raised toolbars. Focus = focus ring, border unchanged.
- Toggle: track `--k-bezel`, thumb `--k-well`; on = accent track. The thumb carries a 1px notch on the on side so state is not colour-only _(from Faceplate)_.
- Tabs (in-view segmented): text only, active = `--k-fg` 600 + 2px accent underline, same grammar as the tab bar.
- Kbd: one quiet mark per chord (#385): mono 12 `--k-fg-subtle`, one 18px box with a 1px `--k-border` around the whole chord, radius 3. macOS writes the chord as one word (`⌘⇧K`), Linux joins the keys with `+` (`Ctrl+Shift+K`). Alternative keys (`j k`) are separate marks.
- Settings: one layout grammar for every section, generated or custom (`views/settings/layout/`). No section adds layout CSS of its own; only content (a table, a swatch grid) keeps its styles.
  - **Place** (#410): Settings belongs to no project, so it opens over the window like the global views (`shell/GlobalHost.svelte`), never as a tab: the sidebar stays, its Settings row wears the selected style and the other rows drop theirs. `Esc` or `⌘,` closes it onto the tab and pane underneath; going to a view or a row also closes it. Deep links (`settings.open {section, toml, source_id}`) retarget the open Settings. Settings tabs saved before #410 are dropped on load.
  - **Nav**: sections under four 13/600 `--k-fg-subtle` sentence-case group headers (Basics, Work, Integrations, Advanced), the §6.8 group-header grammar. Thin sections merge where they belong (Polling lives in Performance).
  - **Section head**: title 16/600 (`h1`), one-line blurb in `--k-fg-muted` capped at `--k-measure`. Every section renders through `SectionForm`: blurb, the section's own content, the generated keys, then a closed **Advanced settings** disclosure (chevron, count in `--k-fg-subtle`) holding the rarely changed keys (`ADVANCED` in `lib/sections.ts`), then, for keys no form edits, one quiet line naming them in mono with a ghost **Edit TOML** button. Search still reaches every key, advanced ones included.
  - **Group** (`SettingsGroup`): 14/600 title (13/600 `--k-fg-muted` when nested, its body indented behind a 2px `--k-border` rule), optional badge after the title and actions at the right end of the same line, a 13 `--k-fg-muted` description, 24px above.
  - **Row** (`SettingsRow`): label left with its markers (source badge, needs trust, restart, reset) at the right end of the label column, help 13 `--k-fg-muted` under the label, the control in a 200-340px right column; buttons in that column align right. A toggle spans the row with its switch in the control column. Below 560px of row width everything stacks. 8px vertical padding, a 1px `--k-border` rule under each row.
  - **Card** (`SettingsCard`) for every list editor (sources, accounts, projects, tools, triggers, plugins, keyed lists, maps): `--k-well`, 1px `--k-border`, radius `--k-radius`, no shadow. Head line: name (600, mono for an id), then badges and muted meta, actions at the right. An optional body under a hairline holds rows or groups.
  - **Layers stay quiet**: the Global / Project / Repo tabs carry their meaning in a tooltip; a one-line note under the toolbar appears only on Project and Repo. A key at its default shows no chip (the "Default" text stays for screen readers), so the keys set in a file are the ones marked.
- Path fields (#365): a text input with a secondary **Browse…** button beside it that opens the system picker (a folder, or a file with a filter for fields that take one), starting at the field's path when it exists, else the home folder. Cancel leaves the field alone; a failed picker is a toast. Used by New session, New project, plugin install and the path settings. Typing the path stays possible.
- Project colour picker: the active theme's slot colours (the 12 ANSI hues, less the ones the theme paints alike, §2.1) as 18px chips (tooltip "Green", "Bright blue"; picking one stores `theme:<slot>`), then a custom hex field _(from Spine)_. A custom hex shows as its own chip with a "Custom" mark and stays selected across theme switches.

### 6.13 Empty and error states

- One sentence stating the condition plus one action button, left-aligned at the top-left of the pane, 16px padding. No illustrations, no centring.
- Examples: `No tickets match "auth".` [Clear filter]. `Jira rejected the token (401).` [Open tracker settings]. `No tracker bound to this project.` [Bind a tracker].
- Errors say what happened and how to fix it; they do not apologise.

### 6.14 Language (#344)

English, French and German. English is the source: every visible string goes through `t()` or `tn()` (`lib/i18n`), keyed by its English text. `fr.json` and `de.json` hold the translations, and a test fails when a key the source uses is missing from either. `app.language` (`system`, `en`, `fr`, `de`; Settings → General) applies at once, and dates and relative times follow it. Not translated: ticket and PR content, terminal output, config keys and TOML.

French and German run longer than English, German in long compound words. Every label is designed for the longest of the three:

- No width that fits only the English word. Buttons, tabs, sidebar rows, chips and settings rows size to their text or sit in a flexible track; a placeholder has room for its translation (the settings search, #362).
- Content may end in an ellipsis: a ticket title, a branch, a status name, a path. A label may not. If it will not fit in all three languages, shorten the English (a verb, not a sentence) and put the full text in the tooltip.
- An optional mark hides whole when the room runs out: field chips, source and assignee columns, the age, signals, key hints under a list (a hint that does not fit wraps out of view), toolbar counts. It is never cut to a stub. Required marks (key, title, status, Blocked, tags) stay.
- One message per sentence, with `{name}` placeholders and `tn()` for plurals. Never join fragments: word order moves between languages.
- Chords, keys and ids stay as typed (mono).

Check a change in German and French before it lands: switch the language, then look at the sidebar, the top bar, the desk header, the status strip, the Queue, the Tickets list at 720px, and the sheets.

## 7. Focus and keyboard

- `:focus-visible`: 2px `--k-focus` outline, offset 1px (inherits the project hue). On bezel surfaces where an outline would merge with an edge, use the two-ring box-shadow from §6.2.
- Terminals: focus shown by the pane ring and header tint only.
- Every hover-only affordance (tab close, row actions) also appears on `:focus-visible` / `:focus-within`.
- Selection (`::selection`) uses `--k-bg-selected` outside terminals.
- Scrollbars 10px (2px transparent inset), `--k-border-strong` thumb, transparent track.

## 8. Motion

Motion explains, never decorates (#398): it says what is loading, where a thing came from or went, what just changed. Anything that only looks nice is out.

- **Properties**: `transform` (and `translate`/`scale`/`rotate`) and `opacity` only, through CSS transitions and keyframes or Svelte `transition:`/`animate:` directives. No JS timers, no `requestAnimationFrame` loops (`scripts/check-no-timers.sh` stays green).
- **Tokens** (`tokens.css`, mirrored in `lib/ui/motion.ts`): `--k-duration-fast` 90ms (hover/press, menus, every out transition), `--k-duration` 150ms (rows moving, sheets, toasts, disclosure), one easing `--k-ease` (ease-out cubic), `--k-delay-show` 150ms before a loader shows.
- **Nothing loops at idle.** An `infinite` animation is allowed only on an element mounted while its operation is in flight (`Spinner`, `LoadBar`); a pulse runs `--k-pulses` (3) times. `idle.spec` runs with motion on and asserts no running animation.
- **Where**: loaders (the load bar under the Queue, Tickets and Pull requests while they fetch; a spinner in the lamp slot of a row whose action runs); rows that change place slide there and rows that leave fade while the rest close up (`VirtualList`, never on scroll or first measure); folding sections turn their chevron; menus and the peek popover grow from the point that opened them; sheets, dialogs and toasts animate in and out; a lamp that starts needing input pulses.
- **Levels**: `app.animations` writes `<html data-motion>`. `reduced` (the `auto` default in safe graphics mode) keeps fades and loaders, sets `--k-shift` and `--k-pulses` to 0. `off` and `prefers-reduced-motion: reduce` end every animation and transition at once. e2e runs with reduced motion.

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
- **Partial themes over Bezel.** A theme overrides only the tokens it names (chrome surfaces, text, borders, status colours, terminal colours) on top of the Bezel base of its mode; lamps, hover/overlay tints and `*_fg` stay Bezel so the lamp shape and contrast rules hold. A theme's accent is only the `--k-project` fallback for projects without a colour, so the project hue stays the accent.
- **Neutral terminal cursor.** Project-hued or amber cursors would force a palette re-push on every project switch and clash with nvim themes.
- **Panes are rounded screens, not bordered boxes.** A 6px bezel gap and a 1px ring set each terminal into the housing; the focus ring replaces the old header tick. The first version had a 1px gap, no ring and square corners.
- **Mono only for pasteable strings, no middle dots, no uppercase.** Each mark encodes data; template chrome is gone.
- **Top-anchored dialogs and palette.** The eye stays near the tab bar where the keyboard user already looks.

## 10. Rollout and checks

1. Replace `tokens.css`; update `theme.ts` arrays; add the token/theme parity unit test.
2. Edit AttentionDot, ProjectRail, ContextBar, PaneHost, StatusBar, Button, Toggle, Kbd, HtmlContent, list rows, Board; set `--k-project` in Shell.
3. Run `VITE_IPC=mock pnpm --filter @kelta/ui dev` and screenshot both themes with Playwright; `pnpm --filter @kelta/ui run e2e`; `bash scripts/ci-local.sh` (bundle budgets, no-timers).
4. Check the pane gap and ring and the lamp shapes at 100% zoom on a 1x Linux display.
5. Switch to German and French and look at every screen you touched (§6.14).
