<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import Card from './lib/components/Card.svelte';
  import SettingsPanel from './lib/components/SettingsPanel.svelte';
  import StatusBadge from './lib/components/StatusBadge.svelte';
  import {
    formatActionError,
    formatUiError,
    humanizeRuntimeMessage,
  } from './lib/runtime-feedback';
  import {
    booleanToColor,
    bootstrapSourceToColor,
    controlAvailabilityToColor,
    formatBootstrapSource,
    formatControlPortValue,
    formatRuntimeStatus,
    statusToColor,
  } from './lib/status';
  import {
    fetchRuntimeConfig,
    fetchTorRuntimeSnapshot,
    fetchTorState,
    restartTor,
    requestNewIdentity,
    saveRuntimeConfig,
    TOR_ACTIVITY_EVENT,
    TOR_RUNTIME_SNAPSHOT_EVENT,
    TOR_STATE_EVENT,
    startTor,
    stopTor,
    type ActivityTone,
    type RuntimeConfigDto,
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
  let settingsOpen = false;
  let settingsLoading = false;
  let settingsLoadErrorMessage = '';
  let settingsConfig: RuntimeConfigDto | null = null;
  let settingsLoadSequence = 0;

  const ACTIVITY_HISTORY_LIMIT = 12;
  const DEFAULT_ACTIVITY_TITLE = 'Runtime event';
  const THEME_STORAGE_KEY = 'torq-theme';

  type ActionName = 'start' | 'stop' | 'restart' | 'new_identity';
  type ThemeName = 'dark' | 'light';

  type ActivityCoalesceKey = 'bootstrap';

  interface ActivityEntry {
    id: string;
    timestamp: number;
    tone: ActivityTone;
    title: string;
    details?: string;
    coalesceKey?: ActivityCoalesceKey;
  }

  let theme: ThemeName = 'dark';

  async function refreshRuntimeView() {
    const [nextState, nextSnapshot] = await Promise.all([
      fetchTorState(),
      fetchTorRuntimeSnapshot(),
    ]);

    state = nextState;
    snapshot = nextSnapshot;
    backendConnected = true;
  }

  function normalizeTheme(value: string | null | undefined): ThemeName {
    return value === 'light' ? 'light' : 'dark';
  }

  function applyTheme(nextTheme: ThemeName) {
    theme = nextTheme;
    document.documentElement.dataset.theme = nextTheme;
    localStorage.setItem(THEME_STORAGE_KEY, nextTheme);
  }

  function toggleTheme() {
    applyTheme(theme === 'dark' ? 'light' : 'dark');
  }

  onMount(() => {
    let active = true;
    const initialTheme = normalizeTheme(
      document.documentElement.dataset.theme || localStorage.getItem(THEME_STORAGE_KEY),
    );

    applyTheme(initialTheme);

    const initializeRuntimeView = async () => {
      try {
        unsubscribeStateEvent = await listen<TorStateDto>(TOR_STATE_EVENT, (event) => {
          state = event.payload;
          backendConnected = true;
          loadErrorMessage = '';
        });
      } catch (error) {
        if (active) {
          eventErrorMessage = formatUiError(
            'Live runtime updates are unavailable. One-time state refreshes may still work.',
            error,
          );
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
          eventErrorMessage = formatUiError(
            'Live runtime updates are unavailable. One-time state refreshes may still work.',
            error,
          );
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
          loadErrorMessage = formatUiError(
            'Could not read runtime state from the desktop backend.',
            error,
          );
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
  $: backendStatusLabel = backendConnected ? 'desktop backend ready' : 'desktop backend unavailable';
  $: themeLabel = theme === 'dark' ? 'Dark' : 'Light';
  $: nextThemeLabel = theme === 'dark' ? 'light' : 'dark';
  $: runtimeStateEmptyMessage = loadErrorMessage
    ? 'Runtime state is unavailable while the desktop backend cannot be read.'
    : 'Reading runtime state from the desktop backend.';
  $: runtimeSnapshotEmptyMessage = loadErrorMessage
    ? 'Runtime snapshot is unavailable while the desktop backend cannot be read.'
    : 'Reading runtime snapshot from the desktop backend.';
  $: controlHintMessage = deriveControlHint(snapshot, torState);
  $: newIdentityHintMessage = canRequestNewIdentity
    ? ''
    : deriveNewIdentityHint(snapshot, torState);
  $: activityEmptyMessage = deriveActivityEmptyMessage(torState, loadErrorMessage);
  $: controlPortNote = deriveControlPortNote(snapshot, torState);
  $: settingsRuntimeStatus = torState?.status ?? snapshot?.tor.status ?? 'stopped';
  $: settingsRestrictionMessage = deriveSettingsRestrictionMessage(torState);
  $: settingsControlStatusMessage = deriveSettingsControlStatusMessage(snapshot);
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
    const details = (extractString(record.details) ?? extractString(record.message))?.trim();
    const coalesceKey = normalizeCoalesceKey(record.coalesce_key, title, record);

    return {
      id: nextActivityId(),
      timestamp,
      tone,
      title,
      details: details ? humanizeRuntimeMessage(details) : undefined,
      coalesceKey,
    };
  }

  function appendActivityEntry(entry: ActivityEntry) {
    const baseEntries = entry.coalesceKey
      ? activityEntries.filter((current) => current.coalesceKey !== entry.coalesceKey)
      : activityEntries;

    activityEntries = [entry, ...baseEntries].slice(0, ACTIVITY_HISTORY_LIMIT);
  }

  function extractErrorMessage(error: unknown) {
    return error instanceof Error ? error.message.trim() : String(error).trim();
  }

  function humanizeSettingsMessage(rawMessage: string) {
    const normalizedMessage = rawMessage.toLowerCase();

    if (
      normalizedMessage.includes(
        'runtime config cannot be changed while tor is starting or running',
      )
    ) {
      return 'Runtime config can only be changed while Tor is stopped.';
    }

    if (normalizedMessage.includes('tor_path') && normalizedMessage.includes('must not be empty')) {
      return 'Tor path is required.';
    }

    if (normalizedMessage.includes('log_path') && normalizedMessage.includes('must not be empty')) {
      return 'Log path is required.';
    }

    if (
      normalizedMessage.includes('control.host') &&
      normalizedMessage.includes('must not be empty')
    ) {
      return 'Control host is required when ControlPort config is enabled.';
    }

    if (
      normalizedMessage.includes('control.auth.cookie_path') &&
      normalizedMessage.includes('must not be empty')
    ) {
      return 'Cookie path is required when ControlPort auth mode is cookie.';
    }

    return humanizeRuntimeMessage(rawMessage);
  }

  function formatSettingsError(prefix: string, error: unknown) {
    const message = extractErrorMessage(error);

    if (!message) {
      return prefix;
    }

    return `${prefix} ${humanizeSettingsMessage(message)}`;
  }

  async function openSettingsPanel() {
    settingsOpen = true;
    settingsLoading = true;
    settingsLoadErrorMessage = '';
    settingsConfig = null;

    const loadId = ++settingsLoadSequence;

    try {
      const config = await fetchRuntimeConfig();

      if (loadId !== settingsLoadSequence) {
        return;
      }

      settingsConfig = config;
    } catch (error) {
      if (loadId !== settingsLoadSequence) {
        return;
      }

      settingsLoadErrorMessage = formatSettingsError(
        'Could not load runtime configuration.',
        error,
      );
    } finally {
      if (loadId === settingsLoadSequence) {
        settingsLoading = false;
      }
    }
  }

  function closeSettingsPanel() {
    settingsLoadSequence += 1;
    settingsOpen = false;
    settingsLoading = false;
    settingsLoadErrorMessage = '';
  }

  async function handleSettingsSave(config: RuntimeConfigDto) {
    try {
      const savedConfig = await saveRuntimeConfig(config);
      settingsConfig = savedConfig;
      settingsLoadErrorMessage = '';
      return savedConfig;
    } catch (error) {
      throw new Error(formatSettingsError('Could not save runtime configuration.', error));
    }
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
      actionErrorMessage = formatActionError(action, error);
      pendingAction = null;
      return;
    }

    if (eventErrorMessage) {
      try {
        await refreshRuntimeView();
        loadErrorMessage = '';
      } catch (error) {
        loadErrorMessage = formatUiError(
          'Could not refresh runtime state from the desktop backend.',
          error,
        );
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

  function deriveControlHint(
    snapshot: TorRuntimeSnapshotDto | null,
    torState: TorStateDto | null,
  ) {
    if (!snapshot || !torState) {
      return 'Reading runtime state from the desktop backend.';
    }

    if (torState.status === 'failed') {
      return 'The last Tor start attempt failed. Review the latest error or activity entry before retrying.';
    }

    if (!isTorActive && !snapshot.control_configured) {
      return 'Tor is stopped. ControlPort is not configured, so New Identity and control-backed bootstrap updates will stay unavailable.';
    }

    if (isTorActive && !snapshot.control_configured) {
      return 'Tor is running without ControlPort configuration. New Identity and control-backed bootstrap updates are unavailable.';
    }

    if (isTorActive && !snapshot.control_available) {
      return 'Tor is running, but ControlPort is not reachable. New Identity and control-backed bootstrap updates are unavailable.';
    }

    if (torState.status === 'starting' && !snapshot.bootstrap_observation_available) {
      return 'Tor is starting. Bootstrap progress is currently falling back to log-based observation.';
    }

    return '';
  }

  function deriveNewIdentityHint(
    snapshot: TorRuntimeSnapshotDto | null,
    torState: TorStateDto | null,
  ) {
    if (!snapshot || !torState) {
      return '';
    }

    if (torState.status !== 'starting' && torState.status !== 'running') {
      return 'New Identity is unavailable while Tor is stopped.';
    }

    if (!snapshot.control_configured) {
      return 'New Identity requires ControlPort configuration.';
    }

    if (!snapshot.control_available) {
      return 'New Identity is unavailable because ControlPort is not reachable.';
    }

    return '';
  }

  function deriveActivityEmptyMessage(torState: TorStateDto | null, loadErrorMessage: string) {
    if (loadErrorMessage) {
      return 'Runtime activity is unavailable while backend state cannot be read.';
    }

    if (torState?.status === 'failed') {
      return 'No new runtime events yet. Review the latest start failure before retrying.';
    }

    if (torState?.status === 'starting' || torState?.status === 'running') {
      return 'Waiting for the next runtime event.';
    }

    return 'No runtime events yet. Start Tor to see lifecycle and bootstrap activity.';
  }

  function deriveControlPortNote(
    snapshot: TorRuntimeSnapshotDto | null,
    torState: TorStateDto | null,
  ) {
    if (!snapshot) {
      return '';
    }

    if (!snapshot.control_configured) {
      return 'ControlPort is not configured. New Identity and control-backed bootstrap observation stay unavailable.';
    }

    if (
      (torState?.status === 'starting' || torState?.status === 'running') &&
      !snapshot.control_available
    ) {
      return 'ControlPort is configured but not currently reachable from the desktop runtime.';
    }

    if (!snapshot.control_available) {
      return 'ControlPort is configured. Availability will be checked after Tor starts.';
    }

    return 'ControlPort is reachable for bootstrap observation and New Identity requests.';
  }

  function deriveSettingsRestrictionMessage(torState: TorStateDto | null) {
    if (!torState || (torState.status !== 'starting' && torState.status !== 'running')) {
      return '';
    }

    return `Runtime config is locked while Tor is ${formatRuntimeStatus(torState.status).toLowerCase()}. Stop the runtime before saving changes.`;
  }

  function deriveSettingsControlStatusMessage(snapshot: TorRuntimeSnapshotDto | null) {
    if (!snapshot) {
      return 'ControlPort settings stay optional until you need identity actions or control-backed bootstrap observation.';
    }

    if (!snapshot.control_configured) {
      return 'ControlPort config is currently disabled. New Identity and control-backed bootstrap observation stay unavailable until it is configured.';
    }

    if (!snapshot.control_available) {
      return 'ControlPort config is present, but availability will only be confirmed after Tor starts and the port becomes reachable.';
    }

    return 'ControlPort is configured and currently reachable for bootstrap observation and New Identity requests.';
  }
</script>

<svelte:head>
  <title>torq</title>
</svelte:head>

<main class="shell">
  <header class="toolbar" aria-label="Application toolbar">
    <div class="toolbar-group toolbar-brand-group">
      <div class="brand-lockup">
        <p class="toolbar-eyebrow">Desktop runtime shell</p>
        <h1>torq</h1>
      </div>

      <StatusBadge
        label={backendStatusLabel}
        tone={backendConnected ? 'success' : 'danger'}
      />
    </div>

    <div class="toolbar-group toolbar-runtime" aria-label="Runtime controls">
      <button
        type="button"
        class={`action-button action-button-primary ${primaryActionTone}`}
        disabled={!canRunPrimaryAction || pendingAction !== null}
        aria-busy={pendingAction === primaryAction}
        on:click={() => performAction(primaryAction)}
      >
        {actionLabel(primaryAction)}
      </button>

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

    <div class="toolbar-group toolbar-service">
      <button
        type="button"
        class="toolbar-service-button"
        aria-haspopup="dialog"
        aria-expanded={settingsOpen}
        on:click={openSettingsPanel}
      >
        <span class="toolbar-service-label">Settings</span>
        <span class="toolbar-service-value">{isTorActive ? 'Locked' : 'Edit'}</span>
      </button>

      <button
        type="button"
        class="toolbar-service-button"
        aria-label={`Switch to ${nextThemeLabel} theme`}
        on:click={toggleTheme}
      >
        <span class="toolbar-service-label">Theme</span>
        <span class="toolbar-service-value">{themeLabel}</span>
      </button>
    </div>
  </header>

  <section class="shell-notes" aria-live="polite">
    {#if controlHintMessage}
      <p class="inline-message inline-message-muted">{controlHintMessage}</p>
    {/if}

    {#if newIdentityHintMessage && newIdentityHintMessage !== controlHintMessage}
      <p class="inline-message inline-message-muted">{newIdentityHintMessage}</p>
    {/if}

    {#if actionErrorMessage}
      <p class="inline-message inline-message-error">{actionErrorMessage}</p>
    {/if}

    {#if eventErrorMessage}
      <p class="inline-message inline-message-muted">{eventErrorMessage}</p>
    {/if}
  </section>

  <div class="content-grid">
    <section class="panel-surface dashboard-panel" aria-label="Tor runtime status panel">
      <div class="section-heading">
        <div class="section-heading-copy">
          <p class="section-kicker">Dashboard</p>
          <h2>Runtime overview</h2>
        </div>
        <p>
          Lifecycle, ControlPort health, and runtime capabilities rendered from the existing
          `tor_state` and `tor_runtime_snapshot` desktop commands.
        </p>
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
                <strong class="metric-value metric-value-mono">{torState.bootstrap}%</strong>
              </div>

              {#if torState.status === 'failed'}
                <p class="supporting-text">
                  The last start attempt failed. Check the latest action error or activity entry.
                </p>
              {/if}
            </div>
          {:else}
            <p class="empty-state">{runtimeStateEmptyMessage}</p>
          {/if}
        </Card>

        <Card title="ControlPort" subtitle="ControlPort configuration and current availability.">
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
            <p class="empty-state">{runtimeSnapshotEmptyMessage}</p>
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
        </Card>
      </div>

      {#if loadErrorMessage}
        <section class="error-panel" aria-live="polite">
          <h2>Backend state unavailable</h2>
          <p>{loadErrorMessage}</p>
        </section>
      {/if}
    </section>

    <section class="panel-surface activity-panel" aria-label="Tor runtime activity">
      <div class="section-heading">
        <div class="section-heading-copy">
          <p class="section-kicker">Activity</p>
          <h2>Runtime stream</h2>
        </div>
        <p>Recent runtime events from the desktop backend.</p>
      </div>

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
          <p class="empty-state activity-empty-state">{activityEmptyMessage}</p>
        {/if}
      </div>
    </section>
  </div>

  <SettingsPanel
    open={settingsOpen}
    config={settingsConfig}
    loading={settingsLoading}
    loadErrorMessage={settingsLoadErrorMessage}
    runtimeStatus={settingsRuntimeStatus}
    restricted={isTorActive}
    restrictionMessage={settingsRestrictionMessage}
    saveAction={handleSettingsSave}
    on:cancel={closeSettingsPanel}
  >
    <p slot="status" class="settings-panel-note">
      Current values are loaded fresh from the backend config layer each time the settings panel
      opens.
    </p>
    <p slot="control-status" class="settings-panel-note">{settingsControlStatusMessage}</p>
    <p slot="runtime-status" class="settings-panel-meta">
      Config updates stay local to this panel until you save. Backend validation remains the
      source of truth.
    </p>
  </SettingsPanel>
</main>
