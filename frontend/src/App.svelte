<script lang="ts">
  import { onMount } from 'svelte';
  import {
    fetchTorRuntimeSnapshot,
    fetchTorState,
    type TorRuntimeSnapshotDto,
    type TorStateDto,
  } from './lib/torq-api';

  let backendConnected = false;
  let state: TorStateDto | null = null;
  let snapshot: TorRuntimeSnapshotDto | null = null;
  let errorMessage = '';

  onMount(async () => {
    try {
      const [nextState, nextSnapshot] = await Promise.all([
        fetchTorState(),
        fetchTorRuntimeSnapshot(),
      ]);

      state = nextState;
      snapshot = nextSnapshot;
      backendConnected = true;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      backendConnected = false;
    }
  });
</script>

<svelte:head>
  <title>torq</title>
</svelte:head>

<main class="shell">
  <header>
    <h1>torq</h1>
    <p class:connected={backendConnected} class:disconnected={!backendConnected}>
      {backendConnected ? 'backend connected' : 'backend disconnected'}
    </p>
    <p class="placeholder">
      Read-only bootstrap shell: snapshot commands are wired, interactive controls come later.
    </p>
  </header>

  <section class="panel">
    <h2>TorState</h2>
    {#if state}
      <pre>{JSON.stringify(state, null, 2)}</pre>
    {:else}
      <p class="placeholder">Waiting for backend state.</p>
    {/if}
  </section>

  <section class="panel">
    <h2>TorRuntimeSnapshot</h2>
    {#if snapshot}
      <pre>{JSON.stringify(snapshot, null, 2)}</pre>
    {:else}
      <p class="placeholder">Waiting for runtime snapshot.</p>
    {/if}
  </section>

  {#if errorMessage}
    <section class="panel error">
      <h2>Load error</h2>
      <pre>{errorMessage}</pre>
    </section>
  {/if}
</main>
