# Owned texture requests

T2 prepares immutable encoded sources while interpreting the display list. A
Material retains an `EncodedTextureRequest` for tex0, tex1, each independent LOD
level and detail. Level zero shares its request. `TextureBindingInput` pairs that
source with sampling state at the renderer/capture boundary. Synthetic RGBA inputs
retain their supplied bytes; framebuffer views retain their ordered descriptors.

The CPU bridge expands a request when the positional upload cache needs pixels.
Explicit pixel inspection and diagnostic traces can also decode a request. There
is no decoded CPU result cache, compute executor or content residency cache.

TMEM owns its bytes in an Arc. Request construction shares that storage without
allocating or copying a bank snapshot. Each load obtains exclusive storage once,
before writing any bytes. If a request or cloned RDP still owns the old bank, the
load copies it; otherwise it writes in place. These copies preserve deferred CPU
decoding even when identity is never requested. They are counted as bank
copy-on-write work, not hidden as zero snapshot cost. Source provenance and rejected
load diagnostics retain their existing independent ownership.

`key()` and `witness()` initialize one request-local identity on first access.
The canonical preimage and default-seed XXH3 key use the already-owned encoded
input, so a key requested after a later load, RDP clone mutation or RDP destruction
still describes the original draw. No extra bank snapshot is needed for identity.
Memory accounting reads only initialized witnesses and cannot force identity.
The profiled key entry point counts first initialization; convenience inspection
accessors are unprofiled, like the explicit pixel decoder.

Material comparisons and positional upload checks compare the canonical recipe
and complete owned bank bytes directly. Linear requests compare their resolved
stream and, for CI, the palette region. They compare bytes even for the same
request allocation and never rely on a digest alone. This is conservative: changing
an unused bank or palette byte can prevent reuse even when the Fast3dV1 footprint
is unchanged. The canonical key/witness contract itself is unchanged. Residency
can use that narrower identity when it has a consumer in T4.

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
request to reach the positional upload cache.

The recipe memo is deferred to T4. A bank reload invalidated every entry in T2,
so demo1-dense's reload-per-draw workload had no memo hits. Without residency there
is no consumer to justify its lookup, retention and bookkeeping costs. Separate
requests now have separate lazy identity cells, including repeated recipes on an
unchanged bank. Linear streams are reconstructed on each request. Only retained
scenes, bindings and interpreter state keep bank allocations alive.

All operation counters use the `tmem.` prefix:

| Counter | Work counted |
| --- | --- |
| `requests`, `rejected_requests` | Interpreter texture-role requests and rejected requests |
| `bank_cow_allocations`, `bank_cow_bytes` | Bank copies made by loads while prior storage is still owned |
| `linear_reconstruction_bytes` | Encoded linear stream bytes constructed for validated requests |
| `hashes_computed`, `bytes_hashed` | Profiled first identity initialization and complete Fast3dV1 preimages hashed |
| `encoded_comparisons`, `bytes_compared` | Direct owned-input comparisons and full equal-length slices passed to equality, independent of early exit |
| `cpu_decode_executions`, `cpu_decode_output_bytes` | Production CPU bridge expansion at upload; untimed inspection is separate |

The removed `memo_*`, `snapshot_allocations`, `snapshot_bytes` and
`witness_comparisons` operations emit no counters or gauges; absence means zero.
`scene_owned_texture_bytes` and `cpu_upload_cache_bytes` count shared allocations
once within each scope, including witnesses only after initialization. The scopes
overlap and must not be added. Their `allocation_overhead_bytes` gauges count
structs, container capacity and Arc control blocks, excluding allocator rounding.

`dispatch-expectations.json` freezes the exact T2 counts. Rust tests consume them
and emit `T2_WORK` JSON under `--nocapture`. Unused identity costs zero snapshots,
hashes or witnesses, including unchanged frames, new recipes, equal-byte reloads,
role aliases and three retained frames of the `tmem-layouts` display list. That
last fixture issues 54 requests with 26 required bank copies and 27 direct bank
comparisons; all identity and memo counts remain zero. Explicit 1×1 I8 identity
access hashes 59 bytes once, including when requested after the bank is gone.
T4's dispatch/residency expectations remain separate. Timed native/Chrome rows
must retain the operation and memory counters with separate instrumentation
controls under the frozen policy.
