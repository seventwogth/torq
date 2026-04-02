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
    border: 1px solid var(--color-border);
    border-radius: 18px;
    background: var(--color-surface);
    padding: 20px;
    display: grid;
    gap: 18px;
  }

  .card-header {
    display: grid;
    gap: 8px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-size: 0.9rem;
    line-height: 1.2;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-text-primary);
  }

  p {
    color: var(--color-text-secondary);
    font-size: 0.95rem;
    line-height: 1.5;
  }

  .card-body {
    display: grid;
    gap: 12px;
  }
</style>
