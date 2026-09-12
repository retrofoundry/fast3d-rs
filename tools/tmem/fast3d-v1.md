# Fast3dV1 preimage freeze

This test-only format freezes Fast3dV1 preimages and XXH3 vectors at fast3d
`bb0fea399793eb2aeb1ae93d5972763b9761da98`. The serializer lives in
`fast3d/tests/common/tmem_preimage.rs` and serializes explicitly supplied selected
bytes. It contains no footprint discovery, HLE validation, decoder or runtime
cache. Production rendering still uses the existing CPU decoder.

## Byte layout

Every integer is unsigned and little-endian. Byte strings are copied verbatim.
There is no alignment padding, terminator beyond the domain's explicit NUL,
native struct encoding, or `usize` field. The entire stream is hashed once with
default-seed/default-secret XXH3-64. The digest is a numeric `u64`; the fixture
JSON writes it as exactly 16 hexadecimal digits, most significant digit first.

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 16 bytes | ASCII `fast3d-tmem-key` followed by `00` |
| 16 | u16 | Version: `1` |
| 18 | u8 | Representation: Tile `0`, Lookup4096x4 `1`, LinearCompat `2` |
| 19 | u8 | Output encoding: RGBA8 `0` |
| 20 | u32 | Output width |
| 24 | u32 | Output height |
| 28 | u32 | Logical width |
| 32 | u32 | Logical height |
| 36 | u8 | Effective format: RGBA `0`, CI `2`, IA `3`, I `4` |
| 37 | u8 | Effective texel size: 4-bit `0`, 8-bit `1`, 16-bit `2`, 32-bit `3` |
| 38 | u16 | TMEM base in eight-byte words, before format-specific bank masking |
| 40 | u16 | TMEM line in eight-byte words |
| 42 | u8 | Canonical palette selector |
| 43 | u8 | Canonical extracted TLUT mode, `0`, `1`, `2`, or `3` |
| 44 | u32 | Number P of selected physical address/byte pairs |
| 48 | 3 × P bytes | P pairs, each a u16 byte address followed by one byte |
| 48 + 3P | u32 | Number L of reconstructed linear bytes |
| 52 + 3P | L bytes | Reconstructed linear stream |
| 52 + 3P + L | u32 | Number Q of selected linear palette address/byte pairs |
| 56 + 3P + L | 3 × Q bytes | Q pairs, each a u16 byte address followed by one byte |

The preimage length is `56 + 3P + L + 3Q`. Addresses are physical TMEM byte
offsets in `0..4096`, after wrap and odd-row addressing. Pair counts count
bytes, including both bytes of each used palette halfword. Each address occurs
once, in ascending order. Repeated visits to the same address do not add bytes;
contradictory values for one address are invalid fixture input.

For Tile and Lookup4096x4, L and Q are zero. P is the union of the encoded texel
bytes and, for CI, the selected palette halfwords. Palette entries retain their
addresses even when two entries have the same colour. For LinearCompat, P is
zero; L bytes preserve the reconstructed stream's order and flat nibble
convention, followed by the selected palette pairs in Q. A non-CI request has
no separate palette pairs. Physical and linear representations have disjoint
payload shapes even if their resulting pixels agree.

The helper accepts already resolved and validated fields. Validation and byte
selection are outside this test serializer. Physical Tile selection must cover every byte
its output decode can read, including wrap, odd-row swaps, final odd nibbles,
and both RGBA32 banks. Lookup selection covers the generated 4096 × 4 output,
regardless of the nominal logical extent. The 32-bit size tag remains `3` for
RGBA32 even though each bank supplies two bytes per texel. TLUT bytes retain
their encoded byte order; the serializer never decodes big-endian halfwords.

## Canonical fields and equality

For every non-CI format, palette and TLUT are zero. For CI8, palette is zero.
CI4 retains its selected bank. CI TLUT modes 0 and 1 remain distinct consulted
interpretations, even though both currently decode to transparent black.
Modes 2 and 3 are the extracted values; they are not rt64's shifted othermode
values `0x8000` and `0xc000`.

Base, line, representation, output extent, and logical extent remain in the
identity for all three representations, as required by the accepted design.
This includes base/line on LinearCompat and line on Lookup4096x4. They are
deliberate recipe distinctions, even where a different recipe yields equal
pixels. Source descriptors must supply effective format/size before this
boundary; historical load format/size are not extra fields.

Sampling bounds, clamp/mirror, shifts, masks, filter, primitive/environment
colours, role, material position, guest pointers, bank generations, provenance
revisions, historical load IDs, and device/provider revisions are absent.
A sampling change can alter the chosen representation or required extent;
that resolved change is represented by those fields. Otherwise it reuses the
same decode identity. Linear content is the resolved encoded stream, not the
identity of the load that supplied it. The fixed field inventory and preimages freeze this omission contract.

