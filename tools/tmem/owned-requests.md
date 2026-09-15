# Owned texture requests

The interpreter prepares immutable encoded sources while interpreting the display list. A
Material retains an `EncodedTextureRequest` for tex0, tex1, each independent LOD
level and detail. Level zero shares its request. `TextureBindingInput` pairs that
source with sampling state at the renderer/capture boundary. Synthetic RGBA inputs
retain their supplied bytes; framebuffer views retain their ordered descriptors.

Renderer preparation sends supported TMEM requests to compute and reuses resident
images by Fast3dV1 identity with exact witness equality. The request CPU decoder is
compiled only for tests. Synthetic RGBA sources upload their supplied pixels.
There is no runtime executor selector or CPU fallback for a TMEM request.

TMEM owns its bytes in an Arc. Request construction shares that storage without
allocating or copying a bank snapshot. Each load obtains exclusive storage once,
before writing any bytes. If a request or cloned RDP still owns the old bank, the
load copies it; otherwise it writes in place. These copies preserve the draw's
encoded input for later GPU preparation and diagnostics. They are counted as bank
copy-on-write work, not hidden as zero snapshot cost. Source provenance and rejected
load diagnostics retain their existing independent ownership.

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

Material and capture-admission comparisons compare the canonical recipe and
complete owned bank bytes directly. Linear requests compare their resolved
stream and, for CI, the palette region. They compare bytes even for the same
request allocation and never rely on a digest alone. This is conservative: changing
an unused bank or palette byte can prevent reuse even when the Fast3dV1 footprint
is unchanged. Renderer residency uses the narrower Fast3dV1 footprint and compares
its exact witness before sharing an image.

The canonical recipe records representation, logical/output extents, format/size,
base/line in 64-bit words and palette/TLUT interpretation. Fast3dV1 has version 1
and explicit little-endian fields. Non-CI palette/TLUT and CI8's bank selector are
canonicalized away. The physical footprint builder selects encoded bytes without
RGBA expansion. Linear compatibility resolves and owns its encoded stream at
construction, preserving provenance differences even between equal physical banks.

Format and poisoned-source checks run before request construction. Linear
reconstruction validates provenance and availability before allocating its stream.
The material builder retains the original load diagnostic and PC. A retained
request remains executable after mutation; a new rejected draw cannot use that
request to bypass validation or reach residency.

Separate requests have separate lazy identity cells, including repeated recipes
on an unchanged bank. There is no interpreter recipe memo. Linear streams are
reconstructed on each request. Only retained scenes, bindings and interpreter
state keep bank allocations alive.

All operation counters use the `tmem.` prefix:

| Counter | Work counted |
| --- | --- |
| `requests`, `rejected_requests` | Interpreter texture-role requests and rejected requests |
| `bank_cow_allocations`, `bank_cow_bytes` | Bank copies made by loads while prior storage is still owned |
| `linear_reconstruction_bytes` | Encoded linear stream bytes constructed for validated requests |
| `hashes_computed`, `bytes_hashed` | Hash executions and complete Fast3dV1 preimages fed to XXH3 |
| `preimage_bytes_constructed` | Canonical bytes written on identity initialization |
| `identity_memo_hits` | Accesses that reuse the request-local identity; includes binding checks and combined key/witness access for residency |
| `footprint_span_bytes`, `palette_index_reads` | Physical range lengths before deduplication and encoded byte reads for CI palette selection |
| `encoded_comparisons`, `bytes_compared` | Direct owned-input comparisons and full equal-length slices passed to equality, independent of early exit |
| `cpu_decode_executions`, `cpu_decode_output_bytes` | Zero for supported TMEM rendering, including live tracing |

The removed `memo_*`, `snapshot_allocations` and `snapshot_bytes` operations emit
no counters or gauges; absence means zero. `scene_owned_texture_bytes` counts
shared allocations once within the scene, including witnesses only after
initialization. Its `allocation_overhead_bytes` gauge counts structs, container
capacity and Arc control blocks, excluding allocator rounding. Renderer residency
accounts for its own witness ownership separately; overlapping scopes must not be added.

Live trace capture copies the request's encoded bank and resolved compatibility
stream with its tile/role/extent metadata and any rejection. It performs no RGBA
expansion or decoder read-visitation. `Request::output`, `reachable_bytes`,
`read_operations` and `palette_bytes` stay empty or zero until the explicit offline
`Request::analyze` developer helper runs. The performance tool's trace collector
invokes that helper outside the interpreter and rendering path. The older decoder
preflight helpers remain diagnostic APIs over saved bank/provenance data; they do
not select the renderer's executor. Inspection and capture admission compare owned
encoded state. Pixel diagnostics use explicit GPU readback or these offline tools.

`dispatch-expectations.json` freezes the exact T2 counts. Rust tests consume them
and emit `T2_WORK` JSON under `--nocapture`. Unused identity costs zero snapshots,
hashes or witnesses, including unchanged frames, new recipes, equal-byte reloads,
role aliases and three retained frames of the `tmem-layouts` display list. That
last fixture issues 54 requests with 26 required bank copies and 27 direct bank
comparisons; all identity and memo counts remain zero. Explicit 1×1 I8 identity
access hashes 59 bytes once, including when requested after the bank is gone.
T4's dispatch/residency expectations are the separate `t4` section and
[renderer contract](../../docs/texture-residency.md). The public facade retains
a cloned prior RDP, so its three-frame reload gate counts 27 bank copies rather
than the moved-RDP fixture's 26. Timed native/Chrome rows
must retain the operation and memory counters with separate instrumentation
controls under the frozen policy.
