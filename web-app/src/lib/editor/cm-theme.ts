import { EditorView } from "@codemirror/view";

export const n64Theme = EditorView.theme(
  {
    "&": {
      color: "var(--color-ink)",
      backgroundColor: "#0b0813",
      fontSize: "12.5px",
      height: "100%",
    },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
    ".cm-content": { caretColor: "var(--color-n64-yellow)" },
    "&.cm-focused": { outline: "none" },
    ".cm-gutters": {
      backgroundColor: "#0a0712",
      color: "var(--color-ink-faint)",
      border: "none",
      borderRight: "1px solid var(--color-edge)",
    },
    ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.03)" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--color-ink-dim)" },
    ".cm-cursor": { borderLeftColor: "var(--color-n64-yellow)" },
    ".cm-selectionBackground, ::selection": { backgroundColor: "rgba(61,48,82,0.6)" },
    "&.cm-focused .cm-selectionBackground": { backgroundColor: "rgba(61,48,82,0.85)" },
    ".cm-lintRange-error": {
      textDecoration: "underline wavy var(--color-n64-red)",
      textDecorationSkipInk: "none",
    },
  },
  { dark: true },
);
