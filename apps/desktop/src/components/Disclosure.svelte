<script lang="ts">
  import type { Snippet } from 'svelte';
  import { disclosure } from '../lib/disclosure';
  import { reducedMotion } from '../lib/motion';

  let {
    id,
    open,
    contentReady = true,
    settled,
    closed,
    children,
  }: {
    id: string;
    open: boolean;
    contentReady?: boolean;
    settled?: () => void;
    closed?: () => void;
    children: Snippet;
  } = $props();
  let visible = $state(false);
  $effect(() => {
    if (open) visible = true;
  });
  function finish(expanded: boolean) {
    if (expanded !== open) return;
    if (!expanded) {
      visible = false;
      closed?.();
    } else if (contentReady) settled?.();
  }
</script>

{#if visible}
  <div
    {id}
    class="cp-disclosure"
    inert={!open}
    aria-hidden={open ? undefined : true}
    use:disclosure={{ open, reduced: reducedMotion.current, contentReady, settled: finish }}
  >
    <div class="cp-disclosure-content">{@render children()}</div>
  </div>
{/if}

<style>
  .cp-disclosure {
    overflow: hidden;
  }
  .cp-disclosure-content {
    display: flow-root;
  }
</style>
