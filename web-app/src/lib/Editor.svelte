<script lang="ts">
  import { onMount } from "svelte";
  import { RefreshCw } from "@lucide/svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { forceLinting } from "@codemirror/lint";
  import { n64Language, n64Highlighting } from "./editor/n64-lang";
  import { n64Theme } from "./editor/cm-theme";
  import { n64Lint, setDiagsEffect } from "./editor/lint";
  import type { Diagnostic } from "./playground.svelte";

  let {
    value = $bindable(),
    diagnostics,
    onrun,
    oninput,
    autoRun = $bindable(false),
  }: {
    value: string;
    diagnostics: Diagnostic[];
    onrun: () => void;
    oninput?: () => void;
    autoRun?: boolean;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          n64Language(),
          n64Highlighting,
          n64Theme,
          n64Lint(),
          keymap.of([
            { key: "Mod-Enter", preventDefault: true, run: () => { onrun(); return true; } },
            indentWithTab,
            ...defaultKeymap,
            ...historyKeymap,
          ]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              value = u.state.doc.toString();
              oninput?.();
            }
          }),
        ],
      }),
    });
    return () => view?.destroy();
  });

  // Push external value changes into the editor.
  $effect(() => {
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
    }
  });

  // Re-lint when diagnostics change. (First run is pre-mount and no-ops; diags start empty.)
  $effect(() => {
    const d = diagnostics;
    if (view) {
      view.dispatch({ effects: setDiagsEffect.of(d) });
      forceLinting(view);
    }
  });
</script>

<div class="bg-panel border border-edge rounded-[10px] overflow-hidden flex flex-col">
  <div class="text-[11px] uppercase tracking-wide text-ink-faint px-3 py-2 border-b border-edge flex items-center justify-between">
    <span>Source · gbi-macro</span>
    <span class="text-[10px] text-ink-faint bg-raised border border-edge rounded-full px-2 py-0.5">⌘↵ run</span>
  </div>
  <div bind:this={host} class="flex-1 min-h-[420px] overflow-auto"></div>
  <div class="flex items-center gap-3 px-3.5 py-2.5 border-t border-edge">
    <button
      type="button"
      onclick={onrun}
      disabled={autoRun}
      class="flex items-center gap-1.5 bg-n64-red text-white font-bold rounded-md px-3.5 py-1.5 text-xs shadow-[0_0_12px_rgba(228,0,43,0.45)] hover:brightness-110 cursor-pointer disabled:opacity-40 disabled:cursor-default disabled:shadow-none"
    ><RefreshCw size={13} strokeWidth={2.2} /> Run</button>
    <button
      type="button"
      aria-pressed={autoRun}
      onclick={() => (autoRun = !autoRun)}
      title={autoRun ? "Hot reload on — re-runs on every edit" : "Hot reload off — press Run to apply edits"}
      class="ml-auto group flex items-center gap-2 rounded-md px-2 py-1 text-xs cursor-pointer hover:bg-white/5"
    >
      <span class="w-2 h-2 rounded-full {autoRun ? 'bg-n64-yellow shadow-[0_0_7px_1px_rgba(255,210,0,0.75)]' : 'bg-ink-faint'}"></span>
      <span class="text-ink-dim group-hover:text-ink">hot reload</span>
      <span class="{autoRun ? 'text-n64-yellow' : 'text-ink-dim'}">{autoRun ? "on" : "off"}</span>
    </button>
  </div>
</div>
