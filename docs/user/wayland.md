# Wayland notes

- App id and window class are `dev.kelta.Kelta`; use it in window rules.
- GTK3 renders at integer scale factors. With fractional scaling (for example 1.5) the compositor scales the
  window and text can look soft. Use an integer scale or a font size increase in Settings > Terminal.
- No in-app global shortcuts: bind `kelta-ctl toggle` in the compositor ([bindings](compositors.md)).
- Input methods: Kelta never sets `GTK_IM_MODULE`. Use the one your session provides (fcitx5, ibus).
- Clipboard: selecting text sets PRIMARY when `terminal.primary_selection` is on; Wayland data-control is used
  for the clipboard.
- Tiling compositors: `window.decorations = "auto"` removes the title bar on Hyprland and Sway.
- Blank window: see [graphics](graphics.md).
