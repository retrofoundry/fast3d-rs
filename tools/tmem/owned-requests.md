# Owned texture requests

The interpreter prepares immutable encoded sources while interpreting the display list. A
Material retains an `EncodedTextureRequest` for tex0, tex1, each independent LOD
level and detail. Level zero shares its request. `TextureBindingInput` pairs that
source with sampling state at the renderer/capture boundary. Synthetic RGBA inputs
retain their supplied bytes; framebuffer views retain their ordered descriptors.

Renderer preparation sends supported TMEM requests to compute and reuses resident
images by UltragfxV1 identity with exact witness equality. The request CPU decoder is
compiled for tests and offline profiling. Synthetic RGBA sources upload their supplied pixels.
There is no runtime executor selector or CPU fallback for a TMEM request.

TMEM owns its bytes in an Arc. Request construction shares that storage without
allocating or copying a bank snapshot. Each load obtains exclusive storage once,
before writing any bytes. If a request or cloned RDP still owns the old bank, the
load copies it; otherwise it writes in place. These copies preserve the draw's
encoded input for later GPU preparation and diagnostics. They are counted as bank
copy-on-write work, not hidden as zero snapshot cost. The written-byte map, framebuffer tags and rejected-load diagnostics retain
their independent ownership.

The renderer's profiled key and witness accessors initialize one request-local
identity on first access.
The canonical preimage and default-seed XXH3 key use the already-owned encoded
input, so a key requested after a later load, RDP clone mutation or RDP destruction
still describes the original draw. No extra bank snapshot is needed for identity.
Memory accounting reads only initialized witnesses and cannot force identity.
The profiled entry points count construction and hashing separately, and count
accesses that reuse an initialized identity. A resident image can share
the immutable witness allocation without retaining the whole encoded request.
The witness has its own Arc control block, counted as allocation overhead.

Material and capture admission compare the canonical recipe and sampled physical
footprint, including stale bytes, initial zeros and used CI palette entries.
Unrelated bank bytes and unused palette entries do not affect equality. This
comparison does not initialize an identity or rely on a digest. Renderer
residency compares the exact UltragfxV1 witness before sharing an image.

The canonical recipe records representation, logical/output extents, format/size,
base/line in 64-bit words and palette/TLUT interpretation. UltragfxV1 has version 1
and explicit little-endian fields. TLUT modes 2/3 keep the mode for every format and
the palette selector for every 4-bit size. With TLUT disabled only CI keeps the mode
and only CI4 the selector. CI8's bank selector is canonicalized away. The physical footprint builder selects encoded bytes without
RGBA expansion. Every ordinary request uses Tile or Lookup4096x4; equal bank
contents and recipes have equal identities regardless of load history.

Format and framebuffer-load poison checks run before request construction.
The material builder retains the original load diagnostic and PC. A retained
request remains executable after mutation; a new rejected draw cannot use that
request to bypass validation or reach residency.

Separate requests have separate lazy identity cells, including repeated recipes
on an unchanged bank. There is no interpreter recipe memo. Only retained scenes, bindings and interpreter
state keep bank allocations alive.

All operation counters use the `tmem.` prefix:

| Counter | Work counted |
| --- | --- |
| `requests`, `rejected_requests` | Interpreter texture-role requests and rejected requests |
| `bank_cow_allocations`, `bank_cow_bytes` | Bank copies made by loads while prior storage is still owned |
| `hashes_computed`, `bytes_hashed` | Hash executions and complete UltragfxV1 preimages fed to XXH3 |
| `preimage_bytes_constructed` | Canonical bytes written on identity initialization |
| `identity_memo_hits` | Accesses that reuse the request-local identity; includes binding checks and combined key/witness access for residency |
| `footprint_span_bytes`, `palette_index_reads` | Physical range lengths before deduplication and CI palette-index reads for identities and comparisons |
| `encoded_comparisons`, `encoded_bytes_compared`, `bytes_compared` | Sampled-footprint comparisons and selected byte counts, independent of early exit |
| `cpu_decode_executions`, `cpu_decode_output_bytes` | Zero for supported TMEM rendering, including live tracing |

The removed `memo_*`, `snapshot_allocations` and `snapshot_bytes` operations emit
no counters or gauges; absence means zero. `scene_owned_texture_bytes` counts
shared allocations once within the scene, including witnesses only after
initialization. Its `allocation_overhead_bytes` gauge counts structs, container
capacity and Arc control blocks, excluding allocator rounding. Renderer residency
accounts for its own witness ownership separately; overlapping scopes must not be added.

Live trace capture copies the request's encoded bank with tile/role/extent metadata
and any rejection. It performs no RGBA expansion or decoder read-visitation.
`Request::output`, `reachable_bytes`, `read_operations` and `palette_bytes` stay
empty or zero until the offline `Request::analyze` helper runs. The trace format
requires `Request::version == 3`, removes `linear` and bank `sources`/`blocks`, and
rejects older records. `Request::decode` replaces the reconstruction/executor APIs;
offline validation returns a string error, not a rendering diagnostic.
`Request::canonical()` uses the diagnostic `ultragfx-b1-bank-v3` stream. Production
Tile/Lookup UltragfxV1 keys retain their version and bytes. Retired linear vectors
remain historical serializer fixtures, not production recipes.

`dispatch-expectations.json` freezes the exact T2 counts. Rust tests consume them
and emit `T2_WORK` JSON under `--nocapture`. Unused identity costs zero snapshots,
hashes or witnesses, including unchanged frames, new recipes, equal-byte reloads,
role aliases and three retained frames of the `tmem-layouts` display list. That
last fixture issues 54 requests with 26 required bank copies and 27 sampled-footprint
comparisons; all identity and memo counts remain zero. Explicit 1×1 I8 identity
access hashes 61 bytes once, including when requested after the bank is gone.
T4's dispatch/residency expectations are the separate `t4` section and
[renderer contract](../../docs/texture-residency.md). The public facade retains
a cloned prior RDP, so its three-frame reload gate counts 27 bank copies rather
than the moved-RDP fixture's 26. Timed native/Chrome rows
must retain the operation and memory counters with separate instrumentation
controls under the frozen policy.

Diagnostic version 3 uses the corrected angrylion decode semantics. `extent` is
the extent of one plane; `decode()`/`analyze()` append four planes when used
palette slots have differing lanes. GPU allocation and census PNG extents
include every plane. Version 1/2 diagnostic requests are rejected. UltragfxV1
keys retain version 1 and distinguish only affected requests through recipe
fields and semantic flags.
