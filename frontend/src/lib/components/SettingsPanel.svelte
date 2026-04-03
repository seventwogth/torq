<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { RuntimeConfigDto } from '../runtime-config';
  import {
    buildRuntimeConfigRequest,
    createRuntimeConfigFormState,
    hasRuntimeConfigFormErrors,
    validateRuntimeConfigForm,
    type RuntimeConfigFormState,
  } from '../runtime-config-form';
  import StatusBadge from './StatusBadge.svelte';

  type RuntimeStatus = 'stopped' | 'starting' | 'running' | 'failed';
  type RuntimeBadgeTone = 'success' | 'warning';

  const EMPTY_CONFIG: RuntimeConfigDto = {
    tor_path: '',
    log_path: '',
    log_mode: 'managed',
    args: [],
    working_dir: null,
    control: null,
    stop_timeout_ms: 5_000,
    log_poll_interval_ms: 250,
  };

  const dispatch = createEventDispatcher<{
    cancel: void;
  }>();

  export let open = false;
  export let title = 'Settings';
  export let subtitle = 'Runtime configuration for the desktop backend.';
  export let config: RuntimeConfigDto | null = null;
  export let runtimeStatus: RuntimeStatus = 'stopped';
  export let restricted = false;
  export let restrictionMessage = '';
  export let disabled = false;
  export let loading = false;
  export let loadErrorMessage = '';
  export let saveAction:
    | ((config: RuntimeConfigDto) => Promise<RuntimeConfigDto | void> | RuntimeConfigDto | void)
    | undefined = undefined;

  let draft: RuntimeConfigFormState = createRuntimeConfigFormState(EMPTY_CONFIG);
  let lastLoadedSignature = '';
  let submitAttempted = false;
  let submitState: 'idle' | 'saving' | 'success' | 'error' = 'idle';
  let submitMessage = '';
  let submitError = '';

  const panelId = `settings-panel-${Math.random().toString(36).slice(2, 8)}`;
  const titleId = `${panelId}-title`;
  const descriptionId = `${panelId}-description`;

  $: sourceSignature = signature(config);

  $: if (open && sourceSignature !== lastLoadedSignature) {
    draft = createRuntimeConfigFormState(config ?? EMPTY_CONFIG);
    lastLoadedSignature = sourceSignature;
    submitAttempted = false;
    submitState = 'idle';
    submitMessage = '';
    submitError = '';
  }

  $: runtimeLocked = restricted || runtimeStatus === 'starting' || runtimeStatus === 'running';
  $: effectiveRestrictionMessage =
    restrictionMessage ||
    (runtimeLocked ? 'Runtime config can only be changed while Tor is stopped.' : '');
  $: fieldErrors = validateRuntimeConfigForm(draft);
  $: hasValidationErrors = hasRuntimeConfigFormErrors(fieldErrors);
  $:
    formDisabled = disabled || loading || runtimeLocked || submitState === 'saving' || config === null;
  $: saveDisabled = formDisabled || hasValidationErrors;
  $: runtimeBadgeLabel = runtimeLocked ? 'Locked' : 'Editable';
  $: runtimeBadgeTone = (runtimeLocked ? 'warning' : 'success') as RuntimeBadgeTone;

  function signature(value: RuntimeConfigDto | null) {
    return JSON.stringify(value ?? null);
  }

  function formatError(error: unknown) {
    if (error instanceof Error && error.message.trim()) {
      return error.message.trim();
    }

    if (typeof error === 'string' && error.trim()) {
      return error.trim();
    }

    return 'Unable to save settings.';
  }

  async function handleSave() {
    if (!config) {
      return;
    }

    submitAttempted = true;
    submitError = '';
    submitMessage = '';

    if (hasRuntimeConfigFormErrors(fieldErrors)) {
      submitState = 'error';
      submitError = 'Resolve the highlighted fields before saving.';
      return;
    }

    const payload = buildRuntimeConfigRequest(draft, config);
    submitState = 'saving';

    try {
      const savedConfig = (await saveAction?.(payload)) ?? payload;
      draft = createRuntimeConfigFormState(savedConfig);
      lastLoadedSignature = signature(savedConfig);
      submitState = 'success';
      submitMessage = 'Settings saved.';
      submitError = '';
    } catch (error) {
      submitState = 'error';
      submitError = formatError(error);
      submitMessage = '';
    }
  }

  function handleCancel() {
    if (submitState === 'saving') {
      return;
    }

    draft = createRuntimeConfigFormState(config ?? EMPTY_CONFIG);
    lastLoadedSignature = signature(config);
    submitAttempted = false;
    submitState = 'idle';
    submitMessage = '';
    submitError = '';
    dispatch('cancel');
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      handleCancel();
    }
  }

  function handleBackdropKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      handleCancel();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open) {
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      handleCancel();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if open}
  <div
    class="settings-layer"
    role="presentation"
    tabindex="-1"
    on:click={handleBackdropClick}
    on:keydown={handleBackdropKeydown}
  >
    <div
      class="settings-panel"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
    >
      <header class="settings-header">
        <div class="settings-title-block">
          <p class="eyebrow">Runtime configuration</p>
          <h2 id={titleId}>{title}</h2>
          <p id={descriptionId}>{subtitle}</p>
        </div>

        <div class="settings-header-meta">
          <StatusBadge label={runtimeBadgeLabel} tone={runtimeBadgeTone} />
          <button
            type="button"
            class="close-button"
            aria-label="Close settings"
            disabled={submitState === 'saving'}
            on:click={handleCancel}
          >
            Close
          </button>
        </div>
      </header>

      <div class="settings-status" aria-live="polite">
        {#if effectiveRestrictionMessage}
          <p class="inline-message inline-message-warning">{effectiveRestrictionMessage}</p>
        {/if}

        {#if loadErrorMessage}
          <p class="inline-message inline-message-error">{loadErrorMessage}</p>
        {/if}

        {#if submitState === 'success' && submitMessage}
          <p class="inline-message inline-message-success">{submitMessage}</p>
        {/if}

        {#if submitState === 'error' && submitError && submitError !== loadErrorMessage}
          <p class="inline-message inline-message-error">{submitError}</p>
        {/if}

        <slot name="status" />
      </div>

      {#if loading}
        <div class="empty-panel-state">
          <p class="empty-state-title">Loading runtime config</p>
          <p class="empty-state-copy">Reading current values from the desktop backend.</p>
        </div>
      {:else if !config}
        <div class="empty-panel-state">
          <p class="empty-state-title">Runtime config unavailable</p>
          <p class="empty-state-copy">
            The desktop backend did not return a configuration payload for the settings panel.
          </p>
        </div>
      {:else}
        <div class="settings-body">
          <section class="settings-section" aria-labelledby={`${titleId}-runtime`}>
            <div class="settings-section-header">
              <h3 id={`${titleId}-runtime`}>Runtime</h3>
              <p>Paths and launch mode for the current runtime.</p>
            </div>

            <div class="field-grid">
              <label class="field">
                <span class="field-label">Tor path</span>
                <input
                  type="text"
                  class="field-input"
                  bind:value={draft.torPath}
                  disabled={formDisabled}
                  placeholder="C:\\Tor\\tor.exe"
                />
                {#if submitAttempted && fieldErrors.torPath}
                  <span class="field-error">{fieldErrors.torPath}</span>
                {/if}
              </label>

              <label class="field">
                <span class="field-label">Log path</span>
                <input
                  type="text"
                  class="field-input"
                  bind:value={draft.logPath}
                  disabled={formDisabled}
                  placeholder="C:\\Tor\\tor.log"
                />
                {#if submitAttempted && fieldErrors.logPath}
                  <span class="field-error">{fieldErrors.logPath}</span>
                {/if}
              </label>

              <label class="field">
                <span class="field-label">Log mode</span>
                <select class="field-input" bind:value={draft.logMode} disabled={formDisabled}>
                  <option value="managed">Managed</option>
                  <option value="external">External</option>
                </select>
                <span class="field-hint">
                  Managed mode lets the runtime own the log destination.
                </span>
              </label>

              <label class="field">
                <span class="field-label">Working dir</span>
                <input
                  type="text"
                  class="field-input"
                  bind:value={draft.workingDir}
                  disabled={formDisabled}
                  placeholder="Optional"
                />
              </label>
            </div>

            <div class="settings-divider" aria-hidden="true"></div>
          </section>

          <section class="settings-section" aria-labelledby={`${titleId}-control`}>
            <div class="settings-section-header">
              <h3 id={`${titleId}-control`}>ControlPort</h3>
              <p>Minimal control configuration for identity and bootstrap use.</p>
            </div>

            <div class="control-toggle-row">
              <label class="toggle">
                <input
                  type="checkbox"
                  bind:checked={draft.controlEnabled}
                  disabled={formDisabled}
                />
                <span>Enable ControlPort config</span>
              </label>
              <StatusBadge
                label={draft.controlEnabled ? 'Configured' : 'Disabled'}
                tone={draft.controlEnabled ? 'muted' : 'neutral'}
              />
            </div>

            <div class="field-grid">
              <label class="field">
                <span class="field-label">Host</span>
                <input
                  type="text"
                  class="field-input"
                  bind:value={draft.controlHost}
                  disabled={formDisabled || !draft.controlEnabled}
                  placeholder="127.0.0.1"
                />
                {#if submitAttempted && fieldErrors.controlHost}
                  <span class="field-error">{fieldErrors.controlHost}</span>
                {/if}
              </label>

              <label class="field">
                <span class="field-label">Port</span>
                <input
                  type="text"
                  inputmode="numeric"
                  class="field-input field-input-mono"
                  bind:value={draft.controlPort}
                  disabled={formDisabled || !draft.controlEnabled}
                  placeholder="9051"
                />
                {#if submitAttempted && fieldErrors.controlPort}
                  <span class="field-error">{fieldErrors.controlPort}</span>
                {/if}
              </label>

              <label class="field">
                <span class="field-label">Auth mode</span>
                <select
                  class="field-input"
                  bind:value={draft.controlAuth}
                  disabled={formDisabled || !draft.controlEnabled}
                >
                  <option value="null">Null</option>
                  <option value="cookie">Cookie</option>
                </select>
                <span class="field-hint">Cookie auth reveals one extra path field.</span>
              </label>

              <label class="field">
                <span class="field-label">Cookie path</span>
                <input
                  type="text"
                  class="field-input"
                  bind:value={draft.controlCookiePath}
                  disabled={
                    formDisabled || !draft.controlEnabled || draft.controlAuth !== 'cookie'
                  }
                  placeholder="Required when auth is cookie"
                />
                {#if submitAttempted && fieldErrors.controlCookiePath}
                  <span class="field-error">{fieldErrors.controlCookiePath}</span>
                {/if}
              </label>
            </div>

            <div class="control-note">
              <slot name="control-status" />
            </div>

            <div class="settings-divider" aria-hidden="true"></div>
          </section>

          <div class="panel-metadata">
            <slot name="runtime-status" />
          </div>
        </div>

        <footer class="settings-footer">
          <button
            type="button"
            class="action-button action-button-secondary"
            disabled={submitState === 'saving'}
            on:click={handleCancel}
          >
            Cancel
          </button>

          <button
            type="button"
            class="action-button action-button-primary primary"
            disabled={saveDisabled}
            aria-busy={submitState === 'saving'}
            on:click={handleSave}
          >
            {submitState === 'saving' ? 'Saving...' : 'Save'}
          </button>
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .settings-layer {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    justify-items: end;
    background: color-mix(in srgb, var(--color-bg) 62%, transparent);
    backdrop-filter: blur(8px);
  }

  .settings-panel {
    width: min(100%, 680px);
    height: 100%;
    overflow: auto;
    border-left: 1px solid color-mix(in srgb, var(--color-border) 82%, transparent);
    background: color-mix(in srgb, var(--color-surface) 97%, var(--color-surface-elevated));
    padding: 20px;
    display: grid;
    gap: 16px;
    align-content: start;
    box-shadow: inset 1px 0 0 color-mix(in srgb, white 2%, transparent);
  }

  .settings-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 12px;
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 76%, transparent);
  }

  .settings-title-block {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .eyebrow {
    color: color-mix(in srgb, var(--color-primary) 72%, var(--color-text-secondary));
    text-transform: uppercase;
    letter-spacing: 0.12em;
    font-size: 0.72rem;
    font-weight: 600;
  }

  h2 {
    margin: 0;
    font-size: clamp(1.1rem, 1.8vw, 1.35rem);
    line-height: 1.1;
    letter-spacing: -0.02em;
    font-weight: 600;
  }

  p {
    margin: 0;
  }

  .settings-title-block p:last-child,
  .empty-state-copy {
    color: var(--color-text-secondary);
    font-size: 0.8rem;
    line-height: 1.55;
    max-width: 64ch;
  }

  .settings-header-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .close-button,
  .action-button {
    border: 1px solid color-mix(in srgb, var(--color-border) 92%, transparent);
    border-radius: var(--radius-md, 10px);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      border-color 120ms ease,
      opacity 120ms ease;
  }

  .close-button:focus-visible,
  .action-button:focus-visible,
  .field-input:focus-visible {
    outline: none;
    border-color: var(--color-focus);
  }

  .close-button {
    min-height: 32px;
    padding: 0 12px;
    background: color-mix(in srgb, var(--color-surface-elevated) 18%, var(--color-surface));
    color: var(--color-text-secondary);
    font-size: 0.78rem;
  }

  .close-button:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--color-text-secondary) 36%, var(--color-border));
    color: var(--color-text-primary);
  }

  .close-button:disabled,
  .action-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .settings-status,
  .settings-body,
  .control-note,
  .panel-metadata {
    display: grid;
  }

  .settings-status,
  .control-note,
  .panel-metadata {
    gap: 6px;
  }

  .settings-body {
    gap: 16px;
  }

  .settings-section {
    display: grid;
    gap: 12px;
    padding: 12px 0 0;
  }

  .settings-section-header {
    display: grid;
    gap: 4px;
  }

  .settings-section-header h3 {
    margin: 0;
    color: var(--color-text-primary);
    font-size: 0.86rem;
    line-height: 1.35;
    font-weight: 600;
    letter-spacing: 0.01em;
  }

  .settings-section-header p {
    color: var(--color-muted);
    font-size: 0.78rem;
    line-height: 1.55;
    max-width: 64ch;
  }

  .settings-divider {
    height: 1px;
    background: color-mix(in srgb, var(--color-border) 76%, transparent);
  }

  .field-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px 16px;
  }

  .field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }

  .field-label {
    color: color-mix(in srgb, var(--color-text-secondary) 84%, var(--color-muted));
    font-size: 0.78rem;
    line-height: 1.45;
    letter-spacing: 0.01em;
  }

  .field-input {
    min-height: 34px;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, var(--color-border) 84%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--color-surface-elevated) 12%, var(--color-surface));
    color: var(--color-text-primary);
    padding: 0 12px;
    font: inherit;
    outline: none;
    transition:
      border-color 120ms ease,
      opacity 120ms ease;
  }

  .field-input:focus {
    border-color: var(--color-focus);
  }

  .field-input:disabled {
    opacity: 0.46;
  }

  .field-input-mono {
    font-family: var(--font-mono, monospace);
    font-variant-numeric: tabular-nums;
  }

  .field-input option {
    background: var(--color-surface);
    color: var(--color-text-primary);
  }

  .field-hint,
  .field-error {
    line-height: 1.45;
    font-size: 0.72rem;
  }

  .field-hint {
    color: var(--color-muted);
  }

  .field-error {
    color: color-mix(in srgb, var(--color-danger) 68%, var(--color-text-primary));
  }

  .control-toggle-row {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 12px;
    padding: 2px 0;
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--color-text-secondary);
    font-size: 0.8rem;
    font-weight: 500;
  }

  .toggle input {
    margin: 0;
    width: 14px;
    height: 14px;
    accent-color: var(--color-primary);
  }

  .settings-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding-top: 12px;
    border-top: 1px solid color-mix(in srgb, var(--color-border) 76%, transparent);
  }

  .action-button {
    min-height: 34px;
    padding: 0 14px;
    color: var(--color-text-primary);
    background: color-mix(in srgb, var(--color-surface-elevated) 20%, var(--color-surface));
    font-size: 0.8rem;
  }

  .action-button:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--color-text-secondary) 34%, var(--color-border));
  }

  .action-button.action-button-secondary {
    color: var(--color-text-secondary);
  }

  .action-button.action-button-primary.primary {
    border-color: color-mix(in srgb, var(--color-primary) 30%, var(--color-border));
    background: color-mix(in srgb, var(--color-primary) 10%, var(--color-surface));
  }

  .inline-message,
  .empty-panel-state {
    padding: 0 0 0 12px;
    border: 0;
    border-left: 1px solid color-mix(in srgb, var(--color-border) 78%, transparent);
    background: transparent;
    color: var(--color-text-secondary);
    line-height: 1.5;
    font-size: 0.78rem;
  }

  .inline-message-warning {
    border-left-color: color-mix(in srgb, var(--color-warning) 32%, var(--color-border));
    color: color-mix(in srgb, var(--color-warning) 74%, var(--color-text-primary));
  }

  .inline-message-error {
    border-left-color: color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    color: color-mix(in srgb, var(--color-danger) 74%, var(--color-text-primary));
  }

  .inline-message-success {
    border-left-color: color-mix(in srgb, var(--color-success) 28%, var(--color-border));
    color: color-mix(in srgb, var(--color-success) 74%, var(--color-text-primary));
  }

  .empty-panel-state {
    display: grid;
    gap: 4px;
  }

  .empty-state-title {
    color: var(--color-text-primary);
    font-size: 0.86rem;
    font-weight: 600;
  }

  @media (max-width: 860px) {
    .settings-layer {
      justify-items: stretch;
      align-items: end;
    }

    .settings-panel {
      width: 100%;
      max-height: 92vh;
      height: auto;
      border-left: 0;
      border-top: 1px solid color-mix(in srgb, var(--color-border) 82%, transparent);
      border-radius: 14px 14px 0 0;
      padding: 20px;
    }

    .field-grid {
      grid-template-columns: 1fr;
    }

    .settings-header,
    .control-toggle-row,
    .settings-footer {
      flex-direction: column;
      align-items: stretch;
    }

    .settings-header-meta {
      justify-content: flex-start;
    }

    .settings-footer {
      justify-content: stretch;
    }

    .action-button {
      width: 100%;
    }
  }
</style>
