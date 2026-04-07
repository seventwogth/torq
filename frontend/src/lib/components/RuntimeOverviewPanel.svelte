<script lang="ts">
  import StatusBadge from './StatusBadge.svelte';
  import type { TorStateDto } from '../torq-api';

  type ActionName = 'start' | 'stop' | 'restart' | 'new_identity';
  type StatusTone = 'success' | 'warning' | 'danger' | 'neutral' | 'muted';

  export let torState: TorStateDto | null = null;
  export let runtimeStatusTone: StatusTone = 'neutral';
  export let runtimeStatusLabel = '';
  export let runtimeFocalMessage = '';
  export let primaryActionTone = 'primary';
  export let primaryActionStateClass = '';
  export let canRunPrimaryAction = false;
  export let pendingAction: ActionName | null = null;
  export let displayedPrimaryAction: ActionName = 'start';
  export let canRestart = false;
  export let canRequestNewIdentity = false;
  export let controlSummaryLabel = 'Pending';
  export let controlSummaryTone: StatusTone = 'neutral';
  export let controlPortNote = '';
  export let bootstrapSourceLabel = 'Pending';
  export let bootstrapSourceTone: StatusTone = 'neutral';
  export let runtimeStateEmptyMessage = '';
  export let snapshotUsesControlBootstrapObservation = false;
  export let onPerformAction: (action: ActionName) => void = () => {};
  export let actionLabel: (action: ActionName) => string = () => '';
</script>

<section class="app-section runtime-focus-panel" aria-label="Tor Process">
  <div class="section-heading">
    <div class="section-heading-copy">
      <p class="section-kicker">Runtime</p>
      <h2>Primary state</h2>
    </div>
    <p>Lifecycle state stays first. Control and observation details remain secondary.</p>
  </div>

  {#if torState}
    <div class="runtime-focus-layout">
      <div class="runtime-focus-primary">
        <div class="runtime-state-stack">
          <span class="runtime-state-label">Current state</span>
          <h3 class={`runtime-state-value tone-${runtimeStatusTone}`}>{runtimeStatusLabel}</h3>
          <p class="runtime-state-copy">{runtimeFocalMessage}</p>
        </div>

        <div class="runtime-control-surface" aria-label="Runtime controls">
          <button
            type="button"
            class={`action-button runtime-primary-action action-button-primary ${primaryActionTone} ${primaryActionStateClass}`}
            disabled={!canRunPrimaryAction || pendingAction !== null}
            aria-busy={pendingAction === displayedPrimaryAction}
            on:click={() => onPerformAction(displayedPrimaryAction)}
          >
            {actionLabel(displayedPrimaryAction)}
          </button>

          <div class="runtime-secondary-actions" aria-label="Secondary runtime controls">
            <button
              type="button"
              class="action-button action-button-secondary"
              disabled={!canRestart || pendingAction !== null}
              aria-busy={pendingAction === 'restart'}
              on:click={() => onPerformAction('restart')}
            >
              {actionLabel('restart')}
            </button>

            <button
              type="button"
              class="action-button action-button-secondary"
              disabled={!canRequestNewIdentity || pendingAction !== null}
              aria-busy={pendingAction === 'new_identity'}
              on:click={() => onPerformAction('new_identity')}
            >
              {actionLabel('new_identity')}
            </button>
          </div>
        </div>
      </div>

      <div class="runtime-metrics">
        <div class="runtime-metric">
          <span class="metric-label">Bootstrap</span>
          <strong class="metric-value metric-value-mono runtime-bootstrap-value">{torState.bootstrap}%</strong>
          <p class="supporting-text">Current progress reported by the runtime state.</p>
        </div>

        <div class="runtime-metric">
          <span class="metric-label">ControlPort</span>
          <StatusBadge label={controlSummaryLabel} tone={controlSummaryTone} />
          <p class="supporting-text">{controlPortNote || 'Waiting for ControlPort state.'}</p>
        </div>

        <div class="runtime-metric">
          <span class="metric-label">Bootstrap source</span>
          <StatusBadge label={bootstrapSourceLabel} tone={bootstrapSourceTone} />
          <p class="supporting-text">
            {snapshotUsesControlBootstrapObservation
              ? 'Using ControlPort bootstrap observation.'
              : 'Falling back to runtime log observation when control-backed updates are unavailable.'}
          </p>
        </div>
      </div>
    </div>
  {:else}
    <p class="empty-state">{runtimeStateEmptyMessage}</p>
  {/if}
</section>
