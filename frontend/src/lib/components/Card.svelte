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
    border-radius: 16px;
    background: var(--color-surface);
    padding: 18px;
    display: grid;
    gap: 16px;
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
    font-size: 0.95rem;
    line-height: 1.2;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--color-text-primary);
  }

  p {
    color: var(--color-text-secondary);
    font-size: 0.95rem;
    line-height: 1.4;
  }

  .card-body {
    display: grid;
    gap: 14px;
  }
</style>
