import { StateEffect, StateField, type Extension } from "@codemirror/state";
import { linter, lintGutter, type Diagnostic as CMDiagnostic } from "@codemirror/lint";
import type { Diagnostic } from "../playground.svelte";

// Named `setDiagsEffect` (not `setDiagnostics`) to avoid shadowing @codemirror/lint's
// own `setDiagnostics` export, which is a different API.
export const setDiagsEffect = StateEffect.define<Diagnostic[]>();

const diagnosticsField = StateField.define<Diagnostic[]>({
  create: () => [],
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setDiagsEffect)) return e.value;
    return value;
  },
});

const n64Linter = linter((view): CMDiagnostic[] => {
  const diags = view.state.field(diagnosticsField);
  const lines = view.state.doc.lines;
  return diags.map((d) => {
    const n = Math.min(Math.max(d.line, 1), lines);
    const line = view.state.doc.line(n);
    return { from: line.from, to: line.to, severity: "error" as const, message: d.msg };
  });
});

export function n64Lint(): Extension {
  return [diagnosticsField, n64Linter, lintGutter()];
}
