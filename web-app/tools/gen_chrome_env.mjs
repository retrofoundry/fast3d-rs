// Procedural 32x32 RGBA mirror-ball spheremap generator for the chrome-icosphere toy.
// Faithful to the SDK `Silver_Reflection` / Programming Manual §11.7.5 convention: a radial
// mirror-ball lightprobe (inscribed disk = the reflected surroundings), vertical sky->ground
// gradient, one bright sun highlight, high-contrast horizon. Low-frequency / high-contrast,
// since 32x32 / 5-bit channels have no room for fine detail.
//
// Orientation: our texgen samples t = (N.Up + 1)/2, and t=0 maps to texture row 0 (verified
// against the default-texture render: down-facing normals sampled the texture's top region).
// So texture ROW 0 (top) must be GROUND (down-facing) and the BOTTOM row must be SKY
// (up-facing). We therefore put b = N.Up = 2*(py+0.5)/H - 1 ... no: row 0 -> down-facing ->
// b must be -1. So b = -(2*(py+0.5)/H - 1) = 1 - 2*(py+0.5)/H  gives row0->b=+1 (sky) which is
// WRONG; we want row0->ground(b=-1). Hence: b = 2*(py+0.5)/H - 1  (row0 -> b=-1 -> ground). Good.
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";

const W = 32, H = 32;
const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
const lerp = (a, b, t) => a + (b - a) * t;
const mix = (c1, c2, t) => [lerp(c1[0], c2[0], t), lerp(c1[1], c2[1], t), lerp(c1[2], c2[2], t)];

// ---- CRC32 (PNG) ----
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

// ---- spheremap variants ----
function render(p) {
  const rgba = new Uint8Array(W * H * 4);
  for (let py = 0; py < H; py++) {
    for (let px = 0; px < W; px++) {
      const a = 2 * (px + 0.5) / W - 1;   // N.Right
      const b = 2 * (py + 0.5) / H - 1;   // N.Up  (row0 -> b=-1 -> ground; bottom -> sky)
      const r2 = a * a + b * b;
      let col;
      if (r2 > 1.0) {
        col = p.corner;                   // outside the sampled disk
      } else {
        const band = p.band;
        if (b > band) col = mix(p.sky, p.skyHi, clamp((b - band) / (1 - band), 0, 1));
        else if (b < -band) col = mix(p.ground, p.groundLo, clamp((-b - band) / (1 - band), 0, 1));
        else { // horizon band
          col = mix(p.ground, p.sky, (b + band) / (2 * band));
          col = mix(col, p.horizon, 0.45);
        }
        // sun highlight (in the sky hemisphere)
        const dx = a - p.sunX, dy = b - p.sunY, d2 = dx * dx + dy * dy;
        col = mix(col, [255, 255, 255], clamp(Math.exp(-d2 / (2 * p.sunS * p.sunS)), 0, 1));
        // rim darkening near the disk edge -> reads as a ball
        const rim = clamp((r2 - 0.72) / 0.28, 0, 1);
        col = mix(col, [12, 14, 20], rim * 0.55);
      }
      const i = (py * W + px) * 4;
      rgba[i] = Math.round(clamp(col[0], 0, 255));
      rgba[i + 1] = Math.round(clamp(col[1], 0, 255));
      rgba[i + 2] = Math.round(clamp(col[2], 0, 255));
      rgba[i + 3] = 255;
    }
  }
  return rgba;
}

const variants = {
  // A: cool studio chrome — pale blue sky, dark cool ground, crisp horizon
  A: { sky: [205, 226, 252], skyHi: [248, 252, 255], ground: [40, 48, 64], groundLo: [16, 19, 28],
       horizon: [150, 170, 200], corner: [10, 12, 16], band: 0.10, sunX: -0.38, sunY: 0.5, sunS: 0.11 },
  // B: high-contrast silver — near-white sky, near-black ground (most Silver_Reflection-like)
  B: { sky: [225, 232, 240], skyHi: [255, 255, 255], ground: [26, 28, 34], groundLo: [6, 7, 10],
       horizon: [170, 178, 190], corner: [6, 7, 10], band: 0.07, sunX: -0.42, sunY: 0.55, sunS: 0.10 },
  // C: warm-sky chrome — golden sky, cool dark ground (sunset studio)
  C: { sky: [250, 226, 180], skyHi: [255, 248, 230], ground: [34, 40, 58], groundLo: [14, 16, 26],
       horizon: [210, 180, 150], corner: [10, 11, 16], band: 0.10, sunX: 0.36, sunY: 0.52, sunS: 0.12 },
  // D: blue-sky chrome — deep-blue zenith -> light-blue horizon, bright horizon line, gray ground,
  //    one white sun. Saturated (NOT white) sky so structure + the sun highlight read clearly on
  //    a spinning ball. This is the wired default.
  D: { sky: [150, 195, 240], skyHi: [40, 90, 165], ground: [88, 92, 104], groundLo: [28, 30, 40],
       horizon: [228, 236, 250], corner: [10, 12, 18], band: 0.06, sunX: 0.32, sunY: 0.5, sunS: 0.105 },
};

// Variant D (blue-sky chrome) is the shipped env; write it to its real path. A/B/C go to /tmp
// for comparison only.
mkdirSync("/tmp/chrome_env", { recursive: true });
for (const [k, p] of Object.entries(variants)) {
  if (k !== "D") writeFileSync(`/tmp/chrome_env/chrome_env_${k}.png`, encodePNG(render(p)));
}
writeFileSync(new URL("../src/chrome_env.png", import.meta.url), encodePNG(render(variants.D)));
console.log("wrote web-app/src/chrome_env.png (variant D) + /tmp/chrome_env/chrome_env_{A,B,C}.png");
