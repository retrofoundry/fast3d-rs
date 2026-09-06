//! Structured, `Copy` diagnostics for the DL walk (spec §3.6). Every legacy string diagnostic
//! becomes a `DiagKind` variant; the human string is rendered on demand via `Display`.

/// A diagnostic emitted during a display-list walk. `at` is the command's byte address in the DL
/// address space (a physical RDRAM offset for `RdramImage`, a raw host pointer for `HostRam`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub at: u64,
    pub kind: DiagKind,
}

/// The kind of a diagnostic. `Copy` (no per-diag allocation). `#[non_exhaustive]`: the walk's
/// internal set may grow. The variable-length unwired-selector list is a `Copy` bitmask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagKind {
    UnknownOpcode(u8),
    UnsupportedCommand {
        opcode: u8,
        w0: u32,
        w1: u64,
    },
    UnsupportedMicrocodeLoad {
        w0: u32,
        w1: u64,
        data_address: Option<u64>,
    },
    UnsupportedTextureFormat {
        fmt: u8,
        siz: u8,
    },
    UnsupportedPrimitiveDepthSource,
    UnsupportedCommandParameters {
        opcode: u8,
    },
    RunawayDl {
        cap: u64,
    },
    DlPastRdram,
    TruncatedRect {
        fill: bool,
    },
    DrawBeforeCimg,
    RenderModeNeverSet,
    VtxOutOfRange {
        count: u32,
        end: u32,
    },
    InvalidModifyVertex {
        index: u32,
        attribute: u32,
    },
    InvalidCullRange {
        first: u32,
        last: u32,
    },
    InvalidConditionalVertex {
        opcode: u8,
        index: u32,
    },
    InvalidVertexTransform {
        index: u32,
    },
    MissingBranchTarget,
    UnhandledMovemem(u8),
    UnhandledMoveword(u8),
    NonCanonicalBlend,
    StrayRdphalf,
    NoTextureLoaded,
    /// A two-texture material's second texture (TEXEL1) tile cannot take the faithful `sample_tile`
    /// path; the legacy fallback ignores `tmem_addr` and would read tex0's TMEM, so we refuse-to-draw
    /// rather than mis-decode it.
    SecondTextureUndecodable,
    /// CA/CB/CC/CD/AA/AB/AC/AD slots: low eight bits cycle 1, high eight bits cycle 0.
    UnwiredSelector {
        slots: u16,
    },
}

/// Combiner selector slot names, in bit order (bit 0 = CA … bit 7 = AD). Shared with
/// `combiner::CycleSel::unwired_mask` (which produces the mask this decodes).
const SEL_SLOTS: [&str; 8] = ["CA", "CB", "CC", "CD", "AA", "AB", "AC", "AD"];

fn unwired_slot_names(slots: u16) -> Vec<String> {
    (0..16)
        .filter(|i| slots & (1 << i) != 0)
        .map(|i| format!("cycle {} {}", if i < 8 { 1 } else { 0 }, SEL_SLOTS[i % 8]))
        .collect()
}

impl DiagKind {
    /// Severity is a pure function of kind (no stored field to desync). Every variant that
    /// drops/aborts draw data is `Error`; only cosmetic ones are `Warn` (spec §3.6).
    pub fn severity(&self) -> Severity {
        match self {
            DiagKind::UnsupportedCommand { opcode: 0xd6, .. } => Severity::Warn,
            DiagKind::UnknownOpcode(_)
            | DiagKind::UnsupportedCommand { .. }
            | DiagKind::UnsupportedMicrocodeLoad { .. }
            | DiagKind::UnsupportedTextureFormat { .. }
            | DiagKind::UnsupportedPrimitiveDepthSource
            | DiagKind::UnsupportedCommandParameters { .. }
            | DiagKind::RunawayDl { .. }
            | DiagKind::DlPastRdram
            | DiagKind::TruncatedRect { .. }
            | DiagKind::DrawBeforeCimg
            | DiagKind::VtxOutOfRange { .. }
            | DiagKind::InvalidModifyVertex { .. }
            | DiagKind::InvalidCullRange { .. }
            | DiagKind::InvalidConditionalVertex { .. }
            | DiagKind::InvalidVertexTransform { .. }
            | DiagKind::MissingBranchTarget
            | DiagKind::NoTextureLoaded
            | DiagKind::SecondTextureUndecodable
            | DiagKind::UnhandledMovemem(_)
            | DiagKind::UnhandledMoveword(_)
            | DiagKind::UnwiredSelector { .. } => Severity::Error,
            DiagKind::RenderModeNeverSet | DiagKind::NonCanonicalBlend | DiagKind::StrayRdphalf => {
                Severity::Warn
            }
        }
    }
}

