//! Versioned literal vectors for N64 GBI protocol conformance.
//!
//! Conformance entries are independently grounded in the cited SDK/header or hardware reference.
//! Characterization entries describe only local `n64-gbi` compatibility behavior.
//! The table covers protocol-leaf primitives; it is not a claim that every primitive is already
//! consumed by fast3d's temporary textual assembler.
//!
//! Provenance fields are human-audit metadata. Offline tests validate their structure and compare
//! every literal with its primitive, but deliberately do not fetch mutable external documents.

/// Literal data pinned by a conformance or characterization vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Literal {
    /// One 32-bit scalar or packed value.
    U32(u32),
    /// The two words of one GBI command.
    Words(crate::encode::CommandWords),
    /// A static byte sequence.
    Bytes(&'static [u8]),
}

/// Strength of the claim made by a vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VectorStatus {
    /// The literal is independently supported by the cited protocol source.
    Conformance,
    /// The literal records local compatibility behavior and makes no protocol-conformance claim.
    CharacterizationOnly,
}

/// Category of evidence used to derive a vector literal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceKind {
    /// A macro or layout in libultra's `gbi.h`.
    LibultraGbiMacro,
    /// An N64 SDK manual or programming document.
    SdkDocument,
    /// A hardware-oriented reference implementation.
    HardwareReference,
    /// Arithmetic independently expanded by hand from a cited wire layout.
    IndependentHandDerivation,
    /// Local behavior retained for compatibility, not protocol conformance.
    LocalCharacterization,
}

/// Human-auditable evidence for one literal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Provenance {
    /// Evidence category.
    pub kind: ProvenanceKind,
    /// Document or implementation containing the source fact.
    pub source: &'static str,
    /// Stable symbol, section, or function within that source.
    pub locator: &'static str,
    /// Arithmetic or field-order derivation from source fact to literal.
    pub derivation: &'static str,
}

/// One named literal and the evidence supporting it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConformanceVector {
    /// Stable identifier shared by downstream consumers.
    pub id: &'static str,
    /// Whether this is protocol conformance or local characterization.
    pub status: VectorStatus,
    /// Independently selected expected value.
    pub literal: Literal,
    /// Source and derivation for the expected value.
    pub provenance: Provenance,
}

/// First stable version of the shared vector table.
pub mod v1 {
    use super::{ConformanceVector, Literal, Provenance, ProvenanceKind, VectorStatus};

    /// Vector-table schema version.
    pub const VERSION: u16 = 1;

    const GBI_H: &str =
        "https://mountainflaw.github.io/doxygen/d3/daf/gbi_8h_source.html (revision 1.141)";
    const SDK_SEGMENTS: &str =
        "https://ultra64.ca/files/documentation/online-manuals/man/kantan/step1/1-4.html";
    const ANGRYLION_FETCH: &str =
        "https://emudev.org/2021/09/21/Angrylion_RDP_Comments#texture-fetching";

