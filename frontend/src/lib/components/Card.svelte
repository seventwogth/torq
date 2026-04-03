<script lang="ts">
  export let title: string;
  export let subtitle = '';
</script>

<section class="card">
  <header class="card-header">
    <h2>{title}</h2>
    {#if subtitle}
      <p>{subtitle}</p>
    {/if}
  </header>

  <div class="card-body">
    <slot />
  </div>
</section>

<style>
  .card {
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, var(--color-border) 88%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--color-surface-elevated) 18%, var(--color-surface));
    padding: 16px;
    display: grid;
    gap: 14px;
  }

  .card-header {
    display: grid;
    gap: 4px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    font-size: 0.9rem;
    line-height: 1.35;
    letter-spacing: 0.01em;
    font-weight: 500;
    color: var(--color-text-primary);
  }

  p {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    color: var(--color-muted);
    font-size: 0.8rem;
    line-height: 1.55;
  }

  .card-body {
    display: grid;
    gap: 12px;
  }
</style>
