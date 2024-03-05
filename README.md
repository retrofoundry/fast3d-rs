# fast3d-rs

fast3d-rs is a library written in Rust for rendering N64 graphics API commands.

## Features

- [x] F3DEX2 microcode supported (more coming)
- [x] WGPU rendering
- [x] OpenGL rendering

## How to Use

Add this library to your project and one of the following renderers: `fast3d-wgpu-renderer` or `fast3d-glium-renderer`.

The library consists of three main components:

- `RCP` - This represents the N64 RCP and provides a reset and a `process_dl` method.
- `RenderData` - This is given to the RCP run command that collects draw data and textures for rendering with different renderers.
- `WgpuRenderer` - This is a renderer that can be used to render data produced
- `GliumRenderer` - This is a renderer that can be used to render data produced

Check out the examples folder for some examples of how to use the library.

_Looking for a solution that includes this, windowing, audio and controller input? Check out [Helix](https://github.com/retrofoundry/helix)!._

## Examples

You can run our examples using the following command:

```bash
cargo run --bin fast3d-examples <example>
```

To run on web:

```bash
cargo xtask run-wasm
```

## Community

[![](https://dcbadge.vercel.app/api/server/nGckYNTp4w)](https://discord.gg/nGckYNTp4w)
