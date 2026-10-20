// `keys.linux_modifier` (#541): "super" gives Linux the macOS keymap with Super for Cmd. The shell sets it from the settings.
let linuxSuper = $state(false);

export function setLinuxSuper(on: boolean): void {
  linuxSuper = on;
}

/** The macOS keymap: Cmd (Super on Linux) is Kelta's key and never reaches the PTY, Ctrl stays the program's. */
export function macKeymap(platform: 'macos' | 'linux'): boolean {
  return platform === 'macos' || linuxSuper;
}
