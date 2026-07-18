# fast3d

**A standalone N64 F3DEX2 HLE + wgpu renderer** — hand it a display list from N64 memory and it
draws the frame.

fast3d walks F3DEX2 display lists through a high-level emulation of the N64 RCP and rasterizes them
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
- **Supported microcodes** — `F3dex2` and `F3dex2e` (GBI_FLOATS / PC ports).

Diagnostics stream through a `DiagSink` (`LogSink`, `NopSink`, or your own).

## Features

- **`asm`** *(default)* — assemble display lists from text; not needed when you feed real game DLs.
- **`debug-ui`** — an egui overlay showing per-frame scene and triangle counts.

## Community

[![](https://dcbadge.vercel.app/api/server/nGckYNTp4w)](https://discord.gg/nGckYNTp4w)
