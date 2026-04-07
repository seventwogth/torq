<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ActivityPanel from './lib/components/ActivityPanel.svelte';
  import AppHeader from './lib/components/AppHeader.svelte';
  import RuntimeDetailsPanel from './lib/components/RuntimeDetailsPanel.svelte';
  import RuntimeOverviewPanel from './lib/components/RuntimeOverviewPanel.svelte';
  import ShellNotes from './lib/components/ShellNotes.svelte';
  import SettingsPanel from './lib/components/SettingsPanel.svelte';
  import {
    formatActionError,
    formatUiError,
    humanizeRuntimeMessage,
  } from './lib/runtime-feedback';
  import {
    bootstrapSourceToColor,
    formatBootstrapSource,
    formatRuntimeStatus,
    statusToColor
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
  <AppHeader
    {backendConnected}
    {backendStatusLabel}
    {runtimeStatusLabel}
    {runtimeStatusTone}
    {settingsOpen}
    {isTorActive}
    {nextThemeLabel}
    {themeLabel}
    {windowControlsAvailable}
    {windowMaximized}
    onOpenSettings={openSettingsPanel}
    onToggleTheme={toggleTheme}
    onMinimizeWindow={minimizeWindow}
    onToggleWindowMaximize={toggleWindowMaximize}
    onCloseWindow={closeWindow}
  />

  <div class="shell-content">
    <ShellNotes
      {controlHintMessage}
      {newIdentityHintMessage}
      {actionErrorMessage}
      {eventErrorMessage}
    />

    <div class="content-grid">
      <section class="runtime-column" aria-label="Runtime overview">
        <RuntimeOverviewPanel
          {torState}
          {runtimeStatusTone}
          {runtimeStatusLabel}
          {runtimeFocalMessage}
          {primaryActionTone}
          {primaryActionStateClass}
          {canRunPrimaryAction}
          {pendingAction}
          {displayedPrimaryAction}
          {canRestart}
          {canRequestNewIdentity}
          {controlSummaryLabel}
          {controlSummaryTone}
          {controlPortNote}
          {bootstrapSourceLabel}
          {bootstrapSourceTone}
          {runtimeStateEmptyMessage}
          snapshotUsesControlBootstrapObservation={snapshot?.uses_control_bootstrap_observation ?? false}
          onPerformAction={performAction}
          {actionLabel}
        />

        <RuntimeDetailsPanel
          {snapshot}
          {capabilities}
          {runtimeSnapshotEmptyMessage}
          {controlPortNote}
          {loadErrorMessage}
        />
      </section>

      <ActivityPanel
        {activitySubscriptionError}
        {activityEntries}
        {activityEmptyMessage}
        {formatActivityTime}
      />
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