    /// Protocol conformance and explicitly local compatibility literals in version 1.
    pub const TABLE: &[ConformanceVector] = &[
        // Provenance: libultra gbi.h G_IM_FMT_RGBA, line 394; literal macro value.
        ConformanceVector {
            id: "image-format-rgba",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:394, G_IM_FMT_RGBA",
                derivation: "literal macro value 0",
            },
        },
        // Provenance: libultra gbi.h G_IM_FMT_YUV, line 395; literal macro value.
        ConformanceVector {
            id: "image-format-yuv",
            status: VectorStatus::Conformance,
            literal: Literal::U32(1),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:395, G_IM_FMT_YUV",
                derivation: "literal macro value 1",
            },
        },
        // Provenance: libultra gbi.h G_IM_FMT_CI, line 396; literal macro value.
        ConformanceVector {
            id: "image-format-ci",
            status: VectorStatus::Conformance,
            literal: Literal::U32(2),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:396, G_IM_FMT_CI",
                derivation: "literal macro value 2",
            },
        },
        // Provenance: libultra gbi.h G_IM_FMT_IA, line 397; literal macro value.
        ConformanceVector {
            id: "image-format-ia",
            status: VectorStatus::Conformance,
            literal: Literal::U32(3),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:397, G_IM_FMT_IA",
                derivation: "literal macro value 3",
            },
        },
        // Provenance: libultra gbi.h G_IM_FMT_I, line 398; literal macro value.
        ConformanceVector {
            id: "image-format-i",
            status: VectorStatus::Conformance,
            literal: Literal::U32(4),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:398, G_IM_FMT_I",
                derivation: "literal macro value 4",
            },
        },
        // Provenance: libultra gbi.h G_IM_SIZ_4b, line 403; literal macro value.
        ConformanceVector {
            id: "image-size-4",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:403, G_IM_SIZ_4b",
                derivation: "literal macro value 0",
            },
        },
        // Provenance: libultra gbi.h G_IM_SIZ_8b, line 404; literal macro value.
        ConformanceVector {
            id: "image-size-8",
            status: VectorStatus::Conformance,
            literal: Literal::U32(1),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:404, G_IM_SIZ_8b",
                derivation: "literal macro value 1",
            },
        },
        // Provenance: libultra gbi.h G_IM_SIZ_16b, line 405; literal macro value.
        ConformanceVector {
            id: "image-size-16",
            status: VectorStatus::Conformance,
            literal: Literal::U32(2),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:405, G_IM_SIZ_16b",
                derivation: "literal macro value 2",
            },
        },
        // Provenance: libultra gbi.h G_IM_SIZ_32b, line 406; literal macro value.
        ConformanceVector {
            id: "image-size-32",
            status: VectorStatus::Conformance,
            literal: Literal::U32(3),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:406, G_IM_SIZ_32b",
                derivation: "literal macro value 3",
            },
        },
        // Provenance: libultra gbi.h G_TX_RENDERTILE, line 3223; literal macro value.
        ConformanceVector {
            id: "tile-render",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:3223, G_TX_RENDERTILE",
                derivation: "literal macro value 0",
            },
        },
        // Provenance: libultra gbi.h G_TX_LOADTILE, line 3222; literal macro value.
        ConformanceVector {
            id: "tile-load",
            status: VectorStatus::Conformance,
            literal: Literal::U32(7),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:3222, G_TX_LOADTILE",
                derivation: "literal macro value 7",
            },
        },
        // Provenance: libultra gbi.h G_CYC_1CYCLE, lines 579 and 588; selector before shift.
        ConformanceVector {
            id: "cycle-selector-one",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,588, G_MDSFT_CYCLETYPE and G_CYC_1CYCLE",
                derivation: "raw selector is 0 before the documented shift by 20",
            },
        },
        // Provenance: libultra gbi.h G_CYC_2CYCLE, lines 579 and 589; selector before shift.
        ConformanceVector {
            id: "cycle-selector-two",
            status: VectorStatus::Conformance,
            literal: Literal::U32(1),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,589, G_MDSFT_CYCLETYPE and G_CYC_2CYCLE",
                derivation: "raw selector is 1 before the documented shift by 20",
            },
        },
        // Provenance: libultra gbi.h G_CYC_COPY, lines 579 and 590; selector before shift.
        ConformanceVector {
            id: "cycle-selector-copy",
            status: VectorStatus::Conformance,
            literal: Literal::U32(2),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,590, G_MDSFT_CYCLETYPE and G_CYC_COPY",
                derivation: "raw selector is 2 before the documented shift by 20",
            },
        },
        // Provenance: libultra gbi.h G_CYC_FILL, lines 579 and 591; selector before shift.
        ConformanceVector {
            id: "cycle-selector-fill",
            status: VectorStatus::Conformance,
            literal: Literal::U32(3),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,591, G_MDSFT_CYCLETYPE and G_CYC_FILL",
                derivation: "raw selector is 3 before the documented shift by 20",
            },
        },
        // Provenance: libultra gbi.h G_CYC_1CYCLE, lines 579 and 588; 0 << 20.
        ConformanceVector {
            id: "cycle-bits-one",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0000_0000),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,588, G_MDSFT_CYCLETYPE and G_CYC_1CYCLE",
                derivation: "0 << 20 = 0x00000000",
            },
        },
        // Provenance: libultra gbi.h G_CYC_2CYCLE, lines 579 and 589; 1 << 20.
        ConformanceVector {
            id: "cycle-bits-two",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0010_0000),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,589, G_MDSFT_CYCLETYPE and G_CYC_2CYCLE",
                derivation: "1 << 20 = 0x00100000",
            },
        },
        // Provenance: libultra gbi.h G_CYC_COPY, lines 579 and 590; 2 << 20.
        ConformanceVector {
            id: "cycle-bits-copy",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0020_0000),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,590, G_MDSFT_CYCLETYPE and G_CYC_COPY",
                derivation: "2 << 20 = 0x00200000",
            },
        },
        // Provenance: libultra gbi.h G_CYC_FILL, lines 579 and 591; 3 << 20.
        ConformanceVector {
            id: "cycle-bits-fill",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0030_0000),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:579,591, G_MDSFT_CYCLETYPE and G_CYC_FILL",
                derivation: "3 << 20 = 0x00300000",
            },
        },
        // Provenance: libultra gbi.h G_CCMUX_* lines 444-464 and four-bit color-A placement.
        ConformanceVector {
            id: "combiner-color-a",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7, 15]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-464 and GCCc0w0/GCCc1w0 at 3045-3050",
                derivation: "A is four bits; G_CCMUX_0=31 truncates to 15",
            },
        },
        // Provenance: libultra gbi.h G_CCMUX_* lines 444-464 and four-bit color-B placement.
        ConformanceVector {
            id: "combiner-color-b",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7, 15]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-464 and GCCc0w1/GCCc1w1 at 3052-3059",
                derivation: "B is four bits; G_CCMUX_0=31 truncates to 15",
            },
        },
        // Provenance: libultra gbi.h G_CCMUX_* lines 444-464 and five-bit color-C placement.
        ConformanceVector {
            id: "combiner-color-c",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 31]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-464 and GCCc0w0/GCCc1w0 at 3045-3050",
                derivation: "C is five bits and retains G_CCMUX_0=31",
            },
        },
        // Provenance: libultra gbi.h G_CCMUX_* lines 444-464 and three-bit color-D placement.
        ConformanceVector {
            id: "combiner-color-d",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-464 and GCCc0w1/GCCc1w1 at 3052-3059",
                derivation: "D is three bits; G_CCMUX_0=31 truncates to 7",
            },
        },
        // Provenance: libultra gbi.h G_ACMUX_* lines 467-476 and three-bit A/B/D slots.
        ConformanceVector {
            id: "combiner-alpha-abd",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:467-476 and GCCc0w0/GCCc0w1/GCCc1w1 at 3045-3059",
                derivation: "exact three-bit alpha A/B/D selectors",
            },
        },
        // Provenance: libultra gbi.h G_ACMUX_* lines 467-476 and three-bit alpha-C slots.
        ConformanceVector {
            id: "combiner-alpha-c",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:467-476 and GCCc0w0/GCCc1w1 at 3045-3059",
                derivation: "exact three-bit alpha C selectors; zero is LOD fraction",
            },
        },
        // Provenance: libultra gbi.h Vtx_tn, lines 1080-1086; fields serialized big-endian.
        ConformanceVector {
            id: "vtx-normal-layout",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
                0xff, 0x80, 0x7f, 0xdd,
            ]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1080-1086, Vtx_tn",
                derivation: "six big-endian 16-bit fields followed by signed nx/ny/nz and alpha",
            },
        },
        // Provenance: libultra gbi.h Light_t fields, lines 1342-1350.
        ConformanceVector {
            id: "directional-light-layout",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0xff, 0x80, 0x7f, 0x99,
            ]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1342-1350, Light_t",
                derivation: "col[3], pad1, colc[3], pad2, dir[3], pad3 occupy the 12 named Light_t bytes; the aligned Light union tail is deliberately outside this conformance literal",
            },
        },
        // Provenance: libultra gbi.h Ambient_t and Ambient union, lines 1351-1370.
        ConformanceVector {
            id: "ambient-light-layout",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1351-1370, Ambient_t and aligned Ambient union",
                derivation: "col[3], pad1, colc[3], pad2 is exactly eight caller-supplied bytes",
            },
        },
        // Provenance: named-field expansion of libultra gdSPDefLookAt, lines 1477-1479.
        ConformanceVector {
            id: "lookat-gdspdef",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[
                0, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0, 0x81, 0, 0, 0x80, 0, 0, 0, 0x80, 0, 0,
                0, 0x7f, 0x80, 0,
            ]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1477-1479, gdSPDefLookAt",
                derivation: "gdSPDefLookAt(127,0,-127,0,127,-128) fixes the 12 named Light_t bytes for X followed by the 12 named bytes for Y; union tails are deliberately excluded",
            },
        },
        // Provenance: libultra NUML and gsSPNumLights, lines 2442-2463.
        ConformanceVector {
            id: "f3dex2-numlights-2",
            status: VectorStatus::Conformance,
            literal: Literal::Words((0xdb02_0000, 0x0000_0030)),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:2442-2463, NUML and gsSPNumLights; gDma1p at 1720-1734",
                derivation: "G_MOVEWORD=DB, index=02, offset=0; 2*24=0x30",
            },
        },
        // Provenance: libultra gsSPLight plus gDma2p, lines 1736-1748 and 2481-2485.
        ConformanceVector {
            id: "f3dex2-light-1",
            status: VectorStatus::Conformance,
            literal: Literal::Words((0xdc08_060a, 0x0123_4567)),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1736-1748 and 2481-2485, gDma2p and gsSPLight",
                derivation: "((16-1)/8)<<19 | ((1*24+24)/8)<<8 | G_MV_LIGHT",
            },
        },
        // Provenance: libultra gsSPLookAt and look-at offsets, lines 1203-1204 and 2645-2672.
        ConformanceVector {
            id: "f3dex2-lookat",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[
                0xdc, 0x08, 0x00, 0x0a, 0x01, 0x23, 0x45, 0x67, 0xdc, 0x08, 0x03, 0x0a,
                0x01, 0x23, 0x45, 0x77,
            ]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:1203-1204 and 2645-2672, G_MVO_LOOKATX/Y and gsSPLookAt",
                derivation: "X offset 0, Y offset 24; second record address is base+16",
            },
        },
        // Provenance: libultra gsDPLoadTLUTCmd and gsDPLoadTLUT, lines 3356-3368 and 4255-4263.
        ConformanceVector {
            id: "load-tlut-3-entries",
            status: VectorStatus::Conformance,
            literal: Literal::Words((0xf000_0000, 0x0700_8000)),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:3356-3368 and 4255-4263, gsDPLoadTLUTCmd/gsDPLoadTLUT",
                derivation: "tile 7 at bit 24 and (3-1)=2 at bit 14",
            },
        },
        // Provenance: libultra mux values and every gsDPSetCombineLERP slot, lines 444-476 and 3045-3095.
        ConformanceVector {
            id: "combine-slot-canary",
            status: VectorStatus::Conformance,
            literal: Literal::Words((0xfc74_5ccf, 0x672f_6fd4)),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-476 and 3045-3095, mux constants and gsDPSetCombineLERP",
                derivation: "w0=FC000000|7<<20|8<<15|5<<12|6<<9|6<<5|15; w1=6<<28|7<<24|1<<21|3<<18|6<<15|6<<12|7<<9|7<<6|2<<3|4",
            },
        },
        // Provenance: a second independent expansion gives every slot a unique two-vector
        // signature (modulo the narrowest, three-bit slot), catching any slot transposition.
        ConformanceVector {
            id: "combine-slot-canary-b",
            status: VectorStatus::Conformance,
            literal: Literal::Words((0xfc36_469f, 0x01bc_a0b7)),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:444-476 and 3045-3095, mux constants and gsDPSetCombineLERP",
                derivation: "w0=FC000000|3<<20|12<<15|4<<12|3<<9|4<<5|31; w1=0<<28|1<<24|5<<21|7<<18|1<<15|2<<12|0<<9|2<<6|6<<3|7",
            },
        },
        // Provenance: independent byte split of the sourced f3dex2-light-1 Gfx words in N64 byte order.
        ConformanceVector {
            id: "command-be-light-1",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0xdc, 0x08, 0x06, 0x0a, 0x01, 0x23, 0x45, 0x67]),
            provenance: Provenance {
                kind: ProvenanceKind::IndependentHandDerivation,
                source: GBI_H,
                locator: "gbi.h:1669-1698, Gfx is two u32 words; f3dex2-light-1 vector",
                derivation: "split w0 then w1 most-significant byte first",
            },
        },
        // Provenance: libultra DPRGBColor, lines 3123-3130.
        ConformanceVector {
            id: "rgba8-12345678",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x1234_5678),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:3123-3130, DPRGBColor/sDPRGBColor",
                derivation: "0x12<<24 | 0x34<<16 | 0x56<<8 | 0x78",
            },
        },
        // Provenance: N64 Introductory Manual section 1-4 segmented-address bit layout.
        ConformanceVector {
            id: "segmented-6-123456",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0612_3456),
            provenance: Provenance {
                kind: ProvenanceKind::SdkDocument,
                source: SDK_SEGMENTS,
                locator: "N64 Introductory Manual, Step 1, section 1-4",
                derivation: "ignored bits 31:28 are zero, segment ID 6 occupies bits 27:24, and offset 0x123456 occupies bits 23:0",
            },
        },
        // Provenance: libultra GPACK_RGBA5551, lines 291-293, which establishes R/G/B/A bit order.
        ConformanceVector {
            id: "rgba5551-f821",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0xf8, 0x21]),
            provenance: Provenance {
                kind: ProvenanceKind::LibultraGbiMacro,
                source: GBI_H,
                locator: "gbi.h:291-293, GPACK_RGBA5551",
                derivation: "31<<11 | 0<<6 | 16<<1 | 1 = 0xF821",
            },
        },
        // Provenance: Angrylion TEXEL_IA16 fetch reads intensity from c>>8 and alpha from c&0xff.
        ConformanceVector {
            id: "ia16-12ab",
            status: VectorStatus::Conformance,
            literal: Literal::Bytes(&[0x12, 0xab]),
            provenance: Provenance {
                kind: ProvenanceKind::HardwareReference,
                source: ANGRYLION_FETCH,
                locator: "fetch_texel, TEXEL_IA16 case",
                derivation: "intensity occupies the high byte and alpha the low byte",
            },
        },
        // Provenance: Angrylion TEXEL_IA8 fetch reads intensity from p&0xf0 and alpha from p&0x0f.
        ConformanceVector {
            id: "ia8-a3",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0xa3),
            provenance: Provenance {
                kind: ProvenanceKind::HardwareReference,
                source: ANGRYLION_FETCH,
                locator: "fetch_texel, TEXEL_IA8 case",
                derivation: "intensity 0xA is the high nibble and alpha 3 the low nibble",
            },
        },
        // Provenance: Angrylion TEXEL_IA4 fetch masks intensity with 0xe and alpha with 1.
        ConformanceVector {
            id: "ia4-b",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x0b),
            provenance: Provenance {
                kind: ProvenanceKind::HardwareReference,
                source: ANGRYLION_FETCH,
                locator: "fetch_texel, TEXEL_IA4 case",
                derivation: "5<<1 | 1 = 0xB; intensity is bits 3:1 and alpha bit 0",
            },
        },
        // Provenance: Angrylion four-bit fetch selects the high nibble for even s and low for odd s.
        ConformanceVector {
            id: "four-bit-pair-1a",
            status: VectorStatus::Conformance,
            literal: Literal::U32(0x1a),
            provenance: Provenance {
                kind: ProvenanceKind::HardwareReference,
                source: ANGRYLION_FETCH,
                locator: "fetch_texel, TEXEL_I4 case",
                derivation: "even nibble 1 at bits 7:4 and odd nibble A at bits 3:0",
            },
        },
        // Provenance: local n64-gbi pack_4bit_row contract; the unused low nibble is deliberately zero.
        ConformanceVector {
            id: "odd-row-zero-pad",
            status: VectorStatus::CharacterizationOnly,
            literal: Literal::Bytes(&[0x12, 0x30, 0xab, 0xc0]),
            provenance: Provenance {
                kind: ProvenanceKind::LocalCharacterization,
                source: "n64-gbi::texel::pack_4bit_row",
                locator: "documented odd-length-row zero-fill contract",
                derivation: "rows [1,2,3] and [A,B,C] restart high-nibble packing and zero each tail",
            },
        },
        // Provenance: local n64-gbi legacy gdp_load_tlut encoder, retained only as compatibility characterization.
        ConformanceVector {
            id: "legacy-tlut-count-3",
            status: VectorStatus::CharacterizationOnly,
            literal: Literal::Words((0xf000_0000, 0x0700_0008)),
            provenance: Provenance {
                kind: ProvenanceKind::LocalCharacterization,
                source: "n64-gbi::encode::gdp_load_tlut",
                locator: "legacy lrt-in-low-12-bits encoder contract",
                derivation: "tile 7 plus ((3-1)<<2)=8 in the low 12 bits; not gsDPLoadTLUTCmd",
            },
        },
        // Provenance: local n64-gbi legacy combiner regression, explicitly not independently sourced.
        ConformanceVector {
            id: "legacy-combine-golden",
            status: VectorStatus::CharacterizationOnly,
            literal: Literal::Words((0xfc12_7e24, 0xffff_f9fc)),
            provenance: Provenance {
                kind: ProvenanceKind::LocalCharacterization,
                source: "n64-gbi::encode::gdp_set_combine_lerp",
                locator: "encode.rs golden_combine_modulate_unmasked regression",
                derivation: "existing computed-and-round-tripped local golden; not independent conformance",
            },
        },
    ];
}
