# Owned texture requests

T2 prepares immutable encoded sources while interpreting the display list. A
Material retains an `EncodedTextureRequest` for tex0, tex1, each independent LOD
level and detail. Level zero shares its request. `TextureBindingInput` pairs that
source with sampling state at the renderer/capture boundary. Existing synthetic
RGBA inputs retain their supplied bytes; framebuffer views retain their separate
ordered descriptor path.

The CPU bridge expands a request when the positional upload cache needs pixels.
Explicit pixel inspection and diagnostic traces can also decode a request. There
is no decoded CPU result cache, compute executor or content residency cache.
Material equality and capture admission compare encoded state. Positional upload
hits compare the exact witness even when two bindings refer to the same request.

Each interpreter TMEM version owns a lazy 4096-byte snapshot and an LRU recipe
memo. RDP clones share immutable request/snapshot allocations. A load starts a
new version even when it writes equal bytes; old scenes retain the prior version.
The memo survives frame boundaries when the bank survives. Reset/recreation starts
an empty memo. No generation, address of a Rust allocation, or historical load ID
is serialized into content identity.

The canonical recipe records representation, logical/output extents, format/size,
base/line in 64-bit words and palette/TLUT interpretation. The Fast3dV1 stream has
version 1 and explicit little-endian fields. Non-CI palette/TLUT and CI8's bank
selector are canonicalized away. The physical footprint builder selects encoded
bytes directly without running RGBA expansion. Linear compatibility reconstructs
its encoded stream and owns a shared palette bank. Its memo entry also checks the
provenance revision.

Format and poisoned-source checks run before memo access. Linear provenance and
availability are checked without allocating a reconstructed stream on a memo hit.
The material builder retains the original load diagnostic and PC. A valid retained
request remains executable after later mutation; a new rejected draw cannot reach
the memo or positional upload cache with that retained request.

The memo limits are 256 entries and 1 MiB of owned payload. Payload includes the
bank once, exact serialized witnesses and reconstructed linear streams. LRU eviction
drops memo references; retained scenes and upload bindings own their references
separately. An entry larger than the payload cap bypasses admission. Ordinary
supported recipes currently fit individually.

All operation counters use the `tmem.` prefix:

| Counter | Work counted |
| --- | --- |
| `snapshot_allocations`, `snapshot_bytes` | New request bank snapshots; sharing an allocation adds zero |
| `linear_reconstruction_bytes` | Encoded stream bytes constructed on misses |
| `hashes_computed`, `bytes_hashed` | Default-seed XXH3 calls and complete Fast3dV1 preimages passed to them |
| `memo_hits`, `memo_misses`, `memo_evictions`, `memo_bypasses` | Validated recipe memo operations |
| `witness_comparisons`, `bytes_compared` | Exact comparisons and full equal-length slices passed to equality, independent of SIMD/early exit |
| `cpu_decode_executions`, `cpu_decode_output_bytes` | Production CPU bridge expansion at upload; untimed inspection is separate |

`tmem.memo_entries` and `tmem.memo_payload_bytes` report current and `.peak`
values. `scene_owned_texture_bytes` and `cpu_upload_cache_bytes` count each shared
allocation once within their respective ownership scopes. These scopes overlap;
do not add them to estimate process memory. Their `allocation_overhead_bytes`
gauges report known struct, container-capacity and Arc control-block storage;
allocator rounding/bookkeeping is allocator-dependent and is not included.

`dispatch-expectations.json` freezes the exact T2 authored counts. Rust tests
consume those expectations and emit `T2_WORK` JSON records under `--nocapture`.
The unchanged 1×1 I8 recipe hashes 59 bytes once and snapshots 4096 bytes once;
subsequent frames add zero snapshot/hash work. A second 2×1 recipe hashes 62 bytes
and shares the bank. Equal-byte reload, sampling-only and rejected/provenance
controls have separate rows. T4's dispatch/residency requirements remain separate
from these CPU measurements. Timed native/Chrome workloads must retain all counters
and memory gauges, with separate instrumentation controls under the frozen policy.
