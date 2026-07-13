//! wasm-bindgen entrypoint: canvas binding + asm -> process_dl -> present wiring.
//!
//! The JS-facing surface (`start`, `Renderer` + `init`/`render`) is per-item
//! `#[cfg(target_arch = "wasm32")]`-gated so the pure helpers below (`map_diags`/`should_present`) and
//! their tests compile + run on a NATIVE `cargo test` host. (Pre-migration the whole file was
//! `#![cfg(target_arch = "wasm32")]`, which stripped it to empty under native tests.)
//!
//! `DiagOut`/`RenderOut`/`map_diags`/`should_present` carry `#[cfg_attr(not(target_arch = "wasm32"),
//! allow(dead_code))]`: on the native `--lib` target (cfg(test) OFF — the one `<GATE>` clippy
//! `-D warnings` compiles) they are reachable only from the wasm `render` path (cfg'd out) and the
//! `#[cfg(test)]` module (cfg'd out on the lib target), so without the allow clippy fails
//! `never constructed` / `never used`.

use fast3d::{Diagnostic, DlSummary};
use serde::Serialize;

#[cfg(target_arch = "wasm32")]
use fast3d::{
    ClearPolicy, Hardware, Microcode, Rdram, RdramImage, Renderer as Fast3dRenderer, RendererConfig,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

// `allow(dead_code)` on the native target only (constructed in the wasm `render` path + `#[cfg(test)]`,
// both cfg'd out on the native `--lib` target that `<GATE>` clippy `-D warnings` compiles).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Serialize)]
struct DiagOut {
    line: usize,
    msg: String,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Serialize)]
struct RenderOut {
    diags: Vec<DiagOut>,
    is_time_variant: bool,
    /// Non-fatal runtime error (e.g. transient surface loss); never a bare string into `diags`.
    error: Option<String>,
}

/// Map structured HLE diagnostics to the JS list. `line` carries the command BYTE ADDRESS
/// (`Diagnostic::at`), matching pre-migration behaviour — the JS label still reads "line" (a known,
/// out-of-scope UX follow-up). `msg` is `DiagKind`'s `Display`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn map_diags(diags: &[Diagnostic]) -> Vec<DiagOut> {
    diags
        .iter()
        .map(|d| DiagOut {
            line: d.at as usize,
            msg: d.kind.to_string(),
        })
        .collect()
}

/// New render gate (replaces the old "block on ANY diagnostic"): present iff the walk rasterized
/// something AND emitted zero ERROR-severity diagnostics. WARN-only DLs (previously blocked) now
/// render — a deliberate loosening verified against the toy corpus in P5.3.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn should_present(summary: &DlSummary) -> bool {
    summary.renderable && summary.errors == 0
}

