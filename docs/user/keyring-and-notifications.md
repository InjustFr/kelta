# Keyring and notification daemon

## Secrets

Account tokens are references, never plain text:

| Reference | Source |
|---|---|
| `keyring:<name>` | macOS Keychain, Linux Secret Service (service `dev.kelta`) |
| `file:<name>` | Kelta's own encrypted file (`secrets.enc` in the data folder), unlocked with a passphrase once per run |
| `gh-cli` | the token of your `gh` login for the account's host, read on demand and never copied (GitHub) |
| `glab-cli` | the token of your `glab` login for the account's host (GitLab) |
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
`exec` in Sway) and make sure the collection is unlocked. Or skip the keyring and use `file:`, `command:` or
`env:`.

### Encrypted file (no keyring needed)

Pick "Encrypted file (passphrase)" as the token source, then in Settings > Accounts > Secret storage choose
a passphrase (the first time creates the file). Kelta asks for it once per run, at startup, when an account
uses a `file:` reference. There is no recovery: a forgotten passphrase means deleting `secrets.enc` and
setting the tokens again.

### macOS: Keychain

Kelta asks Keychain access the first time it reads a token. Choose "Always Allow".

## Notifications

Kelta sends desktop notifications when Claude is blocked (it needs input, asks a question or replied without
changes), a review is requested, CI fails on your PR, and so on, only when the related pane is not visible or the
window is unfocused and outside quiet hours.

A Claude that is ready for review or finished does not notify by default: it can wait. Turn on
`notifications.claude_done` (Settings > Notifications) to be told anyway; `notifications.claude_blocked` turns off
the blocked ones.

### Focus mode

`⇧⌘F` (`Ctrl+Shift+F` on Linux, or the prefix then `f`) hides the sidebar and the Story panel so only the work in
front of you stays, under its header. While it is on, desktop notifications and background toasts about other work
(ready for review, approvals, checks, merges) wait. A blocked Claude still lights the "needs you" chip and still
notifies, unless you turn off `notifications.focus_allow_blocked`. `Esc` `Esc` outside a terminal (inside one, `Esc`
belongs to the program) or `⇧⌘F` again brings the layout back as it was, and one toast sums up what waited: "While
you were focused: SHOP-12 ready to review, PR approved".

### Linux: notification daemon

Notifications use `org.freedesktop.Notifications`. GNOME and KDE have one. On Sway and Hyprland install one:

```sh
sudo apt install mako-notifier            # or dunst, swaync
```

and start it from your compositor config (`exec-once = mako`). Diagnostics calls `GetServerInformation` and
shows the daemon name; if it fails, no daemon owns the bus name.

### macOS

Allow Kelta in System Settings > Notifications. The Dock badge counts the blocked sessions only (needing input or asking), never the finished ones.
