//! CPU-only observation of the existing display-list interpreter.

use std::ops::ControlFlow;

use crate::{DataFormat, Diagnostic, Microcode, Rdram};

pub use crate::hle::rdp::TileDescriptor;
pub use crate::hle::rsp::TextureState;
pub use crate::scene::{ColorImage, Rect, Scissor, TexRectBounds};

/// Maximum dispatched commands in a public walk; this is not a time or heap limit.
pub const MAX_DISPATCHES: u32 = 4096;

/// Interpreter matrix layout, without transposition.
pub type Matrix4 = [[f32; 4]; 4];

/// A named, single-bit geometry flag for the selected microcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryFlag {
    pub mask: u32,
    pub name: &'static str,
}

/// A successfully read command record, preserving its full address operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandWord {
    /// Address of this record in the reader’s command stream.
    pub pc: u64,
    pub w0: u32,
    pub w1: u32,
    /// Full-width address operand, which may exceed the raw `w1`.
    pub w1_addr: u64,
}

/// Control flow after a dispatched command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkFlow {
    Next,
    Call,
    Branch,
    Return,
    End,
    Fault,
}

/// Why execution stopped, including partial walks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WalkTermination {
    #[default]
    End,
    Bounds,
    Runaway,
    ObserverStopped,
    Cap,
}

/// Post-command state. All borrows expire when the observer callback returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateView<'a> {
    pub geometry_mode: u32,
    /// Set single-bit flags in ascending mask order; unknown bits remain in `geometry_mode`.
    pub geometry_names: &'a [GeometryFlag],
    pub texture: TextureState,
    /// Occupied modelview stack entries, initially one.
    pub modelview_depth: usize,
    pub modelview: &'a Matrix4,
    pub projection: &'a Matrix4,
    /// Decoded interpreter viewport scale, in its existing units.
    pub viewport_scale: [f32; 3],
    /// Decoded interpreter viewport translation, in its existing units.
    pub viewport_translation: [f32; 3],
    /// Number of active directional lights; ambient is separate.
    pub light_count: u32,
    /// Light tuples are `(direction, color)` in decoded interpreter units.
    pub lights: &'a [([f32; 3], [f32; 3]); 8],
    pub ambient: [f32; 3],
    pub lookat_axes: &'a [[f32; 3]; 2],
    pub tiles: &'a [TileDescriptor; 8],
    pub load_via_tile: bool,
    pub texture_image: ColorImage,
    /// Stored combine `w0`, despite the `_l` suffix.
    pub combine_l: u32,
    /// Stored combine `w1`, despite the `_h` suffix.
    pub combine_h: u32,
    pub other_mode_h: u32,
    pub other_mode_l: u32,
    pub prim_color: [u8; 4],
    pub env_color: [u8; 4],
    pub fog_color: [u8; 4],
    pub blend_color: [u8; 4],
    pub fill_color_raw: u32,
    pub color_image: ColorImage,
    pub depth_image: u64,
    pub scissor: Scissor,
}

/// The actual framebuffer pair receiving an emitted primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramebufferTarget {
    pub pair_index: usize,
    pub color_image: ColorImage,
    pub depth_image: Option<u64>,
    pub is_depth_clear: bool,
}

/// New HLE output from one dispatch; this does not describe visible pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Emission<'a> {
    Triangles {
        /// `None` for triangles emitted before the first color-image command.
        target: Option<FramebufferTarget>,
        /// Offset of these newly appended indices in the scene index buffer.
        index_start: u32,
        indices: &'a [u32],
    },
    FillRect {
        target: FramebufferTarget,
        rect: Rect,
        color_raw: u32,
    },
    TexRect {
        target: FramebufferTarget,
        rect: TexRectBounds,
        tile: u8,
        uls: i16,
        ult: i16,
        dsdx: i16,
        dtdy: i16,
        flip: bool,
        copy_mode: bool,
        fb_source: Option<u64>,
    },
}