impl std::fmt::Display for DiagKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagKind::UnknownOpcode(op) => write!(f, "unknown opcode 0x{op:02X}"),
            DiagKind::UnsupportedCommand { opcode, w0, w1 } => {
                write!(
                    f,
                    "unsupported opcode 0x{opcode:02X}: w0={w0:#010x}, w1={w1:#018x}"
                )
            }
            DiagKind::UnsupportedMicrocodeLoad {
                w0,
                w1,
                data_address,
            } => {
                write!(
                    f,
                    "unsupported microcode load: w0={w0:#010x}, w1={w1:#018x}, "
                )?;
                match data_address {
                    Some(address) => write!(f, "data={address:#018x}"),
                    None => write!(f, "missing RDPHALF_1 data address"),
                }
            }
            DiagKind::UnsupportedTextureFormat { fmt, siz } => {
                write!(f, "unsupported texture format: fmt={fmt}, siz={siz}")
            }
            DiagKind::UnsupportedPrimitiveDepthSource => {
                write!(f, "primitive depth source is unsupported")
            }
            DiagKind::UnsupportedCommandParameters { opcode } => {
                write!(f, "unsupported parameters for opcode 0x{opcode:02X}")
            }
            DiagKind::RunawayDl { cap } => {
                write!(f, "runaway DL: exceeded {cap} command dispatches")
            }
            DiagKind::DlPastRdram => write!(f, "DL ran past RDRAM"),
            DiagKind::TruncatedRect { fill } => {
                write!(
                    f,
                    "truncated {} continuation",
                    if *fill { "FILLRECT" } else { "TEXRECT" }
                )
            }
            DiagKind::DrawBeforeCimg => write!(f, "draw before first CIMG"),
            DiagKind::RenderModeNeverSet => {
                write!(
                    f,
                    "geometry drawn but render mode never set (other_mode_l == 0)"
                )
            }
            DiagKind::VtxOutOfRange { count, end } => {
                write!(f, "G_VTX out of range: count={count}, end={end}")
            }
            DiagKind::InvalidModifyVertex { index, attribute } => {
                write!(f, "invalid MODIFYVTX slot or attribute: index={index}, attribute={attribute:#04x}")
            }
            DiagKind::InvalidCullRange { first, last } => {
                write!(f, "invalid CULLDL range: first={first}, last={last}")
            }
            DiagKind::InvalidConditionalVertex { opcode, index } => {
                write!(f, "invalid vertex for opcode 0x{opcode:02X}: index={index}")
            }
            DiagKind::InvalidVertexTransform { index } => {
                write!(f, "invalid vertex transform: index={index}")
            }
            DiagKind::MissingBranchTarget => write!(f, "BRANCH_Z missing RDPHALF_1 target"),
            DiagKind::UnhandledMovemem(idx) => write!(f, "unhandled MOVEMEM index 0x{idx:02X}"),
            DiagKind::UnhandledMoveword(ty) => write!(f, "unhandled G_MOVEWORD type 0x{ty:02X}"),
            DiagKind::NonCanonicalBlend => {
                write!(
                    f,
                    "non-canonical blended render mode clamps to AlphaOver fallback"
                )
            }
            DiagKind::StrayRdphalf => write!(f, "stray RDPHALF — rect decode desync"),
            DiagKind::NoTextureLoaded => {
                write!(
                    f,
                    "no texture loaded (tmem is empty; LoadBlock not executed)"
                )
            }
            DiagKind::SecondTextureUndecodable => {
                write!(
                    f,
                    "second texture (TEXEL1) tile cannot take the faithful sample path; refusing to draw"
                )
            }
            DiagKind::UnwiredSelector { slots } => {
                write!(
                    f,
                    "combiner selector not implemented: {:?}",
                    unwired_slot_names(*slots)
                )
            }
        }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} @ {:#018x}", self.kind, self.at)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warn,
    Error,
}

/// A destination for streamed diagnostics. `process_dl` streams into a caller-supplied `&mut dyn`.
pub trait DiagSink {
    fn emit(&mut self, diag: Diagnostic);
}
impl DiagSink for Vec<Diagnostic> {
    fn emit(&mut self, d: Diagnostic) {
        self.push(d);
    }
}
/// Discards diagnostics (helix: `&mut NopSink`).
pub struct NopSink;
impl DiagSink for NopSink {
    fn emit(&mut self, _: Diagnostic) {}
}
/// Routes each diagnostic to `log` at a level chosen by its severity (uses `Display for Diagnostic`,
/// which already appends `@ {addr}` — do NOT append it again).
pub struct LogSink;
impl DiagSink for LogSink {
    fn emit(&mut self, d: Diagnostic) {
        match d.kind.severity() {
            Severity::Error => log::error!("{d}"),
            Severity::Warn => log::warn!("{d}"),
        }
    }
}

