# Bounded framebuffer texture aliases

A load-free TexRect can sample a previously rendered colour image at its exact base
address. This is a fast3d convenience. It does not implement libultra framebuffer
LoadTile or LoadBlock semantics and must not be exported as an rt64 agreement test.

The render tile must have no TMEM load provenance. Its format and texel size must
match the source. SETTIMG must match the stored colour image layout. The tile
line addresses TMEM and does not set the stride of this load-free GPU view. The
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

General range ownership, offset copies, triangle framebuffer textures, same-target
snapshots, bit reinterpretation, CPU/GPU coherence and GPU write-back remain
unsupported. The existing offscreen-then-sample golden is a compatibility test for
the convenience only. Real-load oracle fixtures require a separate implementation
and evidence; none is claimed here.
