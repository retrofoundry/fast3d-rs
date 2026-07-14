#![cfg(feature = "asm")]
use fast3d::asm::parser::*;

#[test]
fn parses_minimal_dl_with_matrix_symbols() {
    let src = "\
Mtx p = scale(0.015625)
Mtx m = identity()
Vp { 480, 640, 511, 511, 480, 640, 0, 511 }
Vtx { -48, -48, 0, 0, 0, 0, 255, 0, 0, 255 }
gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_PUSH)
gsSPViewport(vp)
gsSPClearGeometryMode(G_LIGHTING, G_CULL_BACK)
gsSPSetGeometryMode(G_SHADE, G_SHADING_SMOOTH)
gsSPVertex(verts, 1, 0)
gsSP1Triangle(0, 0, 0, 0)
gsSPEndDisplayList()
";
    let (parsed, diags) = parse(src);
    let stmts: Vec<Stmt> = parsed.into_iter().map(|(_line, s)| s).collect();
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert_eq!(
        stmts[0],
        Stmt::Mtx(MtxDef {
            name: "p".into(),
            init: MtxInit::Scale(0.015625)
        })
    );
    assert_eq!(
        stmts[1],
        Stmt::Mtx(MtxDef {
            name: "m".into(),
            init: MtxInit::Identity
        })
    );
    assert_eq!(
        stmts[2],
        Stmt::Viewport(VpDef {
            vscale: [480, 640, 511, 511],
            vtrans: [480, 640, 0, 511]
        })
    );
    assert!(matches!(stmts[3], Stmt::Vtx(_)));
    assert_eq!(
        stmts[4],
        Stmt::SpMatrix {
            name: "p".into(),
            flags: MtxFlags {
                proj: true,
                load: true,
                push: false
            }
        }
    );
    assert_eq!(
        stmts[5],
        Stmt::SpMatrix {
            name: "m".into(),
            flags: MtxFlags {
                proj: false,
                load: true,
                push: true
            }
        }
    );
    assert_eq!(stmts[6], Stmt::SpViewport);
    assert_eq!(
        stmts[7],
        Stmt::SpClearGeometryMode(0x0002_0000 | 0x0000_0400)
    );
    assert_eq!(stmts[8], Stmt::SpSetGeometryMode(0x0000_0004 | 0x0020_0000));
    assert_eq!(stmts[9], Stmt::SpVertex { n: 1, v0: 0 });
    assert_eq!(
        stmts[10],
        Stmt::Sp1Triangle {
            v0: 0,
            v1: 0,
            v2: 0
        }
    );
    assert_eq!(stmts[11], Stmt::SpEndDisplayList);
}

#[test]
fn reports_bad_line() {
    let (_stmts, diags) = parse("gsSPFlibbertigibbet(1,2)\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].line, 1);
    assert!(diags[0].msg.contains("unrecognized"));
}
