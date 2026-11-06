# Fixed presentation scales

`RendererConfig::resolution_multiplier` accepts integers from 1 through 4. The
multiplier scales presentation colour and depth attachments, including offscreen
framebuffers. Native rendering still runs for every task. Surface resize does not
change the scale.

`RendererConfig::validate` and `Renderer::new` reject multipliers outside that
range. The infallible device constructors and `reconfigure` warn and use 1 for
invalid values. Query the applied value with
`Renderer::effective_resolution_multiplier()`. Reset preserves the scale.
`reconfigure` rebuilds the renderer and drops its framebuffer store; there is no
live scale setter.

Guest image descriptors, TMEM addressing, native output, coverage and write-back
stay at native resolution. Complete RGBA16 and IA16 framebuffer image loads retain
presentation samples at the ordered load point. Each snapshot keeps its source
scale through later tasks and reconfiguration. Load-free aliases sample the live
presentation attachment. CI indices, tagged palettes, split pairs, partial or
mixed TMEM, lookup images and requests that reconstruct any bytes use native
samples for the entire request. Native draws and writer-ID draws always bind
native sources.

Attachment admission has an internal 256 MiB budget for retained native and
presentation colour, shared depth and allocation growth. A task exceeding the
budget or device limits is rejected without reducing the scale. Snapshot and
decode buffer sizes are checked separately against device limits.

`Renderer::present_to` selects presentation output. `Renderer::present_native_to`
selects native output for dumps. The caller supplies the output texture and its
size. Capture replay also offers named native and presentation sources; see
[capture and replay](capture.md).
