import { describe, it, expect } from "vitest";
import { TOYS } from "./index";

const CATEGORIES = new Set(["Basics", "Geometry", "3D", "Texture Formats", "2D"]);

describe("toy registry", () => {
  it("has the 32 curated toys with unique slugs/ids", () => {
    expect(TOYS).toHaveLength(32);
    const slugs = TOYS.map((t) => t.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
    expect(slugs).toEqual(
      expect.arrayContaining([
        "onetri", "textured-quad", "flat-color",
        "backface-culling", "matrix-stack", "segmented-sub-dl",
        "perspective-cube", "lights", "chrome-icosphere", "morphcube",
        "two-cycle-combiner", "i8-ramp", "i4-ramp",
        "ia16-ramp", "ia8-ramp", "ia4-ramp",
        "wrap-repeat", "mirror-repeat",
        "ci8-ramp", "ci8-canary",
        "ci4-grid", "ci4-canary",
        "multi-material",
        "tron",
        "fogworld",
        "alpha-threshold",
        "decal",
        "high-poly",
      ]),
    );
  });

  it("each toy has the required fields and a valid category", () => {
    for (const t of TOYS) {
      expect(t.slug).toBeTruthy();
      expect(t.id).toBeTruthy();
      expect(t.title).toBeTruthy();
      expect(t.description).toBeTruthy();
      expect(t.owner.id).toBeTruthy();
      expect(t.schemaVersion).toBe(1);
      expect(CATEGORIES.has(t.category)).toBe(true);
      expect(t.source.length).toBeGreaterThan(0);
    }
  });

  it("each source contains the commands required to render (TMEM gate + geometry + end)", () => {
    for (const t of TOYS) {
      expect(t.source).toContain("gsSPEndDisplayList");
      // 2D toys (rect-draw or fill-rect) skip the triangle and 3-class checks.
      const is2D = /gsSPTextureRectangle|gsDPFillRectangle/.test(t.source);
      if (!is2D) {
        expect(t.source).toMatch(/gsSP1Triangle|gsSP2Triangles/);
        // A toy is render-valid if it falls into exactly one of three mutually exclusive classes:
        //   1. Textured: has gsDPLoadTextureBlock AND gsSPTexture(...G_ON)
        //   2. Lit: uses G_LIGHTING (hardware lighting provides the color) — no texture
        //   3. Shade-only: uses G_SHADE with no texture and no G_LIGHTING (Gouraud vertex colors)
        const isTextured =
          t.source.includes("gsDPLoadTextureBlock") &&
          /gsSPTexture\([^)]*G_ON/.test(t.source);
        const isLit = !isTextured && t.source.includes("G_LIGHTING");
        const isShadeOnly =
          !isTextured && !isLit &&
          t.source.includes("G_SHADE") &&
          !t.source.includes("gsDPLoadTextureBlock") &&
          !t.source.includes("G_LIGHTING");
        expect([isTextured, isLit, isShadeOnly].filter(Boolean)).toHaveLength(1);
      }
    }
  });
});