/// A `Copy` rollup returned by `process_dl` so a `NopSink` caller still learns the outcome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DlSummary {
    pub commands: u32,
    pub tris: u32,
    pub warns: u32,
    pub errors: u32,
    pub dropped_runs: u32,
    pub renderable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_maps_every_variant_per_spec() {
        for k in [
            DiagKind::UnknownOpcode(0),
            DiagKind::UnsupportedCommand {
                opcode: 0x08,
                w0: 0,
                w1: 0,
            },
            DiagKind::UnsupportedMicrocodeLoad {
                w0: 0,
                w1: 0,
                data_address: None,
            },
            DiagKind::UnsupportedTextureFormat { fmt: 1, siz: 2 },
            DiagKind::UnsupportedPrimitiveDepthSource,
            DiagKind::UnsupportedCommandParameters { opcode: 0xf0 },
            DiagKind::RunawayDl { cap: 1 },
            DiagKind::DlPastRdram,
            DiagKind::TruncatedRect { fill: false },
            DiagKind::TruncatedRect { fill: true },
            DiagKind::DrawBeforeCimg,
            DiagKind::VtxOutOfRange { count: 1, end: 2 },
            DiagKind::InvalidCullRange { first: 2, last: 1 },
            DiagKind::InvalidConditionalVertex {
                opcode: 0x04,
                index: 3,
            },
            DiagKind::InvalidVertexTransform { index: 0 },
            DiagKind::MissingBranchTarget,
            DiagKind::NoTextureLoaded,
            DiagKind::SecondTextureUndecodable,
            DiagKind::UnhandledMovemem(0),
            DiagKind::UnhandledMoveword(0),
            DiagKind::UnwiredSelector { slots: 0b0100 },
        ] {
            assert_eq!(k.severity(), Severity::Error, "{k} must be Error");
        }
        for k in [
            DiagKind::UnsupportedCommand {
                opcode: 0xd6,
                w0: 0,
                w1: 0,
            },
            DiagKind::RenderModeNeverSet,
            DiagKind::NonCanonicalBlend,
            DiagKind::StrayRdphalf,
        ] {
            assert_eq!(k.severity(), Severity::Warn, "{k} must be Warn");
        }
        assert!(Severity::Warn < Severity::Error, "Severity is Ord");
    }

    #[test]
    fn display_renders_kind_alone_and_diagnostic_with_address() {
        assert_eq!(
            DiagKind::UnknownOpcode(0xAB).to_string(),
            "unknown opcode 0xAB"
        );
        assert_eq!(
            DiagKind::UnwiredSelector { slots: 0b0100 }.to_string(),
            "combiner selector not implemented: [\"cycle 1 CC\"]"
        );
        for (kind, expected) in [
            (
                DiagKind::UnsupportedCommand { opcode: 0xd5, w0: 0xd512_3456, w1: 0x1234_5678_ffff_ffff },
                "unsupported opcode 0xD5: w0=0xd5123456, w1=0x12345678ffffffff",
            ),
            (
                DiagKind::UnsupportedTextureFormat { fmt: 7, siz: 3 },
                "unsupported texture format: fmt=7, siz=3",
            ),
            (
                DiagKind::UnsupportedMicrocodeLoad { w0: 0xdd00_07ff, w1: 0x1234_5678_0000_0000, data_address: Some(0x9876_5432_0000_0000) },
                "unsupported microcode load: w0=0xdd0007ff, w1=0x1234567800000000, data=0x9876543200000000",
            ),
            (
                DiagKind::UnsupportedMicrocodeLoad { w0: 0xdd00_0000, w1: 0, data_address: None },
                "unsupported microcode load: w0=0xdd000000, w1=0x0000000000000000, missing RDPHALF_1 data address",
            ),
            (DiagKind::UnhandledMovemem(0x7f), "unhandled MOVEMEM index 0x7F"),
            (DiagKind::UnhandledMoveword(0x7f), "unhandled G_MOVEWORD type 0x7F"),
        ] {
            assert_eq!(kind.to_string(), expected);
        }
        let d = Diagnostic {
            at: 0x1234,
            kind: DiagKind::DrawBeforeCimg,
        };
        assert_eq!(d.to_string(), "draw before first CIMG @ 0x0000000000001234");
    }

    #[test]
    fn vec_and_nop_sinks_behave() {
        let mut v: Vec<Diagnostic> = Vec::new();
        v.emit(Diagnostic {
            at: 4,
            kind: DiagKind::DlPastRdram,
        });
        assert_eq!(
            v,
            vec![Diagnostic {
                at: 4,
                kind: DiagKind::DlPastRdram
            }]
        );
        NopSink.emit(Diagnostic {
            at: 0,
            kind: DiagKind::DlPastRdram,
        });
        LogSink.emit(Diagnostic {
            at: 0,
            kind: DiagKind::StrayRdphalf,
        });
    }

    #[test]
    fn summary_defaults_are_zeroed() {
        assert_eq!(DlSummary::default().commands, 0);
        assert!(!DlSummary::default().renderable);
    }
}
