<script lang="ts">
  import type { IconName } from './icons';

  interface Props {
    name: IconName | (string & {});
    size?: number;
    /** Accessible label; when omitted the icon is decorative (aria-hidden). */
    label?: string;
    class?: string;
  }

  let { name, size = 16, label, class: className = '' }: Props = $props();
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
  <use href="#i-{name}" />
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
