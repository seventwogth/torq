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
    gap: 8px;
    min-height: 32px;
    padding: 0 12px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--tone) 28%, transparent);
    background: color-mix(in srgb, var(--tone) 10%, transparent);
    color: var(--tone);
    font-size: 0.875rem;
    font-weight: 600;
    line-height: 1;
    white-space: nowrap;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 999px;
    background: currentColor;
    box-shadow: 0 0 0 3px color-mix(in srgb, currentColor 18%, transparent);
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
    --tone: var(--color-text-secondary);
  }

  .tone-muted {
    --tone: var(--color-text-secondary);
  }
</style>
