# Keyring and notification daemon

## Secrets

Account tokens are references, never plain text:

| Reference | Source |
|---|---|
| `keyring:<name>` | macOS Keychain, Linux Secret Service (service `dev.kelta`) |
| `command:<argv>` | output of a command, run without a shell, 5 s limit (`command:pass show jira/acme`) |
| `env:<VAR>` | environment variable |

### Linux: Secret Service

GNOME and KDE provide one. On Sway and Hyprland you often have none, and Settings > Diagnostics reports
"Secret Service" as unavailable. Install and start one:

```sh
sudo apt install gnome-keyring            # or KeePassXC with Secret Service integration enabled
gnome-keyring-daemon --start --components=secrets
```

Start it from your session (`exec-once = gnome-keyring-daemon --start --components=secrets` in Hyprland,
`exec` in Sway) and make sure the collection is unlocked. Or skip the keyring and use `command:` / `env:`.

### macOS: Keychain

Kelta asks Keychain access the first time it reads a token. Choose "Always Allow".

## Notifications

Kelta sends desktop notifications when Claude needs input, a review is requested, CI fails on your PR, and so on,
only when the related pane is not visible or the window is unfocused and outside quiet hours.

### Linux: notification daemon

Notifications use `org.freedesktop.Notifications`. GNOME and KDE have one. On Sway and Hyprland install one:

```sh
sudo apt install mako-notifier            # or dunst, swaync
```

and start it from your compositor config (`exec-once = mako`). Diagnostics calls `GetServerInformation` and
shows the daemon name; if it fails, no daemon owns the bus name.

### macOS

Allow Kelta in System Settings > Notifications. The Dock badge shows how many sessions need input.
