<script lang="ts">
  // Settings → Sources → tracker: the ordered steps of a source's tickets (`[sources.tracker] pipeline`, #464).
  // A source without steps shows the effective default read only; Customize writes it to the source.
  import { msg, t } from '$lib/i18n/index.svelte';

  import type { Column, PipelineEvent, StatusCategory, StepDef, TransitionTarget, Trigger } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { settings } from '$lib/stores';
  import { Button, IconButton, Select, TextInput, Toggle } from '$lib/ui';

  import TemplateTools from './fields/TemplateTools.svelte';
  import SettingsGroup from './layout/SettingsGroup.svelte';
  import { uniqueId } from './lib/accounts';
  import { effectivePipeline } from './lib/pipeline';
  import { editSource } from './lib/sources.svelte';

  let { sourceId }: { sourceId: string } = $props();

  const source = $derived(settings.value()?.sources.find((s) => s.id === sourceId) ?? null);
  const custom = $derived(!!source?.tracker.pipeline?.length);
  const work = () => settings.value()!.work;
  const steps = $derived(source ? effectivePipeline(source.tracker, work()) : []);

  const EVENTS: [PipelineEvent, string][] = [
    ['work_started', msg('Work started')],
    ['pr_opened', msg('PR opened')],
    ['pr_ready', msg('PR ready')],
    ['review_requested', msg('Review requested')],
    ['changes_requested', msg('Changes requested')],
    ['approved', msg('Approved')],
    ['merged', msg('Merged')],
  ];
  const CATEGORIES: [StatusCategory, string][] = [
    ['todo', msg('Todo')],
    ['in_progress', msg('In progress')],
    ['in_review', msg('In review')],
    ['done', msg('Done')],
  ];

  // The tracker's statuses; a failed load leaves only the categories.
  let columns = $state<Column[]>([]);
  $effect(() => {
    ipc
      .trackerColumns({ source_id: sourceId })
      .then((c) => (columns = c))
      .catch(() => (columns = []));
  });

  const statusOf = (d: StepDef) =>
    d.enter.find((x): x is Exclude<Trigger, PipelineEvent> => typeof x !== 'string')?.status ?? null;
  const enc = (x: TransitionTarget | null) =>
    !x ? '' : 'category' in x ? `category:${x.category}` : `name:${x.name}`;
  const dec = (v: string): TransitionTarget | null => {
    const i = v.indexOf(':');
    const [k, rest] = [v.slice(0, i), v.slice(i + 1)];
    return k === 'category' ? { category: rest as StatusCategory } : k === 'name' ? { name: rest } : null;
  };
  // The board's statuses plus the ones the steps already name (so a Select can show them).
  const names = $derived([
    ...new Set([
      ...columns.flatMap((c) => c.match_names),
      ...steps.flatMap((d) =>
        [statusOf(d), d.transition_to].flatMap((x) => (x && 'name' in x ? [x.name] : [])),
      ),
    ]),
  ]);
  const targets = $derived([
    { value: '', label: t('None') },
    ...CATEGORIES.map(([c, l]) => ({
      value: `category:${c}`,
      label: t('Any {category} status', { category: t(l) }),
    })),
    ...names.map((n) => ({ value: `name:${n}`, label: n })),
  ]);

  const save = (f: (st: StepDef[]) => StepDef[]) =>
    editSource(sourceId, (s) => ({ ...s, tracker: { ...s.tracker, pipeline: f(s.tracker.pipeline ?? []) } }));
  const edit = (i: number, f: (d: StepDef) => StepDef) =>
    save((st) => st.map((d, j) => (j === i ? f(d) : d)));
  const customize = () =>
    editSource(sourceId, (s) => ({
      ...s,
      tracker: { ...s.tracker, pipeline: effectivePipeline(s.tracker, work()) },
    }));
  const add = () =>
    save((st) => [
      ...st,
      {
        id: uniqueId(
          'step',
          st.map((d) => d.id),
        ),
        label: '',
        enter: [],
        transition_to: null,
        comment: null,
      },
    ]);
  function move(i: number, delta: number): void {
    void save((st) => {
      const next = [...st];
      const [d] = next.splice(i, 1);
      if (d) next.splice(i + delta, 0, d);
      return next;
    });
  }
  const setEvent = (i: number, ev: PipelineEvent, on: boolean) =>
    edit(i, (d) => {
      const rest = d.enter.filter((x) => x !== ev);
      return { ...d, enter: on ? [...rest, ev] : rest };
    });
  // shortcut: one status trigger per step here (picking replaces any others the TOML lists), a list picker if asked.
  const setStatus = (i: number, v: string) =>
    edit(i, (d) => {
      const status = dec(v);
      return {
        ...d,
        enter: [...d.enter.filter((x) => typeof x === 'string'), ...(status ? [{ status }] : [])],
      };
    });
