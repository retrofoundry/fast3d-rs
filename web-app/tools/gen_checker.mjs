// Procedural 32x32 RGBA gray-on-white checker texture generator for the segmented-sub-dl toy.
// Original pattern — NOT a copy of any Nintendo asset.
// Pattern: 4×4 pixel checker squares, alternating near-black (≈24,24,24) and white (255,255,255).
// The dark color is chosen to be RGBA16-representable: R=24≈(0x3<<3), G=24, B=24.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const W = 32, H = 32;

// CRC32 (PNG)
const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, "ascii");
  const body = Buffer.concat([t, data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}
function encodePNG(rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0; // 8-bit RGBA
  const raw = Buffer.alloc((W * 4 + 1) * H);
  for (let y = 0; y < H; y++) {
    raw[y * (W * 4 + 1)] = 0; // filter: none
    for (let x = 0; x < W; x++) {
      const o = y * (W * 4 + 1) + 1 + x * 4, i = (y * W + x) * 4;
      raw[o] = rgba[i]; raw[o + 1] = rgba[i + 1]; raw[o + 2] = rgba[i + 2]; raw[o + 3] = rgba[i + 3];
    }
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);
}

// Checker parameters
// 8 cells per axis × 4px = 32px. Alternating dark (≈24,24,24) / white (255,255,255).
const CELL = 4; // pixels per checker square

// Colors: gray-on-white SDK spirit (not blue/white)
const DARK  = [24, 24, 24, 255];   // near-black, RGBA16-clean
const WHITE = [255, 255, 255, 255];

const rgba = new Uint8Array(W * H * 4);
for (let py = 0; py < H; py++) {
  const row = Math.floor(py / CELL);
  for (let px = 0; px < W; px++) {
    const col = Math.floor(px / CELL);
    // Checker parity: dark when (row+col) is even, white when odd
    const dark = ((row + col) & 1) === 0;
    const c = dark ? DARK : WHITE;
    const i = (py * W + px) * 4;
    rgba[i]     = c[0];
    rgba[i + 1] = c[1];
    rgba[i + 2] = c[2];
    rgba[i + 3] = c[3];
  }
}

writeFileSync(new URL("../src/checker_tex.png", import.meta.url), encodePNG(rgba));
console.log("wrote web-app/src/checker_tex.png");
