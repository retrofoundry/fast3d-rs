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

## Inspect a display list

```rust
use std::ops::ControlFlow;
use fast3d::{DataFormat, Microcode, RdramImage};
use fast3d::inspect::{walk, WalkObserver, WalkStep, WalkTermination};

struct Counter(u32);

impl WalkObserver for Counter {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        assert_eq!(step.seq, self.0);
        self.0 += 1;
        ControlFlow::Continue(())
    }
}

let bytes = [0xDF, 0, 0, 0, 0, 0, 0, 0]; // F3DEX2 ENDDL
let mut counter = Counter(0);
let summary = walk(
    RdramImage::new(&bytes),
    0,
    Microcode::F3dex2,
    DataFormat::Fixed,
    &mut counter,
);
assert_eq!(counter.0, 1);
assert_eq!(summary.termination, WalkTermination::End);
```

Inspection walks on the CPU without a renderer or GPU device. Callbacks borrow state after each dispatched command; copy data you retain. Continuation words share their parent step, and emissions describe HLE output. Return `ControlFlow::Break(())` to cancel. Walks stop after at most 4,096 dispatches and report whether they completed, faulted, were cancelled, or reached the cap.

## Features

- **`debug-ui`** — an egui overlay showing per-frame scene and triangle counts.

## Layout

- `fast3d/` — the HLE interpreter, wgpu renderer, and `Renderer` facade.
- `n64-gbi/` — dependency leaf: GBI/RDP/RSP vocabulary, command encoders, libultra `gu` math,
  and the literal conformance vectors. No dependencies. Consumers that produce or inspect
  display lists should depend on this directly rather than on `fast3d`.

## Community

[![](https://dcbadge.vercel.app/api/server/nGckYNTp4w)](https://discord.gg/nGckYNTp4w)