</script>

{#if source}
  <SettingsGroup
    heading={t('Steps')}
    description={t(
      'A step is entered on its events or when the ticket reaches its status, then can move the ticket and post a comment. A step with neither is entered by hand.',
    )}
  >
    {#snippet actions()}
      {#if custom}
        <Button size="sm" icon="plus" onclick={() => void add()}>{t('Add step')}</Button>
      {:else}
        <Button size="sm" onclick={() => void customize()}>{t('Customize')}</Button>
      {/if}
    {/snippet}
    {#if !custom}
      <p class="muted">
        {t('These are the default steps. Customize copies them to this source to edit them.')}
      </p>
    {/if}
    <fieldset class="steps" disabled={!custom}>
      <legend class="k-visually-hidden">{t('Steps')}</legend>
      <ol>
        {#each steps as d, i (i)}
          {@const n = i + 1}
          <li data-testid="pipeline-step">
            <div class="head">
              <TextInput
                label={t('Label of step {n}', { n })}
                value={d.label}
                onchange={(e) => {
                  const label = (e.currentTarget as HTMLInputElement).value.trim();
                  if (label !== d.label) void edit(i, (x) => ({ ...x, label }));
                }}
              />
              <span class="actions">
                <IconButton
                  icon="arrow-up"
                  label={t('Move step {n} up', { n })}
                  size="sm"
                  disabled={i === 0}
                  onclick={() => move(i, -1)}
                />
                <IconButton
                  icon="arrow-down"
                  label={t('Move step {n} down', { n })}
                  size="sm"
                  disabled={i === steps.length - 1}
                  onclick={() => move(i, 1)}
                />
                <IconButton
                  icon="trash"
                  label={t('Remove step {n}', { n })}
                  size="sm"
                  onclick={() => void save((st) => st.filter((_, j) => j !== i))}
                />
              </span>
            </div>
            <fieldset class="events">
              <legend>{t('Events that enter step {n}', { n })}</legend>
              {#each EVENTS as [ev, label] (ev)}
                <Toggle
                  label={t(label)}
                  checked={d.enter.includes(ev)}
                  onchange={(on) => void setEvent(i, ev, on)}
                />
              {/each}
            </fieldset>
            <Select
              label={t('Status that enters step {n}', { n })}
              value={enc(statusOf(d))}
              options={targets}
              onchange={(v) => void setStatus(i, v)}
            />
            {#if d.enter.length === 0}<span class="muted">{t('Manual only')}</span>{/if}
            <Select
              label={t('Status set by step {n}', { n })}
              value={enc(d.transition_to)}
              options={targets}
              onchange={(v) => void edit(i, (x) => ({ ...x, transition_to: dec(v) }))}
            />
            <TemplateTools
              id="step-comment-{sourceId}-{i}"
              path="sources.tracker.pipeline.comment"
              value={d.comment ?? ''}
              readonly={!custom}
            >
              <TextInput
                id="step-comment-{sourceId}-{i}"
                label={t('Comment of step {n}', { n })}
                value={d.comment ?? ''}
                onchange={(e) => {
                  const c = (e.currentTarget as HTMLInputElement).value;
                  void edit(i, (x) => ({ ...x, comment: c.trim() ? c : null }));
                }}
              />
            </TemplateTools>
          </li>
        {/each}
      </ol>
    </fieldset>
  </SettingsGroup>
{/if}

<style>
  .steps {
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  ol {
    display: grid;
    gap: var(--k-space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .head {
    display: flex;
    gap: var(--k-space-2);
    align-items: end;
  }

  .events {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-3);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .actions {
    white-space: nowrap;
  }

  .muted {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }
</style>
