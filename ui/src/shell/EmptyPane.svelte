<script lang="ts">
  import { dispatch } from '$lib/actions';
  import { Button, EmptyState } from '$lib/ui';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();
</script>

<EmptyState icon="square" title="Empty pane" body="Start a session here or close the pane.">
  {#snippet actions()}
    <Button variant="primary" icon="plus" onclick={() => dispatch('session.new')}>New session</Button>
    <Button icon="x" onclick={onclose}>Close pane</Button>
  {/snippet}
</EmptyState>
