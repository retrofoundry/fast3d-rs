# Hardware fog mirrors

Run both presentation paths with:

```sh
cargo test -p fast3d --lib tests::fog_mirror::
```

The 93 cases cover 40 blender inputs, 25 raw game modes, 24 constant-W
interpolation planes and four neighbour-timing gradients. Expected hardware words
come from uncached N64 RDRAM readbacks. `provenance.json` records the hardware run
IDs, reference revisions, original input/readback hashes and compact fixture hashes.
Hardware supplies the RGB expectations; references do not replace it.

`cases.json` contains raw state commands, byte vertices, selected pixel spans and
uniform expected words. Game modes share setup commands and identical sample data.
The `data` field names each nonuniform case’s `.bin` file. These files
start with a little-endian u16 palette count, followed by palette records and one
little-endian u16 palette index per selected pixel. A record contains a big-endian
hardware word (RGBA16 or RGBA32), followed by three modern RGB bytes when `modern`
is true. Only asserted pixels are stored. Alpha/coverage bits in hardware words
are provenance, not GPU-alpha assertions. RGBA16 comparisons reduce GPU RGB with
`channel >> 3`.

Precision cases assert saved, independently calculated modern RGB as well as the
hardware bound. The modern model uses float fog weights, no patterned RDP dither,
current-pixel timing and affine interpolation at integer XY. Game blending permits
one RGB8 rounding unit; thin triangles permit one unit for byte-vertex adaptation.
Thin cases check the selected edge samples, including covered pixels. The game
masks select full interiors, not partial-coverage edges. Varying W and raw nine-bit
shade wrap/clamp are outside this adapter: wrap/clamp planes are split into byte
vertices, with XY rounded to 1/128 pixel.

Ignored tests name the remaining semantic failures and compare hardware exactly.
Enable them explicitly to reproduce those failures. `FAST3D_FOG_OUTPUT` optionally
exports the exercised 320×240 RGBA8 images into `dual/` and `fallback/`.

To regenerate from an archived `rdp-oracle` case/run tree, without console access:

```sh
python3 tools/fog-mirror/generate.py ORACLE_ARCHIVE fast3d/tests/fog-mirror
```

The generator retains the original inputs and masks, computes modern predictions
in Python, and compacts them. No renderer readback supplies an expected value.
