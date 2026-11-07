# Presentation scaling

`RendererConfig::resolution_multiplier` selects a fixed startup scale from 1
through 4. It scales presentation colour and depth, including offscreen targets.
Native rendering still runs for every task. Surface resize does not change the
scale or supply an Auto hint.

`RendererConfig::validate` and `Renderer::new` reject invalid multipliers. The
infallible device constructors and `reconfigure` warn and use 1 for invalid
values. `reconfigure` rebuilds the renderer and drops its framebuffer store.

## Live policy and Auto

`Renderer::set_scale_policy(ScalePolicy::Fixed(m))` validates 1..=4;
`ScalePolicy::Auto` selects a uniform integer scale. A request takes effect at the
next frame boundary. Set the content-area hint before that boundary:

```rust,ignore
renderer.set_scale_policy(ScalePolicy::Auto)?;
renderer.set_output_content_height(content_height);
renderer.begin_frame_with_vi(hardware.vi())?;
// Submit this frame's tasks, then present.
```

Auto computes `max(ceil(content_height / reference_height), 1)`, capped at 4.
Width does not affect this uniform scale. The reference is the selected native VI
crop height, clipped to the retained logical image, with a minimum of 60. Before
any target exists, valid VI registers supply the height. Without a valid current
reference, it uses the previous valid reference, then 240. Legacy rendering uses
its base canvas height. Exclude outer letterboxing and UI from the content hint.

`begin_frame()` uses the previous reference; `begin_frame_with_vi` accepts the
current VI snapshot. VI changes, hints and policy requests after the boundary do
not change the frame's render scale. The effective scale is available before the
first task. Queries are `requested_scale_policy()`,
`effective_resolution_multiplier()`, `auto_reference_height()` (`None` for an
effective fixed frame), and `reduction_factor()`.

The 4x cap keeps Auto within the same attachment envelope as fixed rendering:
colour/depth storage at 4x is 16 times native. RT64's Auto formula has no global
cap and separately limits individual targets. Fast3d keeps one scale for all
targets and rejects allocations that exceed device limits or its 256 MiB
attachment budget. Transition admission includes old and new attachments. A
rejected boundary returns a structured `ResourceAdmission` diagnostic, preserves
the store and effective scale, and rejects tasks until a later boundary succeeds.
`begin_frame()` logs boundary failures; subsequent tasks also report the error.

Scale changes rebuild every retained presentation colour and depth attachment
from native with nearest expansion. Enhanced detail returns as the game redraws.
Native state, ownership, RDP/TMEM, providers and dither seeds survive; the normal
frame serial still advances. Submitted resources stay alive through wgpu's queue
ownership. Immutable load-time snapshots retain their original source scale.
Reset preserves the policy and effective scale, drops framebuffer state, and
forgets the previous reference.

## Box reduction and scanout

`set_reduction_factor(d)` accepts a nonzero integer; the default 1 bypasses the
reduction pass. This setting is independent of render scale and Auto's budget.
At scanout, `d > 1` averages `d × d` linear-UNORM samples in a compute pass before
the final linear blit. The blocks start at the selected crop's scaled origin,
and reads clamp to that crop. Retained rows outside the crop never contribute.

Each reduced dimension is `max(scaled_crop_extent / d, 1)` with integer division,
matching RT64. Non-divisible trailing samples are omitted; factors larger than a
crop dimension repeat its edge samples. The RGBA8 intermediate rounds each
average to UNORM, allowing at most one byte step at a rounding tie. Reduction
does not change native bytes, rendering, TMEM or snapshots.

`present_to`, `present_last_to`, `present_host_to` and owned-surface presentation
use presentation output with the selected reduction. The caller owns the final
output texture and size. `present_native_to` bypasses both scaling and reduction
for native dumps. Replay's `--scale` and replay options remain fixed-only.
Captures record the effective fixed scale; live Auto hints and the reduction
factor are not replay options. Replay uses reduction factor 1. See
[capture and replay](capture.md).

## Native framebuffer effects

Guest descriptors, TMEM addressing, native output, coverage and write-back stay
native. Complete RGBA16 and IA16 framebuffer image loads retain presentation
samples at the ordered load point. Each snapshot keeps its source scale through
later tasks and scale changes. Load-free aliases sample the live presentation
attachment. CI indices, tagged palettes, split pairs, partial or mixed TMEM,
lookup images and requests that reconstruct bytes use native samples for the
entire request. Native and writer-ID draws always bind native sources.

The attachment budget covers retained native and presentation colour, shared
depth at 9 bytes per pixel (Z, dz and companion depth), versions, allocation growth
and strict-task staging reservations. Snapshot and decode buffers are checked
separately against device limits; admission never reduces a target's scale.
