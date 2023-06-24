# fast3d-rs

fast3d-rs is a library written in Rust for rendering N64 graphics API commands.

## Features

- [x] Several microcodes are supported and more will be added
- [x] OpenGL rendering
- [ ] WGPU rendering

## How to Use

The library consists of three main components:

- `RCP` - This represents the N64 RCP and provides a reset and a run method.
- `GraphicsIntermediateDevice` - This is a component given to the RCP run command that collects draw calls for parsing into different renderers
- `GliumGraphicsDevice` - This is a renderer that can be used to render the draw backend agnostic draw calls produced

<details>
<summary>OpenGL (Glium) Example</summary>
  
```rust
// Prepare the context device
self.graphics_device.start_frame(&mut frame);

// Run the RCP
self.rcp.run(&mut self.intermediate_graphics_device, commands);

// Draw the produced draw calls to context
for draw_call in &self.intermediate_graphics_device.draw_calls {
    assert!(!draw_call.vbo.vbo.is_empty());

    self.graphics_device.set_cull_mode(draw_call.cull_mode);

    self.graphics_device
        .set_depth_stencil_params(draw_call.stencil);

    self.graphics_device.set_blend_state(draw_call.blend_state);
    self.graphics_device.set_viewport(&draw_call.viewport);
    self.graphics_device.set_scissor(draw_call.scissor);

    self.graphics_device.load_program(
        &self.display,
        draw_call.shader_hash,
        draw_call.other_mode_h,
        draw_call.other_mode_l,
        draw_call.geometry_mode,
        draw_call.combine,
    );

    // loop through textures and bind them
    for (index, hash) in draw_call.textures.iter().enumerate() {
        if let Some(hash) = hash {
            let texture = self
                .intermediate_graphics_device
                .texture_cache
                .get_mut(*hash)
                .unwrap();
            self.graphics_device
                .bind_texture(&self.display, index, texture);
        }
    }

    // loop through samplers and bind them
    for (index, sampler) in draw_call.samplers.iter().enumerate() {
        if let Some(sampler) = sampler {
            self.graphics_device.bind_sampler(index, sampler);
        }
    }

    // draw triangles
    self.graphics_device.draw_triangles(
        &self.display,
        target,
        draw_call.projection_matrix,
        &draw_call.fog,
        &draw_call.vbo.vbo,
        &draw_call.uniforms,
    );
}

// Finish rendering
self.graphics_device.end_frame();

// Clear the draw calls
self.intermediate_graphics_device.clear_draw_calls();
```

</details>

<details>
  <summary>WGPU Example</summary>

```rust
```
  
</details>
