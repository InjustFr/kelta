<script lang="ts">
  import type { TemplateCtx } from '$lib/gen';
  import { Button, Menu } from '$lib/ui';

  import { pluginActions, runPluginAction } from './actions';

  // Enabled plugins' ticket/review actions (PLUGINS §4), after the built-ins and with no key: one
  // ghost button, or a `Plugins ▾` menu when there are several; nothing when there is none.
  interface Props {
    kind: 'ticket' | 'review';
    ctx: Partial<TemplateCtx>;
    size?: 'sm' | 'md';
  }

  let { kind, ctx, size = 'sm' }: Props = $props();

  const actions = $derived(pluginActions(kind));
  let running = $state(false);
  let menu = $state<{ x: number; y: number } | null>(null);
  let anchor = $state<HTMLElement>();

  async function run(commandId: string): Promise<void> {
    running = true;
    try {
      await runPluginAction(commandId, ctx);
    } finally {
      running = false;
    }
  }

  function open(): void {
    const r = anchor?.getBoundingClientRect();
    menu = { x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2 };
  }
</script>

{#if actions.length === 1}
  <Button
    {size}
    variant="ghost"
    loading={running}
    data-plugin-action={actions[0]?.command_id}
    onclick={() => void run(actions[0]?.command_id ?? '')}
  >
    {actions[0]?.title}
  </Button>
{:else if actions.length > 1}
  <span bind:this={anchor}>
    <Button
      {size}
      variant="ghost"
      loading={running}
      aria-haspopup="menu"
      aria-expanded={menu !== null}
      onclick={open}
    >
      Plugins ▾
    </Button>
  </span>
  {#if menu}
    <Menu
      items={actions.map((a) => ({ id: a.command_id, label: a.title, detail: a.plugin }))}
      x={menu.x}
      y={menu.y}
      label="Plugin actions"
      onselect={(id) => void run(id)}
      onclose={() => {
        menu = null;
        anchor?.querySelector('button')?.focus();
      }}
    />
  {/if}
{/if}
