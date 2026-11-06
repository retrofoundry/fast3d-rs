# Bounded framebuffer texture aliases

A load-free TexRect can sample a previously rendered colour image at its exact base
address. This is a fast3d convenience. It does not implement libultra framebuffer
LoadTile or LoadBlock semantics and must not be exported as an rt64 agreement test.

The render tile must have no written byte in its reachable TMEM footprint. Its format and texel size must
match the source. SETTIMG must match the stored colour image layout. The tile
line addresses TMEM and does not set the stride of this load-free GPU view; the rectangle's
texel coordinates are scaled by the source extent, not the tile's. The
requested extent must fit the source. Prepared draws identify
the source generation, layout and extent; submission validates the stored view.
Successfully loaded TMEM remains independent of subsequent SETTIMG commands.

Same-target feedback, interior addresses, overlapping views, reinterpretation,
depth textures and second-texture inputs are rejected with a structured diagnostic.
An address outside known targets follows ordinary texture validation. MissingSource
is reserved for an explicit framebuffer reference that cannot be resolved.
Known GPU colour and depth ranges are checked before
LoadTile, LoadBlock or LoadTLUT reads guest memory. Unsupported loads mark the
affected TMEM bytes unavailable, including masked taps, so later dependent draws
cannot sample stale RAM or older TMEM contents. A successful replacement load can
restore those bytes. Unrelated draws still execute. An actual guest-memory read
failure rejects the task.

Diagnostics retain the command PC, including across tasks; preparation errors reach
the caller's DiagSink before DlSummary is calculated. Format or stride changes end
the stored generation. Height growth preserves existing rows.

Target descriptors and the colour-switch counter are interpreter context, separate
from guest RDP registers and TMEM. Normal submissions start from the renderer's
target cache; prefix walks start with empty target history. Capture admission
compares interpretation and prepared inputs using the live initial history and an
empty replay history, advancing each between tasks. Switch epochs are ignored in
that comparison because capture recording excludes diagnostic depth-reset policies.

Real LoadTile, LoadBlock and LoadTLUT commands use ordered GPU snapshots separately
from this convenience. They support interior offsets, source strides, rectangle and
triangle consumers, exact RGBA16/IA16/CI8 reinterpretation, and other formats through
quantised RGBA16 bytes. The existing offscreen-then-sample golden remains a
compatibility test for the load-free path.
[The authored real-load fixtures](framebuffer-load-evidence.md) check snapshot order
and the unquantised render-target convention. GPU write-back and guest imports remain
opt-in; framebuffer loads do not require that switch.

At a fixed presentation scale above 1, complete RGBA16 and IA16 image loads retain
the presentation subrectangle after both producer passes and before later writes.
Native origin, extent and source scale belong to the retained snapshot. Its scale
does not depend on a later consumer's scale. Load-free aliases instead sample the
current presentation attachment, including writes after a load. Native and
writer-ID draws bind native views in both cases.

CI indices, tagged palettes, split pairs, partial or mixed TMEM and cross-copy
byte reconstruction use native samples. A request combining image branches and
byte reconstruction stays native as a whole. TMEM lookup images also stay native.
These routes keep their native sampling precision at every presentation scale.
