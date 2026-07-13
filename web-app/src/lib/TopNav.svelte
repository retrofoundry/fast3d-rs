<nav class="flex items-center gap-[22px] px-5 py-3 border-b border-edge bg-gradient-to-b from-panel to-[#100c1a]">
  <span class="flex items-center gap-2 font-extrabold tracking-wider text-[15px] text-ink">
    <span class="flex gap-1">
      <span class="w-[9px] h-[9px] rounded-full bg-n64-red"></span>
      <span class="w-[9px] h-[9px] rounded-full bg-n64-green"></span>
      <span class="w-[9px] h-[9px] rounded-full bg-n64-blue"></span>
      <span class="w-[9px] h-[9px] rounded-full bg-n64-yellow"></span>
    </span>
    N64-TOYS
  </span>
  <button type="button" onclick={() => (location.hash = "t=onetri")} class="text-ink-dim hover:text-ink text-sm cursor-pointer">new</button>
  <button type="button" onclick={() => (location.hash = "")} class="text-ink-dim hover:text-ink text-sm cursor-pointer">browse</button>
  <div class="ml-auto bg-raised border border-edge rounded-md px-3 py-1.5 text-ink-faint text-sm min-w-[240px]">
    Search…
  </div>
</nav>
