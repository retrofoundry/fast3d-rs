# Triangle Example

A standalone example demonstrating how to use fast3d-rs to render a rotating triangle.

![Square example](screenshot.png)

## Features

- Standalone implementation (no framework required)
- Uses winit 0.29 for windowing
- Uses wgpu 25.0 for rendering
- Demonstrates N64 Fast3D RCP emulation
- Shows proper usage of:
  - Display lists
  - Matrix transformations (projection and modelview)
  - Vertex data and geometry
  - RDP/RSP initialization

## Running

```bash
cargo run --example triangle
```

## Code Structure

- `main.rs` - Main application with wgpu/winit setup and fast3d rendering
- `vertices.rs` - Triangle vertex data and helper functions

The example is designed to be easy to copy and adapt for your own projects.
