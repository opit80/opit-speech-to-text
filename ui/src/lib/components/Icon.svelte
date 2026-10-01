<script lang="ts">
  interface Props {
    name: "mic" | "stop" | "x" | "retry" | "copy" | "trash" | "play" | "pause" | "search" | "home" | "history" | "rules" | "profiles" | "settings" | "check" | "warning" | "plus" | "key" | "wand";
    size?: number;
  }
  let { name, size = 16 }: Props = $props();
  const paths: Record<Props["name"], string> = {
    mic: "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0ZM5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8",
    stop: "M5 5h14v14H5Z",
    x: "m6 6 12 12M6 18 18 6",
    retry: "M3 10a9 9 0 1 1 1 7M3 4v6h6",
    copy: "M9 9h12v12H9ZM5 15H3V3h12v2",
    trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7",
    play: "m7 4 14 8-14 8Z",
    pause: "M6 4h4v16H6ZM14 4h4v16h-4Z",
    search: "M19 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0Zm-2 7 5 5",
    home: "m3 10 9-7 9 7M5 9v12h14V9M9 21v-7h6v7",
    history: "M3 10a9 9 0 1 1 1 7M3 4v6h6M12 7v5l3 2",
    rules: "M8 6h13M8 12h13M8 18h13M3 6h1M3 12h1M3 18h1",
    profiles: "M15 7a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM5 21v-3a7 7 0 0 1 14 0v3M19 4a3 3 0 0 1 0 6M21 14a5 5 0 0 1 2 4v3",
    settings: "M4 6h16M4 12h16M4 18h16M8 3v6M16 9v6M10 15v6",
    check: "m4 12 5 5L20 6",
    warning: "m12 3 10 18H2ZM12 9v5M12 17v1",
    plus: "M12 4v16M4 12h16",
    key: "M11 8a4 4 0 1 1-8 0 4 4 0 0 1 8 0Zm-1 3 10 10M16 17l3-3M19 20l3-3",
    wand: "m4 20 13-13 3 3L7 23ZM14 10l3 3M5 3v6M2 6h6M19 1v4M17 3h4",
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d={paths[name]} />
</svg>

<style>
  svg { display: inline-block; flex-shrink: 0; vertical-align: middle; }
</style>
