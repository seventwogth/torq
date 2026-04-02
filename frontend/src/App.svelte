<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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
    restartTor,
    requestNewIdentity,
    TOR_ACTIVITY_EVENT,
    TOR_RUNTIME_SNAPSHOT_EVENT,
    TOR_STATE_EVENT,
    startTor,
    stopTor,
    type ActivityTone,
    type TorActivityEventDto,
    type TorRuntimeSnapshotDto,
    type TorStateDto,
  } from './lib/torq-api';

  let backendConnected = false;
  let state: TorStateDto | null = null;
  let snapshot: TorRuntimeSnapshotDto | null = null;
  let loadErrorMessage = '';
  let actionErrorMessage = '';
  let eventErrorMessage = '';
  let activitySubscriptionError = '';
  let pendingAction: ActionName | null = null;
  let unsubscribeStateEvent: UnlistenFn | null = null;
  let unsubscribeSnapshotEvent: UnlistenFn | null = null;
  let unsubscribeActivityEvent: UnlistenFn | null = null;
  let activityEntries: ActivityEntry[] = [];
  let activitySequence = 0;

  const ACTIVITY_HISTORY_LIMIT = 12;

  type ActionName = 'start' | 'stop' | 'restart' | 'new_identity';

  type ActivityCoalesceKey = 'bootstrap';

  interface ActivityEntry {
    id: string;
    timestamp: number;
    tone: ActivityTone;
    title: string;
    details?: string;
    coalesceKey?: ActivityCoalesceKey;
  }

  async function refreshRuntimeView() {
    const [nextState, nextSnapshot] = await Promise.all([
      fetchTorState(),
      fetchTorRuntimeSnapshot(),
    ]);

    state = nextState;
    snapshot = nextSnapshot;
    backendConnected = true;
  }

  onMount(() => {
    let active = true;

    const initializeRuntimeView = async () => {
      try {
        unsubscribeStateEvent = await listen<TorStateDto>(TOR_STATE_EVENT, (event) => {
          state = event.payload;
          backendConnected = true;
          loadErrorMessage = '';
        });
      } catch (error) {
        if (active) {
          eventErrorMessage = error instanceof Error ? error.message : String(error);
        }
      }

      try {
        unsubscribeSnapshotEvent = await listen<TorRuntimeSnapshotDto>(
          TOR_RUNTIME_SNAPSHOT_EVENT,
          (event) => {
            snapshot = event.payload;
            backendConnected = true;
            loadErrorMessage = '';
          },
        );
      } catch (error) {
        if (active) {
          eventErrorMessage = error instanceof Error ? error.message : String(error);
        }
      }

      try {
        unsubscribeActivityEvent = await listen<TorActivityEventDto>(
          TOR_ACTIVITY_EVENT,
          (event) => {
            const activityEntry = normalizeActivityEntry(event.payload);

            if (activityEntry) {
              appendActivityEntry(activityEntry);
            }
          },
        );
      } catch (error) {
        if (active) {
          activitySubscriptionError = 'Activity feed unavailable.';
        }
      }

      if (!active) {
        unsubscribeStateEvent?.();
        unsubscribeSnapshotEvent?.();
        unsubscribeActivityEvent?.();
        unsubscribeStateEvent = null;
        unsubscribeSnapshotEvent = null;
        unsubscribeActivityEvent = null;
        return;
      }

      try {
        await refreshRuntimeView();
        loadErrorMessage = '';
      } catch (error) {
        if (active) {
          loadErrorMessage = error instanceof Error ? error.message : String(error);
          backendConnected = false;
        }
      }
    };

    void initializeRuntimeView();

    return () => {
      active = false;
      unsubscribeStateEvent?.();
      unsubscribeSnapshotEvent?.();
      unsubscribeActivityEvent?.();
      unsubscribeStateEvent = null;
      unsubscribeSnapshotEvent = null;
      unsubscribeActivityEvent = null;
    };
  });

  $: torState = state ?? snapshot?.tor ?? null;
  $: hasRuntimeData = torState !== null && snapshot !== null;
  $: isTorActive = torState ? torState.status === 'starting' || torState.status === 'running' : false;
  $: canStart = hasRuntimeData && !isTorActive;
  $: canStop = isTorActive;
  $: canRestart = isTorActive;
  $: canRequestNewIdentity = hasRuntimeData && snapshot?.new_identity_available === true;
  $: primaryAction = (isTorActive ? 'stop' : 'start') as ActionName;
  $: primaryActionTone = primaryAction === 'start' ? 'primary' : 'danger';
  $: canRunPrimaryAction = primaryAction === 'start' ? canStart : canStop;
  $: capabilities = snapshot
    ? [
        {
          label: 'Control configured',
          value: snapshot.control_configured,
          statusLabel: snapshot.control_configured ? 'Configured' : 'Not configured',
        },
        {
          label: 'Control available',
          value: snapshot.control_available,
          statusLabel: snapshot.control_available ? 'Available' : 'Unavailable',
        },
        {
          label: 'New identity available',
          value: snapshot.new_identity_available,
          statusLabel: snapshot.new_identity_available ? 'Available' : 'Unavailable',
        },
        {
          label: 'Bootstrap observation available',
          value: snapshot.bootstrap_observation_available,
          statusLabel: snapshot.bootstrap_observation_available ? 'Available' : 'Unavailable',
        },
      ]
    : [];

  function nextActivityId() {
    activitySequence += 1;
    return `${Date.now()}-${activitySequence}`;
  }

  function normalizeTone(value: unknown): ActivityTone {
    return value === 'success' ||
      value === 'warning' ||
      value === 'danger' ||
      value === 'neutral' ||
      value === 'info'
      ? value
      : 'neutral';
  }

  function humanizeKind(kind: string) {
    return kind
      .replace(/[_-]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .replace(/^\w/, (first: string) => first.toUpperCase());
  }

  function formatActivityTime(timestamp: number) {
    return new Intl.DateTimeFormat([], {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    }).format(new Date(timestamp));
  }

  function extractString(value: unknown) {
    return typeof value === 'string' && value.trim() ? value.trim() : undefined;
  }

  function extractNumber(value: unknown) {
    return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
  }

  function parseTimestamp(value: unknown) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value;
    }

    if (typeof value === 'string') {
      const parsed = Date.parse(value);
      return Number.isFinite(parsed) ? parsed : undefined;
    }

    return undefined;
  }

  function parseActivityTone(kind: string, payload: Record<string, unknown>): ActivityTone {
    const explicitTone = normalizeTone(payload.tone);

    if (payload.tone) {
      return explicitTone;
    }

    switch (kind) {
      case 'started':
      case 'identityrenewed':
      case 'controlavailable':
        return 'success';
      case 'stopped':
      case 'bootstrap':
        return 'warning';
      case 'startfailed':
      case 'crashed':
      case 'error':
        return 'danger';
      case 'warning':
      case 'controlunavailable':
        return 'warning';
      default:
        return 'neutral';
    }
  }

  function activityFromKind(kind: string, payload: Record<string, unknown>) {
    switch (kind) {
      case 'started':
        return { title: 'Tor started', tone: 'success' as ActivityTone };
      case 'stopped':
        return { title: 'Tor stopped', tone: 'neutral' as ActivityTone };
      case 'identityrenewed':
        return { title: 'New identity renewed', tone: 'success' as ActivityTone };
      case 'startfailed':
        return {
          title: 'Tor failed to start',
          tone: 'danger' as ActivityTone,
          details: extractString(payload.message) ?? extractString(payload.details),
        };
      case 'crashed':
        return {
          title: 'Tor crashed',
          tone: 'danger' as ActivityTone,
          details: extractString(payload.message) ?? extractString(payload.details),
        };
      case 'warning':
        return {
          title: 'Runtime warning',
          tone: 'warning' as ActivityTone,
          details: extractString(payload.message) ?? extractString(payload.details),
        };
      case 'error':
        return {
          title: 'Runtime error',
          tone: 'danger' as ActivityTone,
          details: extractString(payload.message) ?? extractString(payload.details),
        };
      case 'bootstrap': {
        const progress =
          extractNumber(payload.bootstrap) ?? extractNumber(payload.progress) ?? undefined;

        return {
          title: `Bootstrap${typeof progress === 'number' ? `: ${progress}%` : ''}`,
          tone: progress === 100 ? ('success' as ActivityTone) : ('warning' as ActivityTone),
          details: extractString(payload.details) ?? extractString(payload.message),
          coalesceKey: 'bootstrap' as ActivityCoalesceKey,
        };
      }
      case 'controlavailabilitychanged': {
        const availability = extractString(payload.availability) ?? 'unknown';

        return {
          title:
            availability === 'available'
              ? 'ControlPort became available'
              : availability === 'unconfigured'
                ? 'ControlPort unconfigured'
                : 'ControlPort unavailable',
          tone:
            availability === 'available'
              ? ('success' as ActivityTone)
              : availability === 'unconfigured'
                ? ('neutral' as ActivityTone)
                : ('warning' as ActivityTone),
          details: availability,
        };
      }
      case 'bootstrapobservationavailabilitychanged': {
        const availability = extractString(payload.availability) ?? 'unknown';

        return {
          title:
            availability === 'available'
              ? 'Bootstrap observation became available'
              : availability === 'unconfigured'
                ? 'Bootstrap observation unconfigured'
                : 'Bootstrap observation unavailable',
          tone:
            availability === 'available'
              ? ('success' as ActivityTone)
              : availability === 'unconfigured'
                ? ('neutral' as ActivityTone)
                : ('warning' as ActivityTone),
          details: availability,
        };
      }
      default:
        return null;
    }
  }

  function normalizeActivityEntry(payload: TorActivityEventDto | string | null | undefined) {
    if (payload == null) {
      return null;
    }

    if (typeof payload === 'string') {
      return {
        id: nextActivityId(),
        timestamp: Date.now(),
        tone: 'neutral' as ActivityTone,
        title: payload,
      };
    }

    if (typeof payload !== 'object') {
      return null;
    }

    const record = payload as Record<string, unknown>;
    const rawKind = extractString(record.kind) ?? extractString(record.type) ?? 'unknown';
    const kind = rawKind.toLowerCase().replace(/[-_\s]+/g, '');
    const inferred = activityFromKind(kind, record);
    const title = extractString(record.title) ?? inferred?.title ?? humanizeKind(rawKind);
    const timestamp =
      parseTimestamp(record.timestamp_ms) ?? parseTimestamp(record.timestamp) ?? Date.now();
    const tone =
      typeof record.tone === 'string'
        ? normalizeTone(record.tone)
        : inferred?.tone ?? parseActivityTone(kind, record);
    const details = extractString(record.details) ?? inferred?.details;
    const isBootstrapLike =
      kind === 'bootstrap' ||
      extractNumber(record.progress) !== undefined ||
      extractNumber(record.bootstrap) !== undefined ||
      title.toLowerCase().startsWith('bootstrap');

    return {
      id: nextActivityId(),
      timestamp,
      tone,
      title,
      details,
      coalesceKey:
        extractString(record.coalesce_key) === 'bootstrap'
          ? 'bootstrap'
          : inferred?.coalesceKey ?? (isBootstrapLike ? 'bootstrap' : undefined),
    };
  }

  function appendActivityEntry(entry: ActivityEntry) {
    const baseEntries = entry.coalesceKey
      ? activityEntries.filter((current) => current.coalesceKey !== entry.coalesceKey)
      : activityEntries;

    activityEntries = [entry, ...baseEntries].slice(0, ACTIVITY_HISTORY_LIMIT);
  }

  async function performAction(action: ActionName) {
    if (pendingAction) {
      return;
    }

    pendingAction = action;
    actionErrorMessage = '';

    try {
      if (action === 'start') {
        await startTor();
      } else if (action === 'stop') {
        await stopTor();
      } else if (action === 'restart') {
        await restartTor();
      } else {
        await requestNewIdentity();
      }
    } catch (error) {
      actionErrorMessage = error instanceof Error ? error.message : String(error);
      pendingAction = null;
      return;
    }

    if (eventErrorMessage) {
      try {
        await refreshRuntimeView();
        loadErrorMessage = '';
      } catch (error) {
        loadErrorMessage = error instanceof Error ? error.message : String(error);
        backendConnected = false;
      }
    }

    pendingAction = null;
  }

  function actionLabel(action: ActionName) {
    const labels: Record<ActionName, string> = {
      start: 'Start',
      stop: 'Stop',
      restart: 'Restart',
      new_identity: 'New Identity',
    };

    const pendingLabels: Record<ActionName, string> = {
      start: 'Starting...',
      stop: 'Stopping...',
      restart: 'Restarting...',
      new_identity: 'Requesting...',
    };

    return pendingAction === action ? pendingLabels[action] : labels[action];
  }
