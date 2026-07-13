import { describe, it, expect } from "vitest";
import { classifyWord } from "./n64-lang";

describe("classifyWord", () => {
  it("classifies declaration keywords", () => {
    for (const w of ["Texture", "Mtx", "Vp", "Vtx", "Gfx"]) {
      expect(classifyWord(w)).toBe("declKeyword");
    }
  });

  it("classifies gs* macros and matrix builtins", () => {
    expect(classifyWord("gsSPMatrix")).toBe("macro");
    expect(classifyWord("gsDPSetCombineLERP")).toBe("macro");
    expect(classifyWord("scale")).toBe("macro");
    expect(classifyWord("identity")).toBe("macro");
    expect(classifyWord("translate")).toBe("macro");
  });

  it("classifies ALL-CAPS constants and booleans as atoms", () => {
    for (const w of ["G_MTX_PROJECTION", "RGBA16", "TEXEL0", "SHADE", "G_ON", "ZERO"]) {
      expect(classifyWord(w)).toBe("atom");
    }
    expect(classifyWord("true")).toBe("atom");
    expect(classifyWord("false")).toBe("atom");
  });

  it("classifies user identifiers as variableName", () => {
    for (const w of ["proj", "model", "tex", "verts", "vp"]) {
      expect(classifyWord(w)).toBe("variableName");
    }
  });

  it("treats mixed-case non-keywords as variableName, not constants", () => {
    // ALL-CAPS rule requires every char in [A-Z0-9_]; a lowercase letter disqualifies.
    for (const w of ["Rgba16", "Proj", "gslower"]) {
      expect(classifyWord(w)).toBe("variableName");
    }
  });

  it("treats a lone uppercase letter as an atom (constant boundary)", () => {
    expect(classifyWord("G")).toBe("atom");
  });
});
