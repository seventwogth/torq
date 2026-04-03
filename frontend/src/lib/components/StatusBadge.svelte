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
    gap: var(--space-2, 8px);
    min-height: 26px;
    padding: 0 var(--space-3, 12px);
    border-radius: var(--radius-lg, 10px);
    border: 1px solid color-mix(in srgb, var(--tone) 18%, var(--color-border));
    background: color-mix(in srgb, var(--tone) 7%, var(--color-surface));
    color: var(--tone);
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    font-size: 0.76rem;
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }

  .dot {
    width: 6px;
    height: 6px;
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