A matching digest selects candidates. Reuse requires exact equality of the
canonical metadata and selected encoded bytes. Equality of the complete
canonical preimage is an equivalent test witness. The forced-bucket test
injects a constant hash into a test-only key constructor and checks this exact
equality contract for changed content, recipes, and CI interpretation. It
makes no runtime cache, eviction, residency, or provider claim. Execution counts are not measured by this serializer.

## Fixture derivation

`vectors.json` is the shared native/wasm input. Each recipe row records the
explicit input pairs, linear bytes, complete expected preimage, numeric hash,
and the bad implementation it distinguishes. Input pair lists may be unordered
and contain identical repeated visits; the expected preimage is canonical.
The `probes` rows mutate one stated byte field in a literal preimage. Some raw
probes are intentionally not valid decode requests, including unknown versions
and inconsistent lengths. They freeze hash sensitivity and field framing, not
admission behavior.

The selected addresses were derived directly:

- `tile-i8` reads byte `3 × 8 = 24`. Its unused-palette/TLUT variant is identical.
- `tile-rgba16-base511` reads bytes 4088 and 4089, preserving `12 35` order.
- `tile-i4-odd-wrap` is 5 × 3 with base 4088 and line 16 bytes. The three
  row-byte lists are `[4088,4089,4090]`, `[12,13,14]`, and `[24,25,26]`:
  the middle row wraps and applies XOR 4. The fifth texel needs the third
  byte of every row. Byte 12 is deliberately visited twice in the input.
- `tile-ci4-bank15` encodes indices 0, 15, and 7 in `0f 70`. Palette address
  `2048 + 15 × 128 + index × 8` gives halfwords at 3968, 4088, and 4024.
  Entries 0 and 7 have equal bytes but remain distinct addresses. The unused
  low nibble of the final byte adds no palette entry.
- `tile-ci8-tlut*` reads index 255 at byte 16 and palette bytes 4088/4089.
  The bank-15 variant of mode 2 canonicalizes to the same preimage.
- `tile-rgba32-two-banks` reads RG at 2040/2041 and BA at 4088/4089.
- `linear-i4-flat-odd` is the flat stream `12 34 56 78 9a bc de f0` for
  fifteen texels. `linear-ci4-bank15` uses `0f 70` followed by the same selected
  bank-15 halfwords as the physical CI4 row, with IA16 interpretation.
- `lookup-i8-full-bank` selects all 4096 physical addresses, each with byte
  `(37 × address + 11) mod 256`. The 4096 × 4 output has nominal logical
  extent 5 × 3. Repeated reads across output planes contribute each address
  only once; the 12,344-byte preimage also exercises long XXH3 input.

No selected-address expectation calls a production footprint builder, TMEM
decoder, or translated compute shader. These examples are serialization
fixtures; independent decode/address tests own production footprint coverage.

## Hash provenance and checks

All 14 recipe vectors and 17 raw probes were hashed independently with upstream
C xxHash 0.8.2, using both `XXH3_64bits` and
`XXH3_64bits_reset/update/digest` in seven-byte chunks. Both results agreed.
The header came from the pinned reference checkout
`43373749dac9bbc1b653e6a02aed40a9e1783bed`, path
`src/contrib/xxHash/xxhash.h`, SHA-256
`2b722f6ba682cbbd46ef8839633b74ca2a35327308dda2dedb8f0f56f750ae2c`.
Only upstream xxHash was used; no rt64 TMEM hasher or Rt64V5 stream was invoked.
The Rust test uses dev-only `twox-hash = 2.1.2` and checks both one-shot and
seven-byte streaming hashing against the literal C results.

For example, the 59-byte `tile-i8` preimage is:

```text
6661737433642d746d656d2d6b657900
0100000001000000010000000100000001000000
0401030001000000
010000001800a50000000000000000
```

Its digest is `c6ed8a9c36cf2022`. The CI8 mode-2 digest is
`b5a09b9c66baf02c`; the full lookup digest is `0e868c9223421692`.
The JSON stores complete streams and their SHA-256 values for independent
rechecking; the Rust serializer is never used to regenerate expected hashes.

Run from the repository's devenv shell:

```sh
cargo test --offline -p fast3d --test tmem_preimage
cargo test --offline -p fast3d --target wasm32-unknown-unknown --test tmem_preimage --no-run
```

The wasm target uses `wasm_bindgen_test` and requires no adapter or GPU.
Compilation is only a build check; the registered browser test execution
provides native/wasm numeric agreement evidence.
