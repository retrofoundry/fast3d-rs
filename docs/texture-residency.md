# Texture decode and residency

Supported HLE textures are decoded by compute during renderer preparation. The
interpreter still performs TMEM loads, validates formats and framebuffer-load poison, and owns the encoded bytes at each draw. A later load cannot change
an earlier request. There is no production CPU texture executor or executor
selector. CPU decoding remains available to tests and explicit offline profiling
tools; live trace collection records encoded state without expanding RGBA pixels.

Every ordinary request samples the TMEM contents at draw time: loaded bytes,
bytes retained from earlier loads, and initial zeros. Tile decodes the requested
extent; Lookup4096x4 covers the complete physical address/odd-row/nibble domain
when an unclamped mask period exceeds the logical extent, a tall full-bank tile
needs T wrapping, or YUV TLUT filtering can fetch outside the nominal tile. Load history does not
select a decoder or reject an ordinary request.

The bank starts at zero and both reset APIs clear it. `begin_frame` preserves it.
This is fast3d's reset policy, not a hardware power-on claim. `NoTextureLoaded`
still rejects ordinary textured draws before any texture load. A 4096-bit written
map distinguishes untouched bytes from loaded zeros for the load-free framebuffer
rectangle shortcut and its triangle rejection. Every load writer updates the map,
including generated S2DEX loads. Guest-read failures roll back the task;
unsupported framebuffer loads poison their destinations until overwritten.

Physical requests upload the owned 4096-byte bank unchanged. Four encoded bytes
occupy each storage-buffer word, with the first byte in bits 0–7. A separate
64-byte uniform uses explicit little-endian fields. Each 8×8 workgroup writes
one RGBA8 texel per invocation, guarded by output bounds. Integer channel
expansion precedes the UNORM conversion. CI without TLUT reads the full bank;
TLUT modes apply to every format, with YUV byte addressing. Ordinary fetches
mask the base T to eight bits before adding a filtered neighbour's delta.

Each bank caches which palette slots have unequal lanes. Loads invalidate that
mask. A bank whose slots are replicated requires no per-request index scan for
lane selection. Otherwise the request checks its used slots. Non-replicated
requests decode four stacked planes, selected by filter tap and triangle;
replicated slots retain the one-plane allocation and sampling path. Tagged
palette lanes are replicated only when their source tags agree. Unknown tagged
indices conservatively depend on the complete palette.

One renderer owns the image cache for its device. Fast3dV1 selects a bucket;
exact canonical-witness equality authorizes every hit, including a repeated
reference to the same request. The witness shares immutable storage with the
request. The cache does not retain the request's whole bank. Sampling state and
texture roles do not enter image identity. Tex0, tex1, detail and each independent
LOD image resolve separately; the level-zero alias adds no resolution or upload.
Sampling bindings are reused when their images and descriptors agree. Prim/env
changes affect draw constants.

The cache defaults to 4096 entries, 64 MiB of decoded GPU payload and 8 MiB of CPU
witness payload. LRU recency advances at queue submission boundaries and updates
only touched entries. Eviction walks the oldest eligible entries. An image larger
than a limit, or a miss that cannot fit because readers pin resident images,
uses a transient result. Repeated transient requests can share pending work
across submissions. After completion, that lookup expires; an independently
retained image remains alive but is not admitted retroactively.

Bindings, prepared work and in-flight submissions retain strong image references.
Changed inactive bindings are released before preparing their replacements;
pinned entries cannot be evicted. Eviction never explicitly destroys a texture.
Several misses in a task use distinct input/uniform buffers and one ordered
compute pass before rendering. Input buffers return to the free pool only after
the queue-completion callback for their submission. The free pool is limited to
4096 buffer pairs and 8 MiB including uniforms. Rendering adds no per-texture CPU
wait, readback or submission. Explicit diagnostic readbacks are untimed and await
completion on browsers as well as native devices.

`begin_frame` clears retained scene work while preserving eligible residency.
`reconfigure` rebuilds the internal renderer, cache, bindings and staging pool.
Constructing a renderer for a replacement device starts the same fresh ownership
epoch. Old completion callbacks only signal their old submission tokens; they
cannot insert resources into a new cache. On device loss, the consumer recreates
its device and renderer. No device handles transfer between epochs.

Synthetic or debug RGBA sources upload their supplied pixels. Framebuffer loads
retain immutable GPU snapshots and per-byte tags with source pointer offsets.
A request keeps the exact path when its recipe is RGBA16/IA16 with TLUT off or
CI8 with TLUT 2/3, every reachable tag belongs to a complete aligned halfword from
one copy, and no byte in the selected palette range is tagged. RGBA16 preserves RGBA8 precision;
IA16 preserves the R10G10B10A2-to-I16A16 reinterpretation. CI8 still reconstructs
indices byte by byte before the TLUT lookup.

