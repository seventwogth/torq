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
    border: 1px solid color-mix(in srgb, var(--color-border) 92%, transparent);
    border-radius: var(--radius-lg, 16px);
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--color-surface) 96%, white 4%),
        var(--color-surface)
      );
    padding: var(--card-padding, 20px);
    display: grid;
    gap: var(--space-4, 16px);
    box-shadow: 0 1px 0 color-mix(in srgb, white 3%, transparent) inset;
  }

  .card-header {
    display: grid;
    gap: 6px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    font-size: 0.84rem;
    line-height: 1.25;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  p {
    font-family: var(--font-ui, 'Manrope', 'Segoe UI', sans-serif);
    color: var(--color-text-secondary);
    font-size: 0.92rem;
    line-height: 1.55;
  }

  .card-body {
    display: grid;
    gap: var(--space-3, 12px);
  }
</style>