</script>

<svelte:head>
  <title>torq</title>
</svelte:head>

<main class="shell">
  <header class="hero">
    <div class="hero-main">
      <div class="hero-copy">
        <p class="eyebrow">Status Panel</p>
        <h1>torq</h1>
        <div class="hero-meta">
          <StatusBadge
            label={backendConnected ? 'backend connected' : 'backend disconnected'}
            tone={backendConnected ? 'success' : 'danger'}
          />
          <p class="hero-text">
            Read-only runtime overview for Tor process state, ControlPort availability, and current
            observation capabilities.
          </p>
        </div>
      </div>

      <div class="control-bar-wrap">
        <div class="control-bar" aria-label="Runtime controls">
          <div class="primary-actions">
            <button
              type="button"
              class={`action-button action-button-primary ${primaryActionTone}`}
              disabled={!canRunPrimaryAction || pendingAction !== null}
              aria-busy={pendingAction === primaryAction}
              on:click={() => performAction(primaryAction)}
            >
              {actionLabel(primaryAction)}
            </button>
          </div>

          <div class="secondary-actions">
            <button
              type="button"
              class="action-button action-button-secondary"
              disabled={!canRestart || pendingAction !== null}
              aria-busy={pendingAction === 'restart'}
              on:click={() => performAction('restart')}
            >
              {actionLabel('restart')}
            </button>

            <button
              type="button"
              class="action-button action-button-secondary"
              disabled={!canRequestNewIdentity || pendingAction !== null}
              aria-busy={pendingAction === 'new_identity'}
              on:click={() => performAction('new_identity')}
            >
              {actionLabel('new_identity')}
            </button>
          </div>
        </div>

        {#if actionErrorMessage}
          <p class="action-error" aria-live="polite">{actionErrorMessage}</p>
        {/if}

        {#if eventErrorMessage}
          <p class="action-error" aria-live="polite">{eventErrorMessage}</p>
        {/if}
      </div>
    </div>
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
                  label={capability.statusLabel}
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

  <section class="activity-panel" aria-label="Tor runtime activity">
    <Card title="Activity" subtitle="Recent runtime events.">
      {#if activitySubscriptionError}
        <p class="activity-note activity-note-error">{activitySubscriptionError}</p>
      {/if}

      {#if activityEntries.length}
        <ul class="activity-list">
          {#each activityEntries as entry}
            <li class={`activity-item tone-${entry.tone}`}>
              <span class="activity-marker" aria-hidden="true"></span>
              <div class="activity-copy">
                <div class="activity-headline">
                  <strong>{entry.title}</strong>
                  <time>{formatActivityTime(entry.timestamp)}</time>
                </div>
                {#if entry.details}
                  <p>{entry.details}</p>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="empty-state">Waiting for runtime activity.</p>
      {/if}
    </Card>
  </section>

  {#if loadErrorMessage}
    <section class="error-panel" aria-live="polite">
      <h2>Load error</h2>
      <p>{loadErrorMessage}</p>
    </section>
  {/if}
</main>
