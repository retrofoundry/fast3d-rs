<script lang="ts">
  let {
    title = $bindable(),
    description = $bindable(),
  }: { title: string; description: string } = $props();
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden">
  <div class="text-[11px] uppercase tracking-wide text-ink-faint px-3 py-2 border-b border-edge">Toy</div>
  <div class="p-3 flex flex-col gap-2">
    <input
      bind:value={title}
      placeholder="Toy title"
      aria-label="Toy title"
      class="bg-transparent text-ink font-semibold text-[17px] outline-none border-b border-transparent focus:border-edge font-sans"
    />
    <textarea
      bind:value={description}
      rows="2"
      placeholder="Description"
      aria-label="Toy description"
      class="bg-transparent text-ink-dim text-xs resize-none outline-none leading-relaxed font-sans"
    ></textarea>
  </div>
</div>