All other framebuffer requests materialise every tagged byte as RGBA16, including
complete pairs in a request containing a split or mismatched pair. RGB uses its
upper five bits without dithering. Alpha is `(a >> 2) & 1`, the existing coverage
convention: the low three bits cross the threshold at 4. It is not an RGBA8 alpha
threshold of 128. The high byte precedes the low byte. `byte_at` in
`render/texture_decode.wgsl` applies this before the ordinary decoder, including
TLUT indices, all four palette lanes, both RGBA32 banks and T wrapping. Ordinary
writes replace only their destination tags; stale tags follow the current recipe.

Both paths bypass Fast3dV1 identity and ordinary residency. `Copies` caches decoded
views by the immutable request's `Arc` identity for one submission, clearing that
map at `begin`. Copy buffers use the copy's `Arc` identity and remain alive while
TMEM or a retained request references the snapshot. Decode jobs wait for the last
referenced copy operation. Equality includes the recipe, mode, tags and immutable
copy descriptors; TLUT requests conservatively compare the whole bank because
GPU-produced indices can select entries absent from CPU placeholders. No GPU bytes
are hashed or represented by placeholder bytes in a content key.
The load-free framebuffer-view path retains its ordered framebuffer descriptors.

Each request memoizes its identity on first use. Separately prepared requests
compute their own keys, including equal-byte reloads. Physical selection marks
row spans in a 4096-bit set, splits odd-row partial words and wrapped ranges,
and mirrors RGBA32's selected low-bank bytes into the high bank. Lookup selects
its complete physical bank directly. Every TLUT-enabled format scans encoded
indices for palette selection; packed 4-bit indices exclude an unused final low
nibble. The serializer emits sorted address/byte pairs in Fast3dV1 framing, with
semantic flags for corrected CI-off and tall-T decoding and four-plane output.
Unaffected streams remain byte-identical to the frozen vectors. Material and capture admission compare those same sampled
bytes directly without constructing identities. Unrelated inherited bytes do
not prevent capture admission. History-dependent output requires a reset-prefix
sequence; inspector walks and renderer prefixes start from default state.

Profiling reports the following work and memory scopes. Gauges also emit `.peak`
values within each recorder interval.

| Name under `tmem.` | Meaning |
| --- | --- |
| `hashes_computed`, `bytes_hashed` | Request-local hash executions and complete Fast3dV1 bytes fed to XXH3 |
| `preimage_bytes_constructed` | Canonical bytes written on identity initialization, counted separately from hashing |
| `identity_memo_hits` | Accesses reusing an initialized request identity, including binding checks; a residency resolution accesses key and witness together |
| `footprint_span_bytes` | Physical range lengths visited by identity or comparison before wrap/overlap deduplication, including both RGBA32 banks; excludes palette selection |
| `palette_index_reads` | Encoded byte reads for TLUT palette selection in identity or comparison; a packed 4-bit byte can supply two indices |
| `palette_lane_slot_checks` | Palette slots checked when a bank's replication mask is first needed after a write |
| `palette_lane_index_reads` | Index bytes examined for lane selection when the bank contains non-replicated slots |
| `bank_cow_allocations`, `bank_cow_bytes` | Load-time copies while old bank storage is owned |
| `encoded_comparisons`, `encoded_bytes_compared`, `witness_comparisons`, `bytes_compared` | Sampled-footprint and residency-witness equality work; selected bytes and equal-length witnesses count in full |
| `hits`, `pending_hits`, `misses`, `evictions`, `bypasses` | Image lookup and admission decisions |
| `decode_dispatches`, `decode_compute_passes` | Actual encoded compute work |
| `decode_input_upload_bytes`, `decode_input_upload_calls` | Raw encoded uploads |
| `decode_uniform_upload_bytes`, `decode_uniform_upload_calls` | Decode parameters, separately from raw input |
| `staging_reuses` | Completed staging reuse |
| `resident_*`, `pinned_*`, `in_flight_*`, `transient_*` | GPU/CPU payload and image counts, deduplicated within each ownership scope |
| `staging_pending_bytes`, `staging_free_bytes`, `staging_bytes` | Raw input plus uniform buffer capacities |
| `resident_allocation_overhead_bytes`, `submission_allocation_overhead_bytes` | Estimates of Rust metadata, container capacity and control blocks; excludes allocator rounding and driver allocations |

Resident, pinned and in-flight scopes overlap and must not be summed. Transient
images are not resident but can be in flight or retained. Cache limits do not cap
arbitrary retained scenes or concurrent submissions. Supported TMEM rendering,
including tracing, has zero `cpu_decode_executions` and
`cpu_decode_output_bytes`. Bank snapshot counters remain zero because requests
share the bank; load-time copy-on-write work is reported separately.

Golden baselines and prior readback artifacts remain frozen. The compute corpus has a separate required inventory and compares
raw GPU output to the CPU oracle plus independent literals. Native and Chrome
tests also exercise default renderer residency, independent roles, realistic
reloads, pressure, completion and device epochs. Hardware and timing acceptance
must be recorded for the candidate revision before merge.

Optional exact-witness replacement snapshots are described in
[texture-replacements.md](texture-replacements.md). Both framebuffer source modes
bypass replacement lookup and alias construction; native decoded residency keeps
its original Fast3dV1 keys.
