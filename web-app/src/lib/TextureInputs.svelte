<script lang="ts">
  import type { Texture } from "./playground.svelte";

  let { textures, onupload }: { textures: Texture[]; onupload: (f: File) => void } = $props();

  function onchange(e: Event) {
    const input = e.target as HTMLInputElement;
    const f = input.files?.[0];
    if (f) {
      onupload(f);
      input.value = ""; // reset so re-selecting the same file fires onchange again
    }
  }
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden">
  <div class="text-[11px] uppercase tracking-wide text-ink-faint px-3 py-2 border-b border-edge">
    Texture inputs
  </div>
  <div class="flex gap-3 p-3">
    {#each textures as tex, i (tex.url)}
      <div class="w-[84px] text-center">
        <img
          src={tex.url}
          alt={`texture ${i}`}
          class="w-[84px] h-[84px] rounded-lg border border-edge-hi object-cover [image-rendering:pixelated]"
        />
        <div class="text-[10px] text-ink-faint mt-1">tex{i}</div>
      </div>
    {/each}
    <label class="w-[84px] text-center cursor-pointer">
      <div
        class="w-[84px] h-[84px] rounded-lg border border-dashed border-edge-hi text-ink-dim text-2xl flex items-center justify-center hover:text-ink"
      >+</div>
      <div class="text-[10px] text-ink-faint mt-1">upload</div>
      <input type="file" accept="image/png" onchange={onchange} class="hidden" />
    </label>
  </div>
</div>
