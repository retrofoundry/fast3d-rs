# Texture decode and residency

Supported HLE textures are decoded by compute during renderer preparation. The
interpreter still performs TMEM loads, validates formats and rejected-source
provenance, and owns the encoded bytes at each draw. A later load cannot change
an earlier request. There is no production CPU texture executor or executor
selector. CPU decoding remains available to tests and explicit offline profiling
tools; live trace collection records encoded state without expanding RGBA pixels.

The three recipes preserve the existing representations:

- Tile decodes an independent RGBA8 image with the requested output extent.
- Lookup decodes the complete 4096×4 physical address/odd-row/nibble lookup.
- Linear compatibility reconstructs the validated encoded stream on the CPU,
  then expands it on the GPU with the existing flat-stream nibble convention.

Physical requests upload the owned 4096-byte bank unchanged. Four encoded bytes
occupy each storage-buffer word, with the first byte in bits 0–7. Compatibility
streams are padded to a four-byte boundary and followed by the owned palette
bank; padding and both upload lengths are counted. A separate 64-byte uniform
uses explicit little-endian fields. Each 8×8 workgroup writes one RGBA8 texel per
invocation, guarded by output bounds. Integer channel expansion precedes the
UNORM conversion. Fragment addressing and filtering remain unchanged.

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

Synthetic or debug RGBA sources still upload their supplied pixels. The existing
bounded framebuffer-view path uses ordered framebuffer descriptors, not TMEM
identity. GPU-source texture loads remain rejected under the existing framebuffer
contract. This change adds no replacement provider.

There is no recipe memo. The three-frame per-draw reload census finds 27 reusable
bank/recipe tuples among 54 request allocations, but does not establish a net
timing benefit from retaining and looking them up. Repeated request objects reuse
their lazy identity; separately prepared requests still compute their own key.

Profiling reports the following work and memory scopes. Gauges also emit `.peak`
values within each recorder interval.

| Name under `tmem.` | Meaning |
| --- | --- |
| `hashes_computed`, `bytes_hashed` | Request-local identity initialization and preimage bytes |
| `bank_cow_allocations`, `bank_cow_bytes` | Load-time copies while old bank storage is owned |
| `linear_reconstruction_bytes` | Owned compatibility stream construction |
| `encoded_comparisons`, `witness_comparisons`, `bytes_compared` | Whole-input and residency-witness equality work; equal-length slices count in full |
| `hits`, `pending_hits`, `misses`, `evictions`, `bypasses` | Image lookup and admission decisions |
| `decode_dispatches`, `decode_compute_passes` | Actual encoded compute work |
| `decode_input_upload_bytes`, `decode_input_upload_calls` | Raw encoded uploads, including alignment padding |
| `decode_uniform_upload_bytes`, `decode_uniform_upload_calls` | Decode parameters, separately from raw input |
| `decode_upload_padding_bytes`, `staging_reuses` | Compatibility padding and completed staging reuse |
| `resident_*`, `pinned_*`, `in_flight_*`, `transient_*` | GPU/CPU payload and image counts, deduplicated within each ownership scope |
| `staging_pending_bytes`, `staging_free_bytes`, `staging_bytes` | Raw input plus uniform buffer capacities |
| `resident_allocation_overhead_bytes`, `submission_allocation_overhead_bytes` | Estimates of Rust metadata, container capacity and control blocks; excludes allocator rounding and driver allocations |

Resident, pinned and in-flight scopes overlap and must not be summed. Transient
images are not resident but can be in flight or retained. Cache limits do not cap
arbitrary retained scenes or concurrent submissions. Supported TMEM rendering,
including tracing, has zero `cpu_decode_executions` and
`cpu_decode_output_bytes`. Bank snapshot counters remain zero because requests
share the bank; load-time copy-on-write work is reported separately.

The existing 27 goldens, 60-row oracle ledger and 171 configuration readback rows
remain frozen. The compute corpus has a separate required inventory and compares
raw GPU output to the CPU oracle plus independent literals. Native and Chrome
tests also exercise default renderer residency, independent roles, realistic
reloads, pressure, completion and device epochs. Hardware and timing acceptance
must be recorded for the candidate revision before merge.
