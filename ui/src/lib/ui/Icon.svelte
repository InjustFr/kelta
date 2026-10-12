<script lang="ts">
  import { ensureIcon, type IconName } from './icons';

  interface Props {
    name: IconName | (string & {});
    size?: number;
    /** Accessible label; when omitted the icon is decorative (aria-hidden). */
    label?: string;
    class?: string;
  }

  let { name, size = 16, label, class: className = '' }: Props = $props();

  // A Lucide icon outside the startup sprite loads on first use; `<use>` is rebuilt once its symbol is in.
  let loaded = $state(0);
  $effect(() => void ensureIcon(name)?.then(() => loaded++));
</script>

<svg
  class="k-icon {className}"
  width={size}
  height={size}
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
  focusable="false"
>
  {#key loaded}<use href="#i-{name}" />{/key}
</svg>

<style>
  .k-icon {
    display: inline-block;
    flex: none;
    vertical-align: middle;
    fill: none;
    stroke: currentColor;
  }
</style>
