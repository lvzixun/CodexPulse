<script lang="ts">
  let { name }: { name: string } = $props();
  const paths: Record<string, string> = {
    activity: 'M2 12h4l3-8 6 16 3-8h4',
    down: 'm6 9 6 6 6-6',
    up: 'm6 15 6-6 6 6',
    right: 'm9 6 6 6-6 6',
    arrow: 'M7 17 17 7M7 7h10v10',
    external: 'M15 3h6v6m0-6-9 9M9 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-4',
    pin: 'm16 3 5 5-4 1-2 5 2 2-1 1-5-2-1 4-5-5 4-1 2-5-1-5ZM3 21l5-5',
    settings: 'M4 7h16M4 17h16M8 4v6m8 4v6',
    radio:
      'M5 5a10 10 0 0 0 0 14m14-14a10 10 0 0 1 0 14M8 8a6 6 0 0 0 0 8m8-8a6 6 0 0 1 0 8M12 11v2',
    bell: 'M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4',
  };
</script>

<svg
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name] ?? paths.activity} /></svg
>
