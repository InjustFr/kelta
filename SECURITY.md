# Security policy

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability reporting
(Security tab, "Report a vulnerability") on https://github.com/InjustFr/kelta. Include the version, the
platform, and steps to reproduce. You will get an answer within a few days.

## Supported versions

Only the latest release and the current `main` get fixes while Kelta is at 0.x.

## Model

- The control socket (`<runtime>/ctl.sock`) is mode 0600 in a 0700 directory and checks the peer uid.
- The webview loads only local assets. Plugin and tool pages run in sandboxed iframes without access to Tauri
  IPC, the parent page, or the network, and plugins act only through the permissions you grant.
- Secrets come from the OS keyring, `command:` or `env:` references. Plugins cannot read them.
- Repository configuration that can run commands stays inert until you trust its exact content.

See `docs/ARCHITECTURE.md` section 11 for the full model.
