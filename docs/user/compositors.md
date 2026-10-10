# Compositor bindings and window rules

Kelta registers no global shortcut itself. Bind `kelta-ctl toggle` in your compositor: it hides the window when
it is focused and raises (or recreates) it otherwise. The window class and Wayland app id are `dev.kelta.Kelta`.

`kelta-ctl` is on the PATH with the deb; with the AppImage or a Dock launch use the stable copy
`~/.local/share/kelta/bin/current/kelta-ctl` (macOS: `~/Library/Application Support/dev.kelta.Kelta/bin/current/kelta-ctl`).

## Hyprland

```ini
bind = SUPER, K, exec, kelta-ctl toggle
bind = SUPER SHIFT, K, exec, kelta-ctl palette
bind = SUPER, J, exec, kelta-ctl next
bind = SUPER SHIFT, J, exec, kelta-ctl back
windowrulev2 = workspace 2, class:^(dev.kelta.Kelta)$
windowrulev2 = tile, class:^(dev.kelta.Kelta)$
```

Kelta detects `HYPRLAND_INSTANCE_SIGNATURE` and starts without a title bar.

## Sway

```
bindsym $mod+k exec kelta-ctl toggle
bindsym $mod+Shift+k exec kelta-ctl palette
bindsym $mod+j exec kelta-ctl next
bindsym $mod+Shift+j exec kelta-ctl back
for_window [app_id="dev.kelta.Kelta"] move container to workspace 2
```

Kelta detects `SWAYSOCK` and starts without a title bar. Add `default_border pixel 2` if you want a border.

## GNOME

Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts > add, command `kelta-ctl toggle`, pick a shortcut.
GNOME keeps the native title bar (`decorations = "auto"` resolves to `native`).

## Focus when raising

When a compositor launches `kelta-ctl` from a key binding it may pass an `XDG_ACTIVATION_TOKEN`; the running
Kelta can only use the token it received at its own start, so on strict focus-stealing prevention the window can
be raised without taking focus. Focus it from the compositor (`focus` rule or `exec` followed by a focus
dispatcher) if that happens.

## Custom decorations

`window.decorations = "custom"` removes the system title bar and draws Kelta's own drag region and resize
handles. `none` removes the title bar and draws nothing, which suits tiling compositors.
