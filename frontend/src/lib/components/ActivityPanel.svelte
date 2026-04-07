<script lang="ts">
  import type { ActivityTone } from '../torq-api';

  interface ActivityEntry {
    id: string;
    timestamp: number;
    tone: ActivityTone;
    title: string;
    details?: string;
    coalesceKey?: 'bootstrap';
  }

  export let activitySubscriptionError = '';
  export let activityEntries: ActivityEntry[] = [];
  export let activityEmptyMessage = '';
  export let formatActivityTime: (timestamp: number) => string = () => '';
</script>

<section class="app-section activity-panel" aria-label="Tor runtime activity">
  <div class="section-heading">
    <div class="section-heading-copy">
      <p class="section-kicker">Activity</p>
      <h2>Recent events</h2>
    </div>
    <p>Recent runtime events from the desktop backend.</p>
  </div>

  {#if activitySubscriptionError}
    <p class="panel-note panel-note-error">{activitySubscriptionError}</p>
  {/if}

  <div class={`activity-feed ${activityEntries.length ? 'has-entries' : 'is-empty'}`}>
    {#if activityEntries.length}
      <ul class="activity-list">
        {#each activityEntries as entry}
          <li class={`activity-item tone-${entry.tone}`}>
            <span class="activity-marker" aria-hidden="true"></span>
            <div class="activity-copy">
              <div class="activity-headline">
                <strong class="activity-title">{entry.title}</strong>
                <time class="activity-time" datetime={new Date(entry.timestamp).toISOString()}>
                  {formatActivityTime(entry.timestamp)}
                </time>
              </div>
              {#if entry.details}
                <p class="activity-details">{entry.details}</p>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty-state activity-empty-state">{activityEmptyMessage}</p>
    {/if}
  </div>
</section>
