<script lang="ts">
  import type { Playground } from "./playground.svelte";
  import PlayerBar from "./PlayerBar.svelte";

  let { pg, canvas = $bindable() }: { pg: Playground; canvas?: HTMLCanvasElement } = $props();
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden">
  <div class="bg-[#070409] flex items-center justify-center aspect-[4/3]">
    <canvas bind:this={canvas} width="640" height="480" class="max-w-full max-h-full"></canvas>
  </div>
  <PlayerBar {pg} {canvas} />
</div>
