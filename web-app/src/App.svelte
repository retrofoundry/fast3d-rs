<script lang="ts">
  import { onMount } from "svelte";
  import { Playground } from "./lib/playground.svelte";
  import { repository } from "./toys/repository";
  import TopNav from "./lib/TopNav.svelte";
  import Gallery from "./lib/Gallery.svelte";
  import Viewport from "./lib/Viewport.svelte";
  import Editor from "./lib/Editor.svelte";
  import TextureInputs from "./lib/TextureInputs.svelte";
  import ToyMeta from "./lib/ToyMeta.svelte";
  import Diagnostics from "./lib/Diagnostics.svelte";
  import Settings from "./lib/Settings.svelte";
  const pg = new Playground();
  let canvasEl = $state<HTMLCanvasElement>();

  function slugFromHash(): string | null {
    const m = location.hash.match(/^#t=(.+)$/);
    return m ? m[1] : null;
  }
  let slug = $state(slugFromHash());
  const inEditor = $derived(slug !== null);

  onMount(() => {
    const onHash = () => (slug = slugFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  });

  // When entering the editor: init the renderer once (canvas now exists), then load the toy.
  // When leaving the editor: tear down the animation loop so it doesn't run in the background.
  $effect(() => {
    if (!inEditor) {
      pg.teardown();
      return;
    }
    if (!canvasEl) return;
    const wanted = slug;
    pg.init(canvasEl)
      .then(async () => {
        if (!wanted) return;
        const toy = await repository.get(wanted);
        if (slugFromHash() !== wanted) return; // navigated away while loading — drop the stale result
        if (toy) await pg.loadToy(toy);
        else location.hash = "";
      })
      .catch((e) => (pg.status = `init failed: ${e instanceof Error ? e.message : String(e)}`));
  });
</script>

<div class="min-h-screen bg-base text-ink font-sans">
  <TopNav />
  <!-- Gallery and editor both stay mounted; visibility toggles so the canvas/renderer survive. -->
  <div class:hidden={inEditor}>
    <Gallery />
  </div>
  <main class:hidden={!inEditor} class="mx-auto max-w-[1280px] grid grid-cols-1 lg:grid-cols-[1fr_1.15fr] gap-[18px] p-[18px]">
    <div class="flex flex-col gap-[18px]">
      <button type="button" onclick={() => (location.hash = "")} class="self-start text-ink-dim hover:text-ink text-sm cursor-pointer">← Browse</button>
      <Viewport {pg} bind:canvas={canvasEl} />
      <TextureInputs textures={pg.textures} onupload={(f) => pg.loadTexture(f)} />
      <ToyMeta bind:title={pg.title} bind:description={pg.description} />
      <Diagnostics diagnostics={pg.diags} />
    </div>
    <div class="flex flex-col gap-[18px]">
      <Editor bind:value={pg.source} diagnostics={pg.diags} onrun={() => pg.run()} oninput={() => pg.scheduleAutoRun()} bind:autoRun={pg.settings.autoRun} />
      <Settings bind:settings={pg.settings} />
    </div>
  </main>
</div>
