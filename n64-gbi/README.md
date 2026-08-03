# n64-gbi

N64 GBI vocabulary, command encoders, and libultra-compatible `gu` matrix math.

A dependency leaf, with no dependencies of its own. It holds the parts of the N64 graphics
interface fixed by the hardware and the SDK rather than by any particular consumer:

- `consts` — RDP and RSP opcode and mode vocabulary, per microcode (F3D, F3DEX2).
- `encode` — libultra `gs*` macros as functions returning the exact command words.
- `gu` — `guTranslate`/`guScale`/`guRotate`/`guPerspective`/`guLookAt`, in libultra's
  row-vector convention.
- `texel` — total, allocation-free packing of already-selected texel components and CI indices.
- `vectors` — versioned literal conformance vectors with source and derivation metadata.

Wire packers mask inputs to their documented field widths; callers choose whether to reject
out-of-range authoring values before packing. New TLUT producers should use
`encode::gdp_load_tlut_cmd`. The similarly named `gdp_load_tlut` is deprecated and retained only
for display lists emitted by fast3d's pre-migration assembler.

Both halves of the encoding agreement depend on it: the interpreter that decodes display lists
([fast3d](https://github.com/retrofoundry/fast3d-rs)) and any tool that produces them. Its
conformance vectors pin independently selected literals with human-auditable SDK, header, or
hardware-reference citations. Offline tests compare every primitive with those literals and
validate the metadata structure; they do not claim to fetch or authenticate the cited documents.
