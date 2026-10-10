# Colour themes

Kelta has one theme slot per mode: **Settings → General → Dark theme / Light theme** (global settings only).
`app.theme` (system, dark or light) picks the mode; the slot of that mode picks the colours. Click a card,
or use the arrow keys and Enter, to choose it. Custom themes can be deleted from their card.

## Import a VS Code theme

**Import theme...** under the two grids reads a VS Code colour-theme file (`.json` or `.jsonc`, up to 5 MB).
Kelta saves it as `[themes.<id>]` in the global `config.toml` and makes it the theme of its mode. `app.theme`
is not changed: if the theme is for the other mode, switch the mode yourself.

- To import the theme you use now, run **Developer: Generate Color Theme From Current Settings** in VS Code
  and import the file it opens. This also works for themes packed in extensions.
- Themes that `include` another file cannot be imported: generate the file as above.
- Only the workbench and terminal colours (`colors`) are used; syntax colours are ignored.
- The id is the theme name in kebab case. A name taken by a built-in theme gets `-custom`; a name taken by
  one of your themes asks before replacing it.
- Colours with alpha are blended over the editor background (terminal colours over the terminal
  background). Missing colours are derived from the others, so the imported theme is always complete.

| Kelta | VS Code (first one present) | Derived when absent |
|---|---|---|
| `base` | `type` (`dark`, `hc`, `hcDark` / `light`, `hcLight`) | dark unless the well is light |
| `well` | `editor.background` | Bezel well |
| `bezel` | `sideBar.background`, `activityBar.background` | well + 6% fg |
| `bezel_raised` | `tab.inactiveBackground`, `editorGroupHeader.tabsBackground`, `titleBar.activeBackground` | well + 10% fg |
| `bg_float` | `editorWidget.background`, `quickInput.background`, `menu.background` | `bezel_raised` |
| `fg` | `editor.foreground`, `foreground` | Bezel fg |
| `fg_muted` / `fg_subtle` | `descriptionForeground` / `tab.inactiveForeground` | fg + 25% / 38% well |
| `fg_chrome` | `sideBar.foreground`, `sideBarTitle.foreground` | `fg_muted` |
| `border` / `border_strong` | `panel.border`, `editorGroup.border`, `sideBar.border`, `widget.border` / `input.border`, `contrastBorder` | well + 15% / 45% fg |
| `accent` | `focusBorder`, `button.background`, `activityBarBadge.background`, `textLink.foreground` | ANSI blue |
| `danger`, `warn`, `ok`, `info` | `errorForeground`, `editorWarning.foreground`, `gitDecoration.addedResourceForeground`, `editorInfo.foreground` (and fallbacks) | ANSI red, yellow, green, blue |
| terminal `background`, `foreground` | `terminal.background`, `terminal.foreground` | well, fg |
| terminal `cursor`, `selection` | `terminalCursor.foreground`, `terminal.selectionBackground` (or the editor ones) | fg, well + 25% accent |
| terminal `ansi` | `terminal.ansiBlack` ... `terminal.ansiBrightWhite` | Bezel palette |

Kelta does not fix contrast: the card preview shows what you get.

## The TOML format

A theme is a partial set of `#rrggbb` overrides on top of the Bezel palette of its `base`. Tokens you leave out
keep Bezel's value; `ansi` takes all 16 colours (black to white, then the bright ones) or none.

```toml
[app]
dark_theme = "solarized"

[themes.solarized]
name = "Solarized"
base = "dark"

[themes.solarized.ui]
well = "#002b36"
fg = "#93a1a1"
accent = "#268bd2"

[themes.solarized.terminal]
cursor = "#93a1a1"
ansi = ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"]
```

UI tokens: `bezel`, `bezel_raised`, `well`, `bg_float`, `fg`, `fg_muted`, `fg_subtle`, `fg_chrome`, `border`,
`border_strong`, `danger`, `warn`, `ok`, `info`, `accent`. Terminal: `background`, `foreground`, `cursor`,
`selection`, `ansi`.
