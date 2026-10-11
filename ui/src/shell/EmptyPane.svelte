<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { dispatch } from '$lib/actions';
  import { Button, EmptyState } from '$lib/ui';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();
</script>

<EmptyState
  icon="square"
  title={t('This pane is empty')}
  body={t('Start a session here, or close the pane to give the space back.')}
>
  {#snippet actions()}
    <Button variant="primary" icon="plus" onclick={() => dispatch('session.new')}
      >{t('Start a session here')}</Button
    >
    <Button icon="x" onclick={onclose}>{t('Close pane')}</Button>
  {/snippet}
</EmptyState>
