<script lang="ts">
  import type { Diagnostic } from "./playground.svelte";

  let { diagnostics }: { diagnostics: Diagnostic[] } = $props();
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden">
  <div class="text-[11px] uppercase tracking-wide text-ink-faint px-3 py-2 border-b border-edge flex items-center justify-between">
    <span>Diagnostics</span>
    {#if diagnostics.length}
      <span class="text-[10px] text-ink-faint bg-raised border border-edge rounded-full px-2 py-0.5">{diagnostics.length}</span>
    {/if}
  </div>
  <div class="p-3 flex flex-col gap-2">
    {#if diagnostics.length === 0}
      <div class="text-n64-green text-xs">✓ no diagnostics</div>
    {:else}
      {#each diagnostics as d (`${d.line}:${d.msg}`)}
        <div class="flex gap-2.5 text-xs bg-raised rounded-md px-2.5 py-1.5 border-l-[3px] border-n64-red">
          <span class="text-n64-yellow shrink-0">line {d.line}</span>
          <span class="text-ink">{d.msg}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>
