<script lang="ts">
  export let label: string;
  export let tone: 'success' | 'warning' | 'danger' | 'neutral' | 'muted' = 'neutral';
</script>

<span class={`badge tone-${tone}`}>
  <span class="dot" aria-hidden="true"></span>
  <span>{label}</span>
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 22px;
    padding: 0 9px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--tone) 10%, var(--color-border));
    background: color-mix(in srgb, var(--tone) 3%, var(--color-surface));
    color: color-mix(in srgb, var(--tone) 68%, var(--color-text-secondary));
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    font-size: 0.7rem;
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }

  .dot {
    width: 4px;
    height: 4px;
    border-radius: 999px;
    background: currentColor;
  }

  .tone-success {
    --tone: var(--color-success);
  }

  .tone-warning {
    --tone: var(--color-warning);
  }

  .tone-danger {
    --tone: var(--color-danger);
  }

  .tone-neutral {
    --tone: var(--color-neutral);
  }

  .tone-muted {
    --tone: var(--color-text-secondary);
  }
</style>