#[cfg(target_arch = "wasm32")]
fn to_js<T: Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap_or(JsValue::NULL)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// The N64-machine boundary for web: an owned RDRAM byte image (the assembled DL). `vi()` defaults to
/// `None` (no live VI) so `present` scans out the last-rendered framebuffer — exactly today's
/// behaviour. Mirrors the `hardware.rs` `WebHardware` spec-by-example.
#[cfg(target_arch = "wasm32")]
struct WebHardware {
    rdram: Vec<u8>,
}
#[cfg(target_arch = "wasm32")]
impl Hardware for WebHardware {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.rdram)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct Renderer {
    inner: Fast3dRenderer,
    hw: WebHardware,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Renderer {
    pub async fn init(canvas: HtmlCanvasElement) -> Result<Renderer, JsValue> {
        let (w, h) = (canvas.width(), canvas.height());
        let inner = Fast3dRenderer::new(
            wgpu::SurfaceTarget::Canvas(canvas),
            w,
            h,
            RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::AutoVsync,
                // `None` = deterministic non-sRGB pick (fixes the caps.formats[0] sRGB washout).
                format: None,
                clear_policy: ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::default(),
            },
        )
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Renderer {
            inner,
            hw: WebHardware { rdram: Vec::new() },
        })
    }

    /// Assemble the source (with uploaded texture bytes), interpret it, and draw to the canvas.
    /// Returns a typed RenderOut {diags, is_time_variant, error} (never a bare JsValue string).
    pub fn render(
        &mut self,
        source: &str,
        time: f32,
        tex_rgba: &[u8],
        tex_w: u32,
        tex_h: u32,
    ) -> JsValue {
        let assembled = match fast3d::asm::assemble_at(source, time, Some((tex_rgba, tex_w, tex_h)))
        {
            Ok(a) => a,
            Err(diags) => {
                return to_js(&RenderOut {
                    diags: diags
                        .into_iter()
                        .map(|d| DiagOut {
                            line: d.line,
                            msg: d.msg,
                        })
                        .collect(),
                    is_time_variant: fast3d::asm::source_is_time_variant(source),
                    error: None,
                });
            }
        };
        let is_time_variant = assembled.is_time_variant;
        // web OWNS WebHardware, so it swaps in the freshly assembled image each frame (no buried
        // field inside Renderer — Hardware is passed by `&hw` per call).
        self.hw.rdram = assembled.image.rdram;

        let mut diags: Vec<Diagnostic> = Vec::new();
        self.inner.begin_frame();
        let summary = self.inner.process_dl(
            &self.hw,
            assembled.image.entry_addr as u64,
            Microcode::F3dex2,
            &mut diags,
        );
        let diag_out = map_diags(&diags);

        if !should_present(&summary) {
            return to_js(&RenderOut {
                diags: diag_out,
                is_time_variant,
                error: None,
            });
        }

        let error = match self.inner.present(&self.hw) {
            Ok(()) => None,
            Err(e) => Some(format!("present: {e:?}")),
        };
        to_js(&RenderOut {
            diags: diag_out,
            is_time_variant,
            error,
        })
    }

    // destroy() DEFERRED (residual #3): the facade documents "web consumers MUST call shutdown()", but
    // on web `Drop`'s `poll` is a no-op AND no JS caller exists yet — adding the export now would be an
    // orphan. It lands with its JS teardown caller (canvas teardown / page-unload) in a follow-up.
}

#[cfg(test)]
mod tests {
    use super::*;
    use fast3d::DiagKind;

    fn summary(renderable: bool, errors: u32, warns: u32) -> DlSummary {
        DlSummary {
            commands: 0,
            tris: 0,
            warns,
            errors,
            dropped_runs: 0,
            renderable,
        }
    }

    #[test]
    fn should_present_gates_on_renderable_and_zero_errors() {
        assert!(
            should_present(&summary(true, 0, 0)),
            "renderable + no errors -> present"
        );
        assert!(
            should_present(&summary(true, 0, 5)),
            "warn-only DLs now render (loosened gate; old web blocked on ANY diag)"
        );
        assert!(
            !should_present(&summary(true, 1, 0)),
            "an error blocks even when renderable"
        );
        assert!(
            !should_present(&summary(false, 0, 0)),
            "nothing rasterized -> nothing to present"
        );
        assert!(
            !should_present(&summary(false, 3, 2)),
            "not renderable (errors present) -> no present"
        );
    }

    #[test]
    fn map_diags_carries_byte_address_as_line_and_kind_display_as_msg() {
        let diags = vec![
            Diagnostic {
                at: 0x20,
                kind: DiagKind::DrawBeforeCimg,
            },
            Diagnostic {
                at: 0x1234,
                kind: DiagKind::UnknownOpcode(0xAB),
            },
        ];
        let out = map_diags(&diags);
        assert_eq!(out.len(), 2);
        assert_eq!(
            (out[0].line, out[0].msg.as_str()),
            (0x20, "draw before first CIMG")
        );
        assert_eq!(
            (out[1].line, out[1].msg.as_str()),
            (0x1234, "unknown opcode 0xAB")
        );
    }
}
