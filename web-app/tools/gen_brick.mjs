// Procedural 32x32 RGBA brick texture generator for the textured-quad toy.
// Original pattern — NOT a copy of any Nintendo asset.
// Pattern: warm terracotta bricks (≈128,70,45) separated by white mortar (≈230,220,210) lines,
// with alternating half-brick offsets on odd rows. Brick dimensions: 14px wide × 7px tall,
// mortar: 2px wide × 2px tall, gives a classic running-bond layout within 32×32.
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

// Brick layout parameters
// Each course: 7px brick body + 2px mortar = 9px row height (3.5 courses ≈ 32px, tile repeats)
// Each brick: 14px body + 2px mortar = 16px column width (2 bricks per row = 32px)
const BRICK_W = 14; // brick body width
const MORTAR_W = 2; // mortar/joint width
const BRICK_H = 7;  // brick body height
const MORTAR_H = 2; // mortar/joint height
const CELL_W = BRICK_W + MORTAR_W; // 16
const CELL_H = BRICK_H + MORTAR_H; // 9

// Colors — warm terracotta palette, RGBA16-representable (quantized to 8 levels per channel roughly)
const BRICK_BASE  = [128, 72, 40, 255];   // mid terracotta
const BRICK_HI    = [152, 88, 48, 255];   // lighter face
const BRICK_DARK  = [104, 56, 32, 255];   // shadow edge
const MORTAR_COL  = [224, 216, 200, 255]; // warm off-white mortar

function brickColor(bx, by) {
  // Slight variation by brick position for visual interest
  const v = ((bx * 7 + by * 13) & 0xf);
  if (v < 4)  return BRICK_DARK;
  if (v < 10) return BRICK_BASE;
  return BRICK_HI;
}

const rgba = new Uint8Array(W * H * 4);
for (let py = 0; py < H; py++) {
  // Which course (row of bricks) are we in?
  const courseIndex = Math.floor(py / CELL_H);
  const rowInCourse = py % CELL_H;

  // Horizontal offset: odd courses shift by half a cell (8px) for running bond
  const xOffset = (courseIndex & 1) ? (CELL_W >> 1) : 0;

  for (let px = 0; px < W; px++) {
    const shiftedX = (px + W - xOffset) % W; // wrap offset
    const colInCell = shiftedX % CELL_W;
    const brickCol  = Math.floor(shiftedX / CELL_W);

    let col;
    // Mortar rows and columns
    const inMortarH = rowInCourse >= BRICK_H;
    const inMortarV = colInCell >= BRICK_W;

    if (inMortarH || inMortarV) {
      col = MORTAR_COL;
    } else {
      col = brickColor(brickCol, courseIndex);
    }

    const i = (py * W + px) * 4;
    rgba[i]     = col[0];
    rgba[i + 1] = col[1];
    rgba[i + 2] = col[2];
    rgba[i + 3] = col[3];
  }
}

writeFileSync(new URL("../src/brick_tex.png", import.meta.url), encodePNG(rgba));
console.log("wrote web-app/src/brick_tex.png");
