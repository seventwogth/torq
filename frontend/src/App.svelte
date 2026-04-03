<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
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
  let windowMaximized = false;
  let windowControlsAvailable = false;

  const ACTIVITY_HISTORY_LIMIT = 12;
  const DEFAULT_ACTIVITY_TITLE = 'Runtime event';
  const THEME_STORAGE_KEY = 'torq-theme';

  type ActionName = 'start' | 'stop' | 'restart' | 'new_identity';
  type ThemeName = 'dark' | 'light';
  type ActivityCoalesceKey = 'bootstrap';
  type StatusTone = 'success' | 'warning' | 'danger' | 'neutral' | 'muted';

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

  function canUseDesktopWindowApis() {
    return (
      typeof window !== 'undefined' &&
      '__TAURI_INTERNALS__' in (window as Window & { __TAURI_INTERNALS__?: unknown })
    );
  }

  function getDesktopWindow() {
    return canUseDesktopWindowApis() ? getCurrentWindow() : null;
  }

  async function initializeWindowChrome() {
    const desktopWindow = getDesktopWindow();

    if (!desktopWindow) {
      return;
    }

    windowControlsAvailable = true;

    try {
      windowMaximized = await desktopWindow.isMaximized();
    } catch {
      windowMaximized = false;
    }
  }

  async function minimizeWindow() {
    const desktopWindow = getDesktopWindow();

    if (!desktopWindow) {
      return;
    }

    try {
      await desktopWindow.minimize();
    } catch {
      // Window chrome is non-critical; runtime controls must remain usable.
    }
  }

  async function toggleWindowMaximize() {
    const desktopWindow = getDesktopWindow();

    if (!desktopWindow) {
      return;
    }

    try {
      await desktopWindow.toggleMaximize();
      windowMaximized = !windowMaximized;
    } catch {
      // Ignore window chrome failures and keep the rest of the UI responsive.
    }
  }

  async function closeWindow() {
    const desktopWindow = getDesktopWindow();

    if (!desktopWindow) {
      return;
    }

    try {
      await desktopWindow.close();
    } catch {
      // Ignore window chrome failures and keep the rest of the UI responsive.
    }
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
    void initializeWindowChrome();

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
  $: displayedPrimaryAction = (pendingAction === 'stop' ? 'stop' : primaryAction) as ActionName;
  $: primaryActionTone = displayedPrimaryAction === 'start' ? 'primary' : 'danger';
  $: primaryActionStateClass = pendingAction === 'stop' ? 'is-stopping' : '';
  $: canRunPrimaryAction = displayedPrimaryAction === 'start' ? canStart : canStop;
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
  $: runtimeStatusLabel = deriveRuntimeStatusLabel(torState, backendConnected);
  $: runtimeStatusTone = deriveRuntimeStatusTone(torState, backendConnected);
  $: runtimeFocalMessage = deriveRuntimeFocalMessage(torState, loadErrorMessage);
  $: controlSummaryLabel = deriveControlSummaryLabel(snapshot, torState);
  $: controlSummaryTone = deriveControlSummaryTone(snapshot, torState);
  $: bootstrapSourceLabel = snapshot ? formatBootstrapSource(snapshot) : 'Pending';
  $: bootstrapSourceTone = snapshot ? bootstrapSourceToColor(snapshot) : 'neutral';
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

  function deriveRuntimeStatusLabel(torState: TorStateDto | null, backendConnected: boolean) {
    if (torState) {
      return formatRuntimeStatus(torState.status);
    }

    return backendConnected ? 'Pending' : 'Connecting';
  }

  function deriveRuntimeStatusTone(
    torState: TorStateDto | null,
    backendConnected: boolean,
  ): StatusTone {
    if (torState) {
      return statusToColor[torState.status];
    }

    return backendConnected ? 'neutral' : 'warning';
  }

  function deriveRuntimeFocalMessage(torState: TorStateDto | null, loadErrorMessage: string) {
    if (loadErrorMessage) {
      return 'The desktop backend is not currently providing a readable runtime state.';
    }

    if (!torState) {
      return 'Reading runtime state from the desktop backend.';
    }

    if (torState.status === 'starting') {
      return 'Tor is bootstrapping. Progress and control availability will continue to update in place.';
    }

    if (torState.status === 'running') {
      return 'Tor is running. Identity and control-backed actions remain available when ControlPort stays reachable.';
    }

    if (torState.status === 'failed') {
      return 'The last Tor start attempt failed. Review the latest action error or activity entry before retrying.';
    }

    return 'Tor is stopped. Start the runtime to restore bootstrap observation and lifecycle activity.';
  }

  function deriveControlSummaryLabel(
    snapshot: TorRuntimeSnapshotDto | null,
    torState: TorStateDto | null,
  ) {
    if (!snapshot) {
      return 'Pending';
    }

    if (!snapshot.control_configured) {
      return 'Disabled';
    }

    if (snapshot.control_available) {
      return 'Reachable';
    }

    if (torState?.status === 'starting' || torState?.status === 'running') {
      return 'Unavailable';
    }

    return 'Awaiting start';
  }

  function deriveControlSummaryTone(
    snapshot: TorRuntimeSnapshotDto | null,
    torState: TorStateDto | null,
  ): StatusTone {
    if (!snapshot) {
      return 'neutral';
    }

    if (!snapshot.control_configured) {
      return 'muted';
    }

    if (snapshot.control_available) {
      return 'success';
    }

    if (torState?.status === 'starting' || torState?.status === 'running') {
      return 'warning';
    }

    return 'neutral';
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
  <header class="app-header" aria-label="Application header">
    <div class="app-header-row">
      <div class="header-left">
        <h1>torq</h1>
        <span
          class={`header-backend-status ${backendConnected ? 'is-success' : 'is-danger'}`}
          title={backendStatusLabel}
        >
          <span class="header-status-dot" aria-hidden="true"></span>
          <span class="header-backend-label">{backendStatusLabel}</span>
        </span>
        <span
          class={`header-runtime-status tone-${runtimeStatusTone}`}
          aria-label={`Runtime status: ${runtimeStatusLabel}`}
        >
          {runtimeStatusLabel}
        </span>
      </div>

      <div class="header-drag-space" aria-hidden="true"></div>

      <div class="header-runtime-actions" aria-label="Runtime controls">
        <button
          type="button"
          class={`action-button header-action-button action-button-primary ${primaryActionTone} ${primaryActionStateClass}`}
          disabled={!canRunPrimaryAction || pendingAction !== null}
          aria-busy={pendingAction === displayedPrimaryAction}
          on:click={() => performAction(displayedPrimaryAction)}
        >
          {actionLabel(displayedPrimaryAction)}
        </button>

        <button
          type="button"
          class="action-button header-action-button action-button-secondary"
          disabled={!canRestart || pendingAction !== null}
          aria-busy={pendingAction === 'restart'}
          on:click={() => performAction('restart')}
        >
          {actionLabel('restart')}
        </button>

        <button
          type="button"
          class="action-button header-action-button action-button-secondary"
          disabled={!canRequestNewIdentity || pendingAction !== null}
          aria-busy={pendingAction === 'new_identity'}
          on:click={() => performAction('new_identity')}
        >
          {actionLabel('new_identity')}
        </button>
      </div>

      <div class="header-secondary-actions">
        <button
          type="button"
          class="toolbar-service-button header-secondary-button"
          aria-haspopup="dialog"
          aria-expanded={settingsOpen}
          on:click={openSettingsPanel}
        >
          Settings
          <span class="toolbar-service-value">{isTorActive ? 'Locked' : 'Edit'}</span>
        </button>

        <button
          type="button"
          class="toolbar-service-button header-secondary-button"
          aria-label={`Switch to ${nextThemeLabel} theme`}
          on:click={toggleTheme}
        >
          Theme
          <span class="toolbar-service-value">{themeLabel}</span>
        </button>
      </div>

      {#if windowControlsAvailable}
        <div class="window-controls" aria-label="Window controls">
          <button
            type="button"
            class="window-control"
            aria-label="Minimize window"
            on:click={minimizeWindow}
          >
            <span class="window-control-glyph is-minimize" aria-hidden="true"></span>
          </button>

          <button
            type="button"
            class="window-control"
            aria-label={windowMaximized ? 'Restore window' : 'Maximize window'}
            on:click={toggleWindowMaximize}
          >
            <span
              class={`window-control-glyph ${windowMaximized ? 'is-restore' : 'is-maximize'}`}
              aria-hidden="true"
            ></span>
          </button>

          <button
            type="button"
            class="window-control is-close"
            aria-label="Close window"
            on:click={closeWindow}
          >
            <span class="window-control-glyph is-close" aria-hidden="true"></span>
          </button>
        </div>
      {/if}
    </div>
  </header>

  <div class="shell-content">
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
      <section class="runtime-column" aria-label="Runtime overview">
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
              </div>

              <div class="runtime-metrics">
                <div class="runtime-metric">
                  <span class="metric-label">Bootstrap</span>
                  <strong class="metric-value metric-value-mono runtime-bootstrap-value">
                    {torState.bootstrap}%
                  </strong>
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
                    {snapshot?.uses_control_bootstrap_observation
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
                    <StatusBadge
                      label={formatBootstrapSource(snapshot)}
                      tone={bootstrapSourceToColor(snapshot)}
                    />
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
      </section>

      <section class="app-section activity-panel" aria-label="Tor runtime activity">
        <div class="section-heading">
          <div class="section-heading-copy">
            <p class="section-kicker">Activity</p>
            <h2>Recent events</h2>
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
