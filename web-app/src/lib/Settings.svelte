<script lang="ts">
  import { Switch, Select } from "bits-ui";
  import type { Settings } from "./playground.svelte";

  let { settings = $bindable() }: { settings: Settings } = $props();

  const microcodes = [{ value: "F3DEX2", label: "F3DEX2" }];
  const formats = [
    { value: "RGBA16", label: "RGBA16" },
    { value: "RGBA32", label: "RGBA32" },
  ];

  const triggerCls =
    "bg-raised border border-edge rounded-md px-3 py-1.5 text-ink text-xs flex items-center gap-2 min-w-[110px] justify-between cursor-pointer";
  const contentCls =
    "bg-panel border border-edge rounded-md py-1 text-xs text-ink shadow-lg z-50";
  const itemCls =
    "px-3 py-1.5 cursor-pointer data-[highlighted]:bg-raised data-[selected]:text-n64-yellow";
  const switchCls =
    "w-[38px] h-[20px] rounded-full bg-edge-hi data-[state=checked]:bg-n64-green relative transition-colors cursor-pointer";
  const thumbCls =
    "block w-[16px] h-[16px] rounded-full bg-white absolute top-[2px] left-[2px] data-[state=checked]:translate-x-[18px] transition-transform";
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden">
  <div class="text-[11px] uppercase tracking-wide text-ink-faint px-3 py-2 border-b border-edge">Settings</div>

  <div class="flex items-center justify-between px-3 py-2.5 border-b border-edge">
    <span id="microcode-label" class="text-ink-dim text-xs">Microcode</span>
    <Select.Root type="single" bind:value={settings.microcode} items={microcodes}>
      <Select.Trigger aria-labelledby="microcode-label" class={triggerCls}>{settings.microcode} ▾</Select.Trigger>
      <Select.Portal>
        <Select.Content class={contentCls} sideOffset={6}>
          {#each microcodes as m (m.value)}
            <Select.Item class={itemCls} value={m.value} label={m.label}>{m.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  </div>

  <div class="flex items-center justify-between px-3 py-2.5 border-b border-edge">
    <span id="colorformat-label" class="text-ink-dim text-xs">Color format</span>
    <Select.Root type="single" bind:value={settings.colorFormat} items={formats}>
      <Select.Trigger aria-labelledby="colorformat-label" class={triggerCls}>{settings.colorFormat} ▾</Select.Trigger>
      <Select.Portal>
        <Select.Content class={contentCls} sideOffset={6}>
          {#each formats as f (f.value)}
            <Select.Item class={itemCls} value={f.value} label={f.label}>{f.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  </div>

  <div class="flex items-center justify-between px-3 py-2.5">
    <span class="text-ink-dim text-xs">Show wireframe</span>
    <Switch.Root bind:checked={settings.wireframe} class={switchCls}>
      <Switch.Thumb class={thumbCls} />
    </Switch.Root>
  </div>
</div>
