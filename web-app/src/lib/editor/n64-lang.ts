import { StreamLanguage, HighlightStyle, syntaxHighlighting, LanguageSupport } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const DECL_KEYWORDS = new Set(["Texture", "Mtx", "Vp", "Vtx", "Gfx"]);
const BUILTINS = new Set(["scale", "identity", "translate"]);
const BOOLS = new Set(["true", "false"]);

export type TokenType =
  | "declKeyword"
  | "macro"
  | "atom"
  | "number"
  | "comment"
  | "punctuation"
  | "variableName";

/** Pure classification of a bare identifier word (unit-tested). */
export function classifyWord(w: string): TokenType {
  if (DECL_KEYWORDS.has(w)) return "declKeyword";
  if (/^gs[A-Z]/.test(w) || BUILTINS.has(w)) return "macro";
  if (BOOLS.has(w)) return "atom";
  if (/^[A-Z][A-Z0-9_]*$/.test(w)) return "atom"; // ALL-CAPS constants
  return "variableName";
}

const n64StreamParser = StreamLanguage.define<{}>({
  name: "n64gbi",
  startState: () => ({}),
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match(/\/\/.*/)) return "comment";
    if (stream.match(/0x[0-9a-fA-F]+/)) return "number";
    if (stream.match(/-?\d+(?:\.\d+)?/)) return "number";

    const id = stream.match(/[A-Za-z_][A-Za-z0-9_]*/);
    if (id) {
      return classifyWord((id as RegExpMatchArray)[0]);
    }

    if (stream.match(/[{}(),|=]/)) return "punctuation";
    stream.next();
    return null;
  },
  tokenTable: {
    declKeyword: t.keyword,
    macro: t.macroName,
    atom: t.atom,
    number: t.number,
    comment: t.lineComment,
    punctuation: t.punctuation,
    variableName: t.variableName,
  },
});

export const n64HighlightStyle = HighlightStyle.define([
  { tag: t.keyword, color: "var(--color-n64-yellow)" },
  { tag: t.macroName, color: "var(--color-n64-blue)" },
  { tag: t.atom, color: "var(--color-n64-green)" },
  { tag: t.number, color: "#ff8a5c" },
  { tag: t.lineComment, color: "var(--color-ink-faint)", fontStyle: "italic" },
  { tag: t.punctuation, color: "var(--color-ink-dim)" },
  { tag: t.variableName, color: "var(--color-ink)" },
]);

export function n64Language(): LanguageSupport {
  return new LanguageSupport(n64StreamParser);
}

export const n64Highlighting = syntaxHighlighting(n64HighlightStyle);
