# Graphics and NVIDIA troubleshooting (Linux)

Kelta uses WebKitGTK. Some driver stacks render a blank or flickering window. Kelta applies workarounds before
the webview starts; they need a restart to change.

## What happens automatically

- NVIDIA proprietary driver detected (`/proc/driver/nvidia/version` or `/sys/module/nvidia_drm`): the DMABUF
  renderer is disabled (`WEBKIT_DISABLE_DMABUF_RENDERER=1`) and explicit sync is turned off
  (`__NV_DISABLE_EXPLICIT_SYNC=1`). Turn this off with `linux.graphics.auto_nvidia = false`.
- A launch that never finished (the guard file `<data>/launch-guard` is still there at the next start) restarts
  once with the safe profile. If the second start fails too, Kelta prints a hint to run `kelta --safe-graphics`.
- Variables you already set in your environment are never overwritten.

## Safe graphics

```sh
kelta --safe-graphics
```

or the **Safe graphics** action of the desktop entry (right-click the launcher icon). The safe profile sets all
workarounds: DMABUF off, compositing mode off, explicit sync off and the X11 backend (`GDK_BACKEND=x11`).

## Settings

`[linux.graphics]` in `config.toml` (Settings > Linux graphics):

| Key | Effect |
|---|---|
| `profile = "auto" \| "default" \| "safe"` | `safe` applies every workaround |
| `auto_nvidia` | NVIDIA workaround as above |
| `disable_dmabuf` | `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| `disable_compositing` | `WEBKIT_DISABLE_COMPOSITING_MODE=1` |
| `nvidia_disable_explicit_sync` | `__NV_DISABLE_EXPLICIT_SYNC=1` |
| `gdk_backend = "auto" \| "wayland" \| "x11"` | `GDK_BACKEND` |

Settings > Diagnostics > Graphics shows the profile and the variables Kelta set.

## Memory with NVIDIA

The memory budget (250 MB for the reference workload) is for Mesa. With the NVIDIA proprietary driver the
number is informational and up to 30 % higher.

## Still blank?

1. Run `kelta --safe-graphics` from a terminal and read the output.
2. Check `webkit2gtk-4.1` is 2.40 or newer (Diagnostics shows it).
3. Try `linux.graphics.gdk_backend = "x11"` on Wayland, or `"wayland"` on X11.
