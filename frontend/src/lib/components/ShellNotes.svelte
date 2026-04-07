<script lang="ts">
  export let controlHintMessage = '';
  export let newIdentityHintMessage = '';
  export let actionErrorMessage = '';
  export let eventErrorMessage = '';
</script>

<section class="shell-notes" aria-live="polite">
  {#if controlHintMessage}
    <p class="inline-message inline-message-muted">{controlHintMessage}</p>
  {/if}

  {#if newIdentityHintMessage && newIdentityHintMessage !== controlHintMessage}
    <p class="inline-message inline-message-muted">{newIdentityHintMessage}</p>
  {/if}

  {#if actionErrorMessage}
    <p class="inline-message inline-message-error">{actionErrorMessage}</p>
  {/if}

  {#if eventErrorMessage}
    <p class="inline-message inline-message-muted">{eventErrorMessage}</p>
  {/if}
</section>
