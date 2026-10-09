# tools-pack

A declarative plugin (PLUGINS.md §4.3): two PTY tools, `tools-pack/k9s` and `tools-pack/btop`. Opening a
plugin tool needs `sessions.spawn` and `exec:<program>`; both are requested here and shown in the install
dialog. "Check" runs the `check` argv (exit 0 = installed) and shows `install_hint` otherwise.
