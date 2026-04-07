<script lang="ts">
  import StatusBadge from './StatusBadge.svelte';
  import {
    booleanToColor,
    bootstrapSourceToColor,
    controlAvailabilityToColor,
    formatBootstrapSource,
    formatControlPortValue,
  } from '../status';
  import type { TorRuntimeSnapshotDto } from '../torq-api';

  export let snapshot: TorRuntimeSnapshotDto | null = null;
  export let capabilities: Array<{ label: string; value: boolean; statusLabel: string }> = [];
  export let runtimeSnapshotEmptyMessage = '';
  export let controlPortNote = '';
  export let loadErrorMessage = '';
</script>

<section class="app-section dashboard-panel" aria-label="Runtime details">
  <div class="section-heading">
    <div class="section-heading-copy">
      <p class="section-kicker">Details</p>
      <h2>Runtime details</h2>
    </div>
    <p>Current control status, available capabilities, and observation mode from the existing desktop commands.</p>
  </div>

  <div class="dashboard-section-grid">
    <section class="detail-section">
      <div class="detail-section-head">
        <h3>ControlPort</h3>
        <p>Configuration status and reachability.</p>
      </div>

      {#if snapshot}
        <div class="metric-stack">
          <div class="metric">
            <span class="metric-label">Status</span>
            <StatusBadge
              label={formatControlPortValue(snapshot.control.port)}
              tone={controlAvailabilityToColor[snapshot.control.port]}
            />
          </div>

          <div class="metric">
            <span class="metric-label">Bootstrap observation</span>
            <StatusBadge
              label={formatControlPortValue(snapshot.control.bootstrap_observation)}
              tone={controlAvailabilityToColor[snapshot.control.bootstrap_observation]}
            />
          </div>

          <p class="supporting-text">{controlPortNote}</p>
        </div>
      {:else}
        <p class="empty-state">{runtimeSnapshotEmptyMessage}</p>
      {/if}
    </section>

    <section class="detail-section">
      <div class="detail-section-head">
        <h3>Capabilities</h3>
        <p>Feature availability derived from the current snapshot.</p>
      </div>

      {#if snapshot}
        <ul class="capability-list">
          {#each capabilities as capability}
            <li>
              <span class="metric-label">{capability.label}</span>
              <StatusBadge label={capability.statusLabel} tone={booleanToColor(capability.value)} />
            </li>
          {/each}
        </ul>
      {:else}
        <p class="empty-state">{runtimeSnapshotEmptyMessage}</p>
      {/if}
    </section>

    <section class="detail-section">
      <div class="detail-section-head">
        <h3>Observation</h3>
        <p>Current source of bootstrap updates for the UI.</p>
      </div>

      {#if snapshot}
        <div class="metric-stack">
          <div class="metric">
            <span class="metric-label">Bootstrap source</span>
            <StatusBadge label={formatBootstrapSource(snapshot)} tone={bootstrapSourceToColor(snapshot)} />
          </div>

          <div class="metric metric-copy-only">
            <span class="metric-label">Observation path</span>
            <span class="supporting-text">
              {snapshot.uses_control_bootstrap_observation
                ? 'Using ControlPort bootstrap observation.'
                : snapshot.control.bootstrap_observation === 'unconfigured'
                  ? 'ControlPort bootstrap observation is not configured.'
                  : snapshot.tor.status === 'starting' || snapshot.tor.status === 'running'
                    ? 'ControlPort bootstrap observation is unavailable, so the desktop shell is falling back to Tor log output.'
                    : 'Bootstrap observation will appear after Tor starts.'}
            </span>
          </div>
        </div>
      {:else}
        <p class="empty-state">{runtimeSnapshotEmptyMessage}</p>
      {/if}
    </section>
  </div>

  {#if loadErrorMessage}
    <section class="error-panel" aria-live="polite">
      <h2>Backend state unavailable</h2>
      <p>{loadErrorMessage}</p>
    </section>
  {/if}
</section>
