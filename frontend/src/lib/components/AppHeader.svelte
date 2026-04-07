<script lang="ts">
  export let backendConnected = false;
  export let backendStatusLabel = '';
  export let runtimeStatusLabel = '';
  export let runtimeStatusTone = 'neutral';
  export let settingsOpen = false;
  export let isTorActive = false;
  export let nextThemeLabel = 'dark';
  export let themeLabel = 'Dark';
  export let windowControlsAvailable = false;
  export let windowMaximized = false;
  export let onOpenSettings: () => void = () => {};
  export let onToggleTheme: () => void = () => {};
  export let onMinimizeWindow: () => void = () => {};
  export let onToggleWindowMaximize: () => void = () => {};
  export let onCloseWindow: () => void = () => {};
</script>

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

    <div class="header-secondary-actions">
      <button
        type="button"
        class="toolbar-service-button header-secondary-button"
        aria-haspopup="dialog"
        aria-expanded={settingsOpen}
        on:click={onOpenSettings}
      >
        Settings
        <span class="toolbar-service-value">{isTorActive ? 'Locked' : 'Edit'}</span>
      </button>

      <button
        type="button"
        class="toolbar-service-button header-secondary-button"
        aria-label={`Switch to ${nextThemeLabel} theme`}
        on:click={onToggleTheme}
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
          on:click={onMinimizeWindow}
        >
          <span class="window-control-glyph is-minimize" aria-hidden="true"></span>
        </button>

        <button
          type="button"
          class="window-control"
          aria-label={windowMaximized ? 'Restore window' : 'Maximize window'}
          on:click={onToggleWindowMaximize}
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
          on:click={onCloseWindow}
        >
          <span class="window-control-glyph is-close" aria-hidden="true"></span>
        </button>
      </div>
    {/if}
  </div>
</header>
