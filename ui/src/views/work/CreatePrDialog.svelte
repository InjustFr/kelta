<script lang="ts">
  import type { WorkItem } from '$lib/gen';
  import { workCreatePr } from '$lib/ipc/commands';
  import { toasts, work } from '$lib/stores';
  import { Button, Dialog, Select, TextInput } from '$lib/ui';

  interface Props {
    item: WorkItem;
    onclose: () => void;
  }

  let { item, onclose }: Props = $props();

  type Mode = 'default' | 'draft' | 'ready';
  let title = $state('');
  let body = $state('');
  let mode = $state<Mode>('default');
  let busy = $state(false);

  async function create(): Promise<void> {
    busy = true;
    try {
      const updated = await workCreatePr({
        id: item.id,
        draft: {
          title: title.trim() === '' ? null : title,
          body: body.trim() === '' ? null : body,
          draft: mode === 'default' ? null : mode === 'draft',
        },
      });
      work.upsert(updated);
      toasts.info(updated.pr_url ? `Pull request ready: ${updated.pr_url}` : 'Pull request created');
      onclose();
    } catch (err) {
      toasts.error(err, 'Create PR');
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title="Create pull request" width={520} {onclose}>
  <form
    id="create-pr-form"
    class="form"
    onsubmit={(e) => {
      e.preventDefault();
      void create();
    }}
  >
    <p class="hint">
      Pushes <code>{item.branch}</code> to the remote (in a visible pane, so credential prompts work), then
      opens the PR against <code>{item.base}</code>. Empty fields use the <code>work.pr</code> templates.
    </p>
    <TextInput label="Title" bind:value={title} placeholder="Default: {'{key}: {title}'}" />
    <TextInput label="Description" bind:value={body} multiline rows={5} placeholder="Default: template" />
    <Select
      label="Type"
      bind:value={mode}
      options={[
        { value: 'default', label: 'Use work.pr.draft' },
        { value: 'draft', label: 'Draft' },
        { value: 'ready', label: 'Ready for review' },
      ]}
    />
  </form>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" type="submit" form="create-pr-form" loading={busy}>Create PR</Button>
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .hint {
    margin: 0;
    color: var(--k-fg-muted);
  }
</style>
