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
  `RdramImage::new(&bytes)` (safe, borrowed) or `unsafe HostRam::new(..)` (raw pointer, native
  64-bit only) — and `vi()` gives the VI registers that pick the scanout framebuffer.
- **`begin_frame` → `process_dl` → `present`** — reset per-frame state, interpret one display list
  into the internal framebuffer, then scan the VI framebuffer out to the owned surface (or
  `present_to` a view you own).
- **Supported microcodes** — `F3dex2` and `F3d`. The fixed-vs-float vertex/matrix layout is an
  orthogonal `DataFormat` axis: `Fixed` (authentic N64, the default) or `Float` (`GBI_FLOATS`, as
  PC ports like sm64/wafel emit) — select it once with `Renderer::set_data_format`.

Diagnostics stream through a `DiagSink` (`LogSink`, `NopSink`, or your own).

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
