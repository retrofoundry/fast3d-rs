import { describe, it, expect } from "vitest";
import { parseBin } from "./texture-bin";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Build a headered .bin buffer that mirrors CI8_TEX / CI4_TEX:
 * 32×32 RGBA8, even rows alpha=255, odd rows alpha=0.
 * This is the alternating-alpha pattern that the CI canary toys depend on.
 */
function makeCI8Like(): Uint8Array {
  const W = 32, H = 32;
  const buf = new Uint8Array(8 + W * H * 4);
  const dv = new DataView(buf.buffer);
  dv.setUint32(0, W, true); // w LE u32
  dv.setUint32(4, H, true); // h LE u32
  for (let row = 0; row < H; row++) {
    const alpha = row % 2 === 0 ? 255 : 0;
    const lum = (row * 8) & 0xff;
    for (let col = 0; col < W; col++) {
      const base = 8 + (row * W + col) * 4;
      buf[base] = lum; buf[base + 1] = lum; buf[base + 2] = lum; buf[base + 3] = alpha;
    }
  }
  return buf;
}

// ---------------------------------------------------------------------------
// Core parseBin tests
// ---------------------------------------------------------------------------

// Mirrors rgba-2x1.bin (web-app/src/lib/rgba-2x1.bin):
//   Header : w=2 (LE u32), h=1 (LE u32) — 8 bytes
//   Pixel 0: R=0xff G=0x00 B=0x00 A=0x80  ← non-trivial alpha (canvas would corrupt this)
//   Pixel 1: R=0x00 G=0xff B=0x00 A=0xff
function makeTwoByOne(): Uint8Array {
  const buf = new Uint8Array(16);
  const dv = new DataView(buf.buffer);
  dv.setUint32(0, 2, true); // w = 2
  dv.setUint32(4, 1, true); // h = 1
  buf[8] = 0xff; buf[9] = 0x00; buf[10] = 0x00; buf[11] = 0x80; // pixel 0: red, α=0x80
  buf[12] = 0x00; buf[13] = 0xff; buf[14] = 0x00; buf[15] = 0xff; // pixel 1: green, α=0xff
  return buf;
}

describe("parseBin", () => {
  it("reads w and h from the LE header", () => {
    const { w, h } = parseBin(makeTwoByOne());
    expect(w).toBe(2);
    expect(h).toBe(1);
  });

  it("rgba slice is exactly w*h*4 bytes", () => {
    const { rgba, w, h } = parseBin(makeTwoByOne());
    expect(rgba.length).toBe(w * h * 4);
  });

  it("preserves non-trivial alpha (alpha-as-data survives — canvas would premultiply/clamp)", () => {
    const { rgba } = parseBin(makeTwoByOne());
    // Pixel 0 alpha byte is at index 3; would be 0xff after canvas getImageData round-trip
    expect(rgba[3]).toBe(0x80);
  });

  it("1×1 single-pixel parse", () => {
    const buf = new Uint8Array(12);
    const dv = new DataView(buf.buffer);
    dv.setUint32(0, 1, true); // w
    dv.setUint32(4, 1, true); // h
    buf[8] = 0xab; buf[9] = 0xcd; buf[10] = 0xef; buf[11] = 0x7f;
    const { rgba, w, h } = parseBin(buf);
    expect(w).toBe(1);
    expect(h).toBe(1);
    expect(rgba.length).toBe(4);
    expect(rgba[3]).toBe(0x7f);
  });
});

// ---------------------------------------------------------------------------
// Alpha-preservation test — proof the CI canary signal survives the .bin path
// ---------------------------------------------------------------------------

describe("parseBin — CI canary alpha-preservation (headered 32×32, alternating alpha)", () => {
  // The CI8_TEX / CI4_TEX const in goldens.rs uses the same pattern:
  //   even rows → alpha=255 (palette a1=1 → TEXEL0_ALPHA output = white)
  //   odd  rows → alpha=0   (palette a1=0 → TEXEL0_ALPHA output = black)
  // parseBin must return this intact; canvas getImageData would flatten all alpha to 255.

  it("header decodes to (32, 32)", () => {
    const { w, h } = parseBin(makeCI8Like());
    expect(w).toBe(32);
    expect(h).toBe(32);
  });

  it("rgba body is exactly 32×32×4 bytes", () => {
    const { rgba } = parseBin(makeCI8Like());
    expect(rgba.length).toBe(32 * 32 * 4);
  });

  it("even rows (row 0) have alpha=255", () => {
    const { rgba } = parseBin(makeCI8Like());
    // Row 0, pixel 0: alpha at index 3
    expect(rgba[3]).toBe(255);
  });

  it("odd rows (row 1) have alpha=0 — canvas path would return 255 here (premultiply clamp)", () => {
    const { rgba } = parseBin(makeCI8Like());
    // Row 1, pixel 0: alpha at index (32 * 4) + 3
    expect(rgba[32 * 4 + 3]).toBe(0);
  });

  it("alpha values are mixed — both 0 and 255 present, proving canary signal is NOT flattened", () => {
    const { rgba } = parseBin(makeCI8Like());
    const seen = new Set<number>();
    for (let i = 3; i < rgba.length; i += 4) seen.add(rgba[i]);
    expect(seen.has(0)).toBe(true);
    expect(seen.has(255)).toBe(true);
    expect(seen.size).toBeGreaterThan(1);
  });
});
