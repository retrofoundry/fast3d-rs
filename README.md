# fast3d-rs

fast3d-rs is a library written in Rust for rendering N64 graphics API commands.

## Features

- [x] F3DEX2 microcode supported (more coming)
- [x] WGPU rendering
- [x] OpenGL rendering

## How to Use

The library consists of three main components:

- `RCP` - This represents the N64 RCP and provides a reset and a runDL method.
- `RCPOutput` - This is a component given to the RCP run command that collects draw calls for parsing into different renderers
- `WgpuGraphicsDevice` - This is a renderer that can be used to render the output produced
- `GliumGraphicsDevice` - This is a renderer that can be used to render the output produced

For examples see an example usage here for [wgpu](https://github.com/retrofoundry/helix/blob/main/src/gui/wgpu_renderer.rs) and [opengl](https://github.com/retrofoundry/helix/blob/main/src/gui/glium_renderer.rs).

## Community

[![](https://dcbadge.vercel.app/api/server/nGckYNTp4w)](https://discord.gg/nGckYNTp4w)
