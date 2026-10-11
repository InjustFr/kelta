<script lang="ts">
  // "Browse…" next to a path field: the system picker (tauri-plugin-dialog), starting at the
  // field's path when it exists, else the home folder. Cancelling leaves the field alone.
  import { homeDir } from '@tauri-apps/api/path';
  import { open } from '@tauri-apps/plugin-dialog';

  import { t } from '$lib/i18n/index.svelte';
  import { fsExists } from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';

  interface Props {
    /** The field's current text. */
    value: string;
    onpick: (path: string) => void;
    /** Pick a file with one of these extensions instead of a folder. */
    extensions?: string[];
    label?: string;
  }

  let { value, onpick, extensions, label }: Props = $props();

  async function browse(): Promise<void> {
    try {
      const home = await homeDir();
      const current = value.trim().replace(/^~(?=\/|$)/, home);
      const [exists] = current ? await fsExists({ paths: [current] }) : [false];
      const picked = await open({
        directory: !extensions,
        defaultPath: exists ? current : home,
        filters: extensions && [{ name: extensions.join(', '), extensions }],
      });
      if (typeof picked === 'string') onpick(picked);
    } catch (err) {
      toasts.error(err, t('Cannot open the system picker'));
    }
  }
</script>

<Button onclick={browse}>{label ?? t('Browse…')}</Button>
