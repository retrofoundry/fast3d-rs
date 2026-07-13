/**
 * Parse a raw RGBA `.bin` texture file produced by the n64-toys pipeline.
 *
 * Format: 4-byte LE uint32 width, 4-byte LE uint32 height, then w*h*4 raw RGBA bytes.
 *
 * This bypasses the browser's canvas/getImageData path (which premultiplies and
 * clamps alpha), preserving alpha-as-data for IA textures.
 */
export function parseBin(buf: Uint8Array): { rgba: Uint8Array; w: number; h: number } {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  return { rgba: buf.subarray(8), w: dv.getUint32(0, true), h: dv.getUint32(4, true) };
}