/// One post-command callback; continuation words share their parent dispatch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkStep<'a> {
    /// Zero-based execution identity; addresses may repeat.
    pub seq: u32,
    /// Original dispatch address, even when the command changes control flow.
    pub pc: u64,
    /// One to three successfully read records at the reader’s actual command stride.
    pub words: &'a [CommandWord],
    /// Return-stack length before dispatch; root depth is zero.
    pub depth_before: usize,
    /// Return-stack length after dispatch; root depth is zero.
    pub depth_after: usize,
    pub flow: WalkFlow,
    /// Resolved continuation; `None` for root end or a terminating fault.
    pub next_pc: Option<u64>,
    pub state: StateView<'a>,
    pub emissions: &'a [Emission<'a>],
    /// Start of this step’s diagnostics in the complete summary vector.
    pub diagnostics_start: usize,
    pub diagnostics: &'a [Diagnostic],
}

/// Receives borrowed steps; copy any data retained after the callback.
pub trait WalkObserver {
    /// Return `Break(())` to cancel after this command; root end and faults take precedence.
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()>;
}

/// Counts and diagnostics from a completed or partial CPU walk.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkSummary {
    pub dispatched: u32,
    /// Total emitted indices divided by three.
    pub tris: u32,
    /// Existing interpreter count of discarded draws.
    pub dropped_runs: u32,
    /// Interpreter diagnostics in their original order, without further deduplication.
    pub diagnostics: Vec<Diagnostic>,
    /// Start of terminal diagnostics outside all step slices.
    pub final_diagnostics_start: usize,
    pub termination: WalkTermination,
}

/// Walk on the CPU without a renderer or GPU device, stopping after at most 4,096 dispatches.
pub fn walk<M: Rdram>(
    mem: M,
    entry: u64,
    microcode: Microcode,
    format: DataFormat,
    observer: &mut dyn WalkObserver,
) -> WalkSummary {
    let mut adapter = CappedObserver {
        observer,
        capped: false,
    };
    let result = crate::hle::interpret(mem, entry, microcode.into(), format, Some(&mut adapter));
    let termination = if result.termination == WalkTermination::ObserverStopped && adapter.capped {
        WalkTermination::Cap
    } else {
        result.termination
    };
    WalkSummary {
        dispatched: result.commands,
        tris: (result.scene.indices.len() / 3) as u32,
        dropped_runs: result.dropped_runs,
        diagnostics: result.diags,
        final_diagnostics_start: result.final_diagnostics_start,
        termination,
    }
}

struct CappedObserver<'a> {
    observer: &'a mut dyn WalkObserver,
    capped: bool,
}

impl WalkObserver for CappedObserver<'_> {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self.observer.command(step)?;
        if step.seq + 1 >= MAX_DISPATCHES {
            self.capped = true;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

pub(crate) fn geometry_flags(
    microcode: Microcode,
    mode: u32,
    storage: &mut [GeometryFlag; 12],
) -> &[GeometryFlag] {
    macro_rules! flags {
        ($module:ident; $($name:ident),* $(,)?) => {{
            use n64_gbi::consts::$module::*;
            &[$(GeometryFlag { mask: $name, name: stringify!($name) }),*]
        }};
    }
    let flags: &[GeometryFlag] = match microcode {
        Microcode::F3dex2 => flags!(rsp_f3dex2;
            G_ZBUFFER, G_SHADE, G_CULL_FRONT, G_CULL_BACK, G_FOG, G_LIGHTING,
            G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR, G_SHADING_SMOOTH, G_CLIPPING),
        Microcode::F3d => flags!(rsp_f3d;
            G_ZBUFFER, G_TEXTURE_ENABLE, G_SHADE, G_SHADING_SMOOTH, G_CULL_FRONT,
            G_CULL_BACK, G_FOG, G_LIGHTING, G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR,
            G_POINT_LIGHTING, G_CLIPPING),
    };
    let mut len = 0;
    for flag in flags {
        if mode & flag.mask != 0 {
            storage[len] = *flag;
            len += 1;
        }
    }
    &storage[..len]
}
