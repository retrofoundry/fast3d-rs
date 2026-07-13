<script lang="ts">
  import { Play, Pause, RotateCcw, Maximize } from "@lucide/svelte";
  import type { Playground } from "./playground.svelte";

  let { pg, canvas }: { pg: Playground; canvas?: HTMLCanvasElement } = $props();

  function fullscreen() {
    canvas?.requestFullscreen?.();
  }
  // filled-track percentage for the scrubber
  const pct = $derived(Math.max(0, Math.min(100, (pg.time / pg.scrubMax) * 100)));
</script>

<div class="border-t border-edge">
  <!-- Top row: info + fullscreen (always) -->
  <div class="flex items-center gap-3 px-3.5 py-2.5">
    <span class="text-[10px] text-ink-dim border border-edge rounded-full px-2 py-0.5 tracking-wide">{pg.settings.microcode}</span>
    <span class="text-xs {pg.errored ? 'text-n64-red' : 'text-ink-dim'}">{pg.status}</span>
    <span class="ml-auto text-xs text-ink-dim tabular-nums">640×480</span>
    <button
      type="button"
      onclick={fullscreen}
      title="Fullscreen"
      aria-label="Fullscreen"
      class="w-7 h-7 rounded-md border border-edge bg-raised text-ink-dim flex items-center justify-center hover:text-ink hover:border-edge-hi cursor-pointer"
    ><Maximize size={15} strokeWidth={2} /></button>
  </div>

  <!-- Bottom row: transport (animated only) -->
  {#if pg.isAnimated}
    <div class="flex items-center gap-3 px-3.5 py-2.5 border-t border-edge">
      {#if pg.playing}
        <button type="button" onclick={() => pg.pause()} title="Pause" aria-label="Pause"
          class="w-8 h-8 rounded-full bg-n64-red text-white flex items-center justify-center shadow-[0_2px_10px_rgba(228,0,43,0.5)] hover:brightness-110 cursor-pointer">
          <Pause size={15} fill="currentColor" strokeWidth={0} /></button>
      {:else}
        <button type="button" onclick={() => pg.play()} title="Play" aria-label="Play"
          class="w-8 h-8 rounded-full bg-n64-red text-white flex items-center justify-center shadow-[0_2px_10px_rgba(228,0,43,0.5)] hover:brightness-110 cursor-pointer pl-0.5">
          <Play size={15} fill="currentColor" strokeWidth={0} /></button>
      {/if}
      <button type="button" onclick={() => pg.reset()} title="Reset" aria-label="Reset"
        class="w-8 h-8 rounded-full border border-n64-green/50 bg-n64-green/10 text-n64-green flex items-center justify-center hover:bg-n64-green/20 cursor-pointer">
        <RotateCcw size={15} strokeWidth={2} /></button>
      <input
        class="scrub flex-1"
        type="range" min="0" max={pg.scrubMax} step="0.01" value={pg.time}
        style="--pct:{pct}%"
        oninput={(e) => pg.seek(parseFloat((e.currentTarget as HTMLInputElement).value))}
        aria-label="Time"
      />
      <div class="text-right leading-tight tabular-nums w-14 shrink-0">
        <div class="text-xs text-ink">{pg.time.toFixed(2)}s</div>
        {#if pg.playing}<div class="text-[10px] text-ink-dim">{pg.fps} fps</div>{/if}
      </div>
    </div>
  {/if}
</div>

<style>
  /* Styled scrubber: rounded track, accent-blue fill up to the thumb, white thumb. */
  .scrub {
    -webkit-appearance: none;
    appearance: none;
    height: 16px;
    background: transparent;
    cursor: pointer;
  }
  .scrub::-webkit-slider-runnable-track {
    height: 5px;
    border-radius: 3px;
    background: linear-gradient(to right, var(--color-n64-blue) 0% var(--pct), var(--color-edge) var(--pct) 100%);
  }
  .scrub::-moz-range-track { height: 5px; border-radius: 3px; background: var(--color-edge); }
  .scrub::-moz-range-progress { height: 5px; border-radius: 3px; background: var(--color-n64-blue); }
  .scrub::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 13px; height: 13px; margin-top: -4px;
    border-radius: 50%; background: #fff;
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-n64-blue) 25%, transparent), 0 1px 4px rgba(0,0,0,.5);
  }
  .scrub::-moz-range-thumb {
    width: 13px; height: 13px; border: none;
    border-radius: 50%; background: #fff;
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-n64-blue) 25%, transparent), 0 1px 4px rgba(0,0,0,.5);
  }
</style>
