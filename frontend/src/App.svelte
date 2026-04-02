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
  const DEFAULT_ACTIVITY_TITLE = 'Runtime event';

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
          eventErrorMessage = formatUiError('Live runtime updates are unavailable.', error);
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
          eventErrorMessage = formatUiError('Live runtime updates are unavailable.', error);
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
          activitySubscriptionError = 'Activity feed is unavailable.';
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
          loadErrorMessage = formatUiError('Unable to load backend state.', error);
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

  function formatUiError(prefix: string, error: unknown) {
    const message = error instanceof Error ? error.message.trim() : String(error).trim();
    return message ? `${prefix} ${message}` : prefix;
  }

  function humanizeActivityTitle(value: string) {
    const normalized = value
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .replace(/[_-]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .toLowerCase();

    return normalized ? normalized.replace(/^\w/, (first: string) => first.toUpperCase()) : '';
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

  function parseTimestamp(value: unknown) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value;
    }

    if (typeof value === 'string') {
      const numeric = Number(value);

      if (Number.isFinite(numeric)) {
        return numeric;
      }

      const parsed = Date.parse(value);
      return Number.isFinite(parsed) ? parsed : undefined;
    }

    return undefined;
  }

  function normalizeCoalesceKey(
    value: unknown,
    title: string,
    record: Record<string, unknown>,
  ): ActivityCoalesceKey | undefined {
    const explicitKey = extractString(value);

    if (explicitKey === 'bootstrap') {
      return explicitKey;
    }

    const looksLikeBootstrap =
      title.toLowerCase().startsWith('bootstrap') ||
      typeof record.progress === 'number' ||
      typeof record.bootstrap === 'number';

    return looksLikeBootstrap ? 'bootstrap' : undefined;
  }

  function normalizeActivityEntry(payload: TorActivityEventDto | string | null | undefined) {
    if (payload == null) {
      return null;
    }

    if (typeof payload === 'string') {
      const title = payload.trim();

      return {
        id: nextActivityId(),
        timestamp: Date.now(),
        tone: 'neutral' as ActivityTone,
        title: title || DEFAULT_ACTIVITY_TITLE,
      };
    }

    if (typeof payload !== 'object') {
      return null;
    }

    const record = payload as Record<string, unknown>;
    const rawKind = extractString(record.kind) ?? extractString(record.type) ?? '';
    const title =
      extractString(record.title) ?? (humanizeActivityTitle(rawKind) || DEFAULT_ACTIVITY_TITLE);
    const timestamp =
      parseTimestamp(record.timestamp_ms) ?? parseTimestamp(record.timestamp) ?? Date.now();
    const tone = typeof record.tone === 'string' ? normalizeTone(record.tone) : 'neutral';
    const details = extractString(record.details) ?? extractString(record.message);
    const coalesceKey = normalizeCoalesceKey(record.coalesce_key, title, record);

    return {
      id: nextActivityId(),
      timestamp,
      tone,
      title,
      details,
      coalesceKey,
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
      actionErrorMessage = formatUiError('Action failed.', error);
      pendingAction = null;
      return;
    }

    if (eventErrorMessage) {
      try {
        await refreshRuntimeView();
        loadErrorMessage = '';
      } catch (error) {
        loadErrorMessage = formatUiError('Unable to refresh backend state.', error);
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

        <div class="control-feedback" aria-live="polite">
          {#if actionErrorMessage}
            <p class="inline-message inline-message-error">{actionErrorMessage}</p>
          {/if}

          {#if eventErrorMessage}
            <p class="inline-message inline-message-muted">{eventErrorMessage}</p>
          {/if}
        </div>
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
          <p class="empty-state">Runtime state is loading.</p>
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
          <p class="empty-state">Runtime snapshot is loading.</p>
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
          <p class="empty-state">Runtime snapshot is loading.</p>
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
          <p class="empty-state">Runtime snapshot is loading.</p>
        {/if}
      </Card>
    </div>
  </section>

  <section class="activity-panel" aria-label="Tor runtime activity">
    <Card title="Activity" subtitle="Recent runtime events.">
      {#if activitySubscriptionError}
        <p class="panel-note panel-note-error">{activitySubscriptionError}</p>
      {/if}

      <div class={`activity-feed ${activityEntries.length ? 'has-entries' : 'is-empty'}`}>
        {#if activityEntries.length}
          <ul class="activity-list">
            {#each activityEntries as entry}
              <li class={`activity-item tone-${entry.tone}`}>
                <span class="activity-marker" aria-hidden="true"></span>
                <div class="activity-copy">
                  <div class="activity-headline">
                    <strong class="activity-title">{entry.title}</strong>
                    <time class="activity-time" datetime={new Date(entry.timestamp).toISOString()}>
                      {formatActivityTime(entry.timestamp)}
                    </time>
                  </div>
                  {#if entry.details}
                    <p class="activity-details">{entry.details}</p>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty-state activity-empty-state">Activity will appear here.</p>
        {/if}
      </div>
    </Card>
  </section>

  {#if loadErrorMessage}
    <section class="error-panel" aria-live="polite">
      <h2>Load error</h2>
      <p>{loadErrorMessage}</p>
    </section>
  {/if}
</main>
