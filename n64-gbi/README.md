# n64-gbi

N64 GBI vocabulary, command encoders, and libultra-compatible `gu` matrix math.

A dependency leaf, with no dependencies of its own. It holds the parts of the N64 graphics
interface fixed by the hardware and the SDK rather than by any particular consumer:

- `consts` — RDP and RSP opcode and mode vocabulary, per microcode (F3D, F3DEX2).
- `encode` — libultra `gs*` macros as functions returning the exact command words.
- `gu` — `guTranslate`/`guScale`/`guRotate`/`guPerspective`/`guLookAt`, in libultra's
  row-vector convention.

Both halves of the encoding agreement depend on it: the interpreter that decodes display lists
([fast3d](https://github.com/retrofoundry/fast3d-rs)) and any tool that produces them. Its test
suite is a set of literal conformance vectors checked against the libultra macro definitions —
the independent oracle that a round-trip test between an encoder and its own interpreter cannot
provide.
