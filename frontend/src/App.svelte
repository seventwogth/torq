<script lang="ts">
  import { onMount } from 'svelte';
  import Card from './lib/components/Card.svelte';
  import StatusBadge from './lib/components/StatusBadge.svelte';
  import {
    booleanToColor,
    bootstrapSourceToColor,
    controlAvailabilityToColor,
    formatBooleanStatus,
    formatBootstrapSource,
    formatControlPortValue,
    formatRuntimeStatus,
    statusToColor,
  } from './lib/status';
  import {
    fetchTorRuntimeSnapshot,
    fetchTorState,
    type TorRuntimeSnapshotDto,
    type TorStateDto,
  } from './lib/torq-api';

  let backendConnected = false;
  let state: TorStateDto | null = null;
  let snapshot: TorRuntimeSnapshotDto | null = null;
  let errorMessage = '';

  onMount(async () => {
    try {
      const [nextState, nextSnapshot] = await Promise.all([
        fetchTorState(),
        fetchTorRuntimeSnapshot(),
      ]);

      state = nextState;
      snapshot = nextSnapshot;
      backendConnected = true;
      errorMessage = '';
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  });

  $: torState = state ?? snapshot?.tor ?? null;
  $: capabilities = snapshot
    ? [
        { label: 'Control configured', value: snapshot.control_configured },
        { label: 'Control available', value: snapshot.control_available },
        { label: 'New identity available', value: snapshot.new_identity_available },
        {
          label: 'Bootstrap observation available',
          value: snapshot.bootstrap_observation_available,
        },
      ]
    : [];
</script>

<svelte:head>
  <title>torq</title>
</svelte:head>

<main class="shell">
  <header class="hero">
    <div class="hero-copy">
      <p class="eyebrow">Status Panel</p>
      <h1>torq</h1>
      <p class="hero-text">
        Read-only runtime overview for Tor process state, ControlPort availability, and current
        observation capabilities.
      </p>
    </div>

    <StatusBadge
      label={backendConnected ? 'backend connected' : 'backend disconnected'}
      tone={backendConnected ? 'success' : 'danger'}
    />
  </header>

  <section class="status-panel" aria-label="Tor runtime status panel">
    <div class="section-heading">
      <h2>Status Panel</h2>
      <p>Rendered from the existing `tor_state` and `tor_runtime_snapshot` backend commands.</p>
    </div>

    <div class="card-grid">
      <Card title="Tor Process" subtitle="Lifecycle and bootstrap progress from the current runtime state.">
        {#if torState}
          <div class="metric-stack">
            <div class="metric">
              <span class="metric-label">Status</span>
              <StatusBadge
                label={formatRuntimeStatus(torState.status)}
                tone={statusToColor[torState.status]}
              />
            </div>

            <div class="metric">
              <span class="metric-label">Bootstrap</span>
              <strong class="metric-value">{torState.bootstrap}%</strong>
            </div>
          </div>
        {:else}
          <p class="empty-state">Waiting for backend state.</p>
        {/if}
      </Card>

      <Card title="ControlPort" subtitle="ControlPort configuration and current availability.">
        {#if snapshot}
          <div class="metric-stack">
            <div class="metric">
              <span class="metric-label">Port</span>
              <StatusBadge
                label={formatControlPortValue(snapshot.control.port)}
                tone={controlAvailabilityToColor[snapshot.control.port]}
              />
            </div>

            <div class="metric">
              <span class="metric-label">Control available</span>
              <StatusBadge
                label={formatBooleanStatus(snapshot.control_available)}
                tone={booleanToColor(snapshot.control_available)}
              />
            </div>
          </div>
        {:else}
          <p class="empty-state">Waiting for runtime snapshot.</p>
        {/if}
      </Card>

      <Card title="Capabilities" subtitle="Feature flags derived from the current snapshot.">
        {#if snapshot}
          <ul class="capability-list">
            {#each capabilities as capability}
              <li>
                <span class="metric-label">{capability.label}</span>
                <StatusBadge
                  label={formatBooleanStatus(capability.value)}
                  tone={booleanToColor(capability.value)}
                />
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty-state">Waiting for runtime snapshot.</p>
        {/if}
      </Card>

      <Card title="Runtime Mode" subtitle="Current source of bootstrap observation for the UI.">
        {#if snapshot}
          <div class="metric-stack">
            <div class="metric">
              <span class="metric-label">Bootstrap source</span>
              <StatusBadge
                label={formatBootstrapSource(snapshot)}
                tone={bootstrapSourceToColor(snapshot)}
              />
            </div>

            <div class="metric">
              <span class="metric-label">Observation path</span>
              <span class="supporting-text">
                {snapshot.uses_control_bootstrap_observation
                  ? 'Using ControlPort bootstrap observation.'
                  : snapshot.tor.status === 'starting' || snapshot.tor.status === 'running'
                    ? 'Falling back to log-based runtime observation.'
                    : 'Bootstrap observation is currently unavailable.'}
              </span>
            </div>
          </div>
        {:else}
          <p class="empty-state">Waiting for runtime snapshot.</p>
        {/if}
      </Card>
    </div>
  </section>

  {#if errorMessage}
    <section class="error-panel" aria-live="polite">
      <h2>Load error</h2>
      <p>{errorMessage}</p>
    </section>
  {/if}
</main>
