# fast3d

**A standalone N64 HLE + wgpu renderer** — hand it a display list from N64 memory and it
draws the frame.

fast3d walks N64 display lists through a high-level emulation of the RCP and rasterizes them
with wgpu, so the same renderer runs natively and on the web (WebGPU/wasm).

## Use it

```toml
[dependencies]
fast3d = "1.0"
```

Implement `Hardware` to expose N64 memory, then drive the frame loop:

```rust
// once
let mut renderer = fast3d::Renderer::new(window, width, height, config).await?;

// per frame
renderer.begin_frame();
renderer.process_dl(&hw, dl_entry, fast3d::Microcode::F3dex2, &mut diags);
renderer.present(&hw)?;
```

- **`Hardware`** — your bridge to guest memory. `rdram()` returns an `Rdram` reader —
  `RdramImage::new(&bytes)` or your own safe reader — and `vi()` gives the VI registers
  that pick the scanout framebuffer. Native pointer graphs use the unsafe host entry below.
- **`begin_frame` → `process_dl` → `present`** — reset per-frame state, interpret one display list
  into the internal framebuffer, then scan the VI framebuffer out to the owned surface (or
  `present_to` a view you own).
- **Supported microcodes** — `F3dex2` and `F3d`. The fixed-vs-float vertex/matrix layout is an
  orthogonal `DataFormat` axis: `Fixed` (authentic N64, the default) or `Float` (`GBI_FLOATS`, as
  PC ports like sm64/wafel emit) — select it once with `Renderer::set_data_format`.

Diagnostics stream through a `DiagSink` (`LogSink`, `NopSink`, or your own).

## Memory readers and native ports

`Rdram` reads and address resolution return `Result<_, MemoryError>`.
`read_bytes(address, length)` returns exactly `length` bytes or an error;
`in_bounds` is advisory and overflow-safe. Errors carry the requested `u64`
address, byte length and a `MemoryErrorKind`, without a command PC or logging.
`Command`, `RawVertex` and `Matrix` are exported from both `fast3d` and
`fast3d::hardware`. They are decoded values, not guest layouts for casting.
`Command::w1` contains numeric low bits; address operands use `w1_addr`.
`Matrix` is `[[f32; 4]; 4]`.

IMAGE readers decode big-endian Fixed data. Segment writes store the raw low
32 bits; resolution uses bits 24–27 to select a segment and adds the low 24 bits
with intentional 32-bit wrapping. Masked resolution then applies `0x00ff_fff8`.
Operands above `u32::MAX` are rejected. Float matrix/vertex requests are errors;
custom readers can implement their own decoded layouts.

A failed read produces `DiagKind::MemoryRead { access, error }` at the command
that initiated it. The first memory failure discards that task before GPU
submission, with `errors > 0`, `tris = 0` and `renderable = false`. Earlier tasks
remain visible. No source memory is read during presentation.

`HostRam` is a safe descriptor with initial `segments: [u64; 16]`; it no longer
implements `Rdram`. Replace native `Hardware` adapters with:

```rust
renderer.set_data_format(fast3d::DataFormat::Fixed);
renderer.begin_frame();
let ram = fast3d::HostRam::new(&[]);
// SAFETY: the submitting guest is blocked; every reachable input stays valid until return.
let summary = unsafe {
    renderer.process_dl_host(ram, entry, fast3d::Microcode::F3dex2, &mut diags)
};
// The guest can resume here.
renderer.present_last()?;
```

Native commands are 16-byte native-endian words with full-width addresses.
Reads allow unaligned inputs; Fixed matrices use native packed words, and
Fixed/Float vertices have 16/24-byte strides. Every reachable input must be
allocated, readable, initialized in that layout and stable until consumption
returns. The lifetime witness does not establish pointer validity, and the
dispatch cap only bounds liveness. CIMG/ZIMG are GPU target identities and need
not be readable unless a command uses them as input. `present_last_to(view)`
scans out to a caller-owned view.

With `capture`, use the equivalent unsafe
`CaptureFrame::process_dl_host(&mut renderer, ram, entry, microcode, data_format, &mut diags)`.
It owns all recorded spans before returning; `present_last(&mut renderer)` or
`present_last_to(&mut renderer, view)` and fixture serialization can run after
the guest resumes. Replay reads owned spans through the safe fallible API.

## Convert and key registers

`G_SETCONVERT` stores six signed nine-bit coefficients (-256..255).
`G_SETKEYR` and `G_SETKEYGB` retain each channel's centre, scale and 12-bit width.
Draws snapshot these registers, including rectangles. Unused setters are silent.

These combiner inputs are recognised but remain unwired:

| Input | Colour slot / encoding | Active-draw diagnostic |
| --- | --- | --- |
| `KEY_CENTER` | B / 6 | `UnsupportedKeyInput { selector: KeyInput::Center }` |
| `KEY_SCALE` | C / 6 | `UnsupportedKeyInput { selector: KeyInput::Scale }` |
| K4 | B / 7 | `UnsupportedConvertInput { selector: ConvertInput::K4 }` |
| K5 | C / 15 | `UnsupportedConvertInput { selector: ConvertInput::K5 }` |

One-cycle mode checks cycle 1; two-cycle mode checks both cycles. Copy and fill
modes bypass the combiner. Inactive selectors do not reject a draw.

Writing TEXTCONV to a value other than `G_TC_FILT` rejects subsequent draws with
`UnsupportedTextureConversion`; enabling `G_CK_KEY` rejects them with
`UnsupportedChromaKey`. The unwritten TEXTCONV default retains the library's
existing rendering behaviour. All these diagnostics have Error severity and
drop the draw before shader preparation. Selector enums live in `fast3d::diag`.

## Features

- **`debug-ui`** — an egui overlay showing per-frame scene and triangle counts.

## Layout

- `fast3d/` — the HLE interpreter, wgpu renderer, and `Renderer` facade.
- `n64-gbi/` — dependency leaf: GBI/RDP/RSP vocabulary, command encoders, libultra `gu` math,
  and the literal conformance vectors. No dependencies. Consumers that produce or inspect
  display lists should depend on this directly rather than on `fast3d`.

## Community

[![](https://dcbadge.vercel.app/api/server/nGckYNTp4w)](https://discord.gg/nGckYNTp4w)
