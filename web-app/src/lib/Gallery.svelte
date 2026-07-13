<script lang="ts">
  import { repository } from "../toys/repository";
  import type { ToySummary } from "../toys/types";

  let toys = $state<ToySummary[]>([]);
  $effect(() => {
    repository.list().then((t) => (toys = t));
  });

  let categories = $derived([...new Set(toys.map((t) => t.category))]);
  function open(slug: string) {
    location.hash = `t=${slug}`;
  }
</script>

<div class="mx-auto max-w-[1100px] p-[18px] flex flex-col gap-6">
  <h1 class="text-ink font-extrabold tracking-wide text-xl">Toys</h1>
  {#each categories as cat (cat)}
    <section class="flex flex-col gap-3">
      <h2 class="text-ink-dim text-sm uppercase tracking-wide">{cat}</h2>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        {#each toys.filter((t) => t.category === cat) as toy (toy.slug)}
          <button
            type="button"
            onclick={() => open(toy.slug)}
            class="text-left bg-panel border border-edge rounded-[10px] p-3 hover:border-n64-blue cursor-pointer flex flex-col gap-1"
          >
            <span class="text-ink font-semibold">{toy.title}</span>
            <span class="text-ink-dim text-xs leading-relaxed">{toy.description}</span>
            <span class="text-ink-faint text-[10px] mt-1">@{toy.owner.handle} · {toy.tags.join(", ")}</span>
          </button>
        {/each}
      </div>
    </section>
  {/each}
</div>
