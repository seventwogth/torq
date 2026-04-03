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
    border: 1px solid color-mix(in srgb, var(--color-border) 76%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--color-surface-elevated) 10%, var(--color-surface));
    padding: 12px;
    display: grid;
    gap: 12px;
  }

  .card-header {
    display: grid;
    gap: 4px;
    padding-bottom: 12px;
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    font-size: 0.86rem;
    line-height: 1.35;
    letter-spacing: 0.01em;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  p {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    color: var(--color-muted);
    font-size: 0.78rem;
    line-height: 1.55;
  }

  .card-body {
    display: grid;
    gap: 12px;
  }
</style>
