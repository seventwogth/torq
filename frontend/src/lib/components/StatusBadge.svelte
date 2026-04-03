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
    min-height: 18px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--tone);
    font-family: var(--font-ui, 'Segoe UI', system-ui, sans-serif);
    font-size: 0.75rem;
    font-weight: 500;
    line-height: 1.2;
    white-space: nowrap;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: currentColor;
    opacity: 0.9;
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
