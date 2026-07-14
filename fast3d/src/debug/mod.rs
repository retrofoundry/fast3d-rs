//! Optional built-in egui debugger (feature `debug-ui`, default-off). Reads the crate-private
//! per-frame `Scene` list directly — impossible for a public render hook — so it ships in-crate. Gated
//! at COMPILE time (this whole module) AND RUNTIME (`Debugger::enabled`, default false), so the
//! default build + headless goldens pay zero. T4.4 wires `DebugInput` + toggles; T4.5 adds egui-wgpu
//! rendering.

/// A windowing-agnostic UI event fed to the built-in debugger via `Renderer::debugger_input`.
/// `fast3d`-owned (NOT routed through `Hardware`, which models N64 memory, not UI events — spec §3.5).
/// `#[non_exhaustive]`: more variants can be added later without an API break.
///
/// NOT `Copy`: the `Text(String)` variant owns a heap `String`. Callers pass `&DebugInput`, so the
/// missing `Copy` costs them nothing; in-crate consumers borrow (see `Debugger::input`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DebugInput {
    /// Pointer moved to a physical-pixel position (top-left origin).
    PointerMoved { x: f32, y: f32 },
    /// A pointer button changed state at the last-known pointer position.
    PointerButton { button: DebugButton, pressed: bool },
    /// Scroll delta in physical pixels.
    Scroll { delta_x: f32, delta_y: f32 },
    /// A key was pressed or released (physical/repeat state is not modelled in v1).
    Key { key: DebugKey, pressed: bool },
    /// Committed text input (post-IME), e.g. one grapheme per keystroke. Owns its `String`.
    Text(String),
}

/// The pointer button in a `DebugInput::PointerButton`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DebugButton {
    Left,
    Right,
    Middle,
}

/// A key in a `DebugInput::Key`. A pragmatic subset of egui 0.35's `egui::Key`: letters, digits, and
/// the common editing/navigation keys. `#[non_exhaustive]` so it can grow (more keys, function keys,
/// modifiers) without an API break. Digit names mirror egui (`Num0`..`Num9`) to keep `to_egui_key` a
/// transparent 1:1 map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DebugKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
}

/// Map a `DebugButton` to egui's button. A PRIVATE module fn — deliberately NOT a
/// `impl From<DebugButton> for egui::PointerButton`: a trait impl carries no visibility modifier and
/// would leak `egui::PointerButton` into fast3d's public surface, which spec §3.5 does not sanction.
/// Consumed only in-crate by `Debugger::input`.
fn to_egui_pointer_button(b: DebugButton) -> egui::PointerButton {
    match b {
        DebugButton::Left => egui::PointerButton::Primary,
        DebugButton::Right => egui::PointerButton::Secondary,
        DebugButton::Middle => egui::PointerButton::Middle,
    }
}

/// Map a `DebugKey` to egui's key. A PRIVATE module fn for the SAME reason as
/// `to_egui_pointer_button` — a public `impl From<DebugKey> for egui::Key` would leak `egui::Key` into
/// fast3d's public surface (spec §3.5). Variant names verified against egui 0.35's `egui::Key`
/// (`egui-0.35.0/src/data/key.rs`: digits are `Num0`..`Num9`, arrows are `Arrow{Up,Down,Left,Right}`).
/// `DebugKey` is our own `#[non_exhaustive]` enum, so this in-crate match needs no wildcard arm.
fn to_egui_key(k: DebugKey) -> egui::Key {
    match k {
        DebugKey::A => egui::Key::A,
        DebugKey::B => egui::Key::B,
        DebugKey::C => egui::Key::C,
        DebugKey::D => egui::Key::D,
        DebugKey::E => egui::Key::E,
        DebugKey::F => egui::Key::F,
        DebugKey::G => egui::Key::G,
        DebugKey::H => egui::Key::H,
        DebugKey::I => egui::Key::I,
        DebugKey::J => egui::Key::J,
        DebugKey::K => egui::Key::K,
        DebugKey::L => egui::Key::L,
        DebugKey::M => egui::Key::M,
        DebugKey::N => egui::Key::N,
        DebugKey::O => egui::Key::O,
        DebugKey::P => egui::Key::P,
        DebugKey::Q => egui::Key::Q,
        DebugKey::R => egui::Key::R,
        DebugKey::S => egui::Key::S,
        DebugKey::T => egui::Key::T,
        DebugKey::U => egui::Key::U,
        DebugKey::V => egui::Key::V,
        DebugKey::W => egui::Key::W,
        DebugKey::X => egui::Key::X,
        DebugKey::Y => egui::Key::Y,
        DebugKey::Z => egui::Key::Z,
        DebugKey::Num0 => egui::Key::Num0,
        DebugKey::Num1 => egui::Key::Num1,
        DebugKey::Num2 => egui::Key::Num2,
        DebugKey::Num3 => egui::Key::Num3,
        DebugKey::Num4 => egui::Key::Num4,
        DebugKey::Num5 => egui::Key::Num5,
        DebugKey::Num6 => egui::Key::Num6,
        DebugKey::Num7 => egui::Key::Num7,
        DebugKey::Num8 => egui::Key::Num8,
        DebugKey::Num9 => egui::Key::Num9,
        DebugKey::Backspace => egui::Key::Backspace,
        DebugKey::Delete => egui::Key::Delete,
        DebugKey::Enter => egui::Key::Enter,
        DebugKey::Tab => egui::Key::Tab,
        DebugKey::Escape => egui::Key::Escape,
        DebugKey::Space => egui::Key::Space,
        DebugKey::ArrowUp => egui::Key::ArrowUp,
        DebugKey::ArrowDown => egui::Key::ArrowDown,
        DebugKey::ArrowLeft => egui::Key::ArrowLeft,
        DebugKey::ArrowRight => egui::Key::ArrowRight,
        DebugKey::Home => egui::Key::Home,
        DebugKey::End => egui::Key::End,
    }
}

/// The built-in egui debugger. Constructed eagerly (cheap — no GPU) and default-DISABLED. The
/// egui-wgpu `Renderer` is built lazily on first `draw` (T4.5) and rebuilt on a surface-format change.
pub struct Debugger {
    pub(crate) enabled: bool,
    ctx: egui::Context,
    raw_input: egui::RawInput,
    pointer_pos: egui::Pos2,
    renderer: Option<egui_wgpu::Renderer>,
    renderer_format: Option<wgpu::TextureFormat>,
    /// Test-only observation: the number of scenes the LAST `draw` was handed. `pub(crate)` so the
    /// in-crate `debugger_present_tests` (a root-level `lib.rs` module) can read it end-to-end to prove
    /// `record_and_submit` forwards the FULL `frame_scenes` list (not last-only). Not otherwise read.
    #[allow(dead_code)]
    pub(crate) last_scene_count: usize,
}

impl Debugger {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            ctx: egui::Context::default(),
            raw_input: egui::RawInput::default(),
            pointer_pos: egui::Pos2::ZERO,
            renderer: None,
            renderer_format: None,
            last_scene_count: 0,
        }
    }

    /// Buffer a UI event until the next `draw`. Returns whether egui wants (would consume) it based on
    /// the PREVIOUS frame's layout (egui decides consumption during `draw`). No-op returning `false`
    /// while disabled.
    pub(crate) fn input(&mut self, input: &DebugInput) -> bool {
        if !self.enabled {
            return false;
        }
        // `match input` (borrow), NOT `match *input`: `DebugInput` is no longer `Copy` (the
        // `Text(String)` variant owns a heap `String`), so dereferencing would try to move out of the
        // `&DebugInput` borrow (E0507). The `Copy`-field arms bind through an `&`-pattern; `Text` binds
        // its `String` by reference and clones it into the egui event.
        match input {
            &DebugInput::PointerMoved { x, y } => {
                self.pointer_pos = egui::pos2(x, y);
                self.raw_input
                    .events
                    .push(egui::Event::PointerMoved(self.pointer_pos));
                self.ctx.egui_wants_pointer_input()
            }
            &DebugInput::PointerButton { button, pressed } => {
                self.raw_input.events.push(egui::Event::PointerButton {
                    pos: self.pointer_pos,
                    button: to_egui_pointer_button(button),
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
                self.ctx.egui_wants_pointer_input()
            }
            &DebugInput::Scroll { delta_x, delta_y } => {
                self.raw_input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(delta_x, delta_y),
                    // egui 0.35 `MouseWheel` REQUIRES `phase` (a synthetic wheel event is TouchPhase::Move).
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::default(),
                });
                self.ctx.egui_wants_pointer_input()
            }
            &DebugInput::Key { key, pressed } => {
                // egui 0.35 `Event::Key` field set (verified against
                // `egui-0.35.0/src/data/input/event.rs`): { key, physical_key, pressed, repeat,
                // modifiers }. We do not model physical keys or key-repeat in v1 → `None`/`false`.
                self.raw_input.events.push(egui::Event::Key {
                    key: to_egui_key(key),
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                });
                self.ctx.egui_wants_keyboard_input()
            }
            DebugInput::Text(text) => {
                // egui 0.35 `Event::Text(String)`. `text` is borrowed from `&DebugInput` → clone into
                // the owned event.
                self.raw_input.events.push(egui::Event::Text(text.clone()));
                self.ctx.egui_wants_keyboard_input()
            }
        }
    }

    /// Paint the inspector over `view` (LoadOp::Load — never erases the game). Shares the caller's
    /// `encoder`; forbids egui user paint-callbacks and `debug_assert!`s `update_buffers` returns an
    /// empty `Vec` (RR). The egui-wgpu `Renderer` is (re)built whenever `format` differs from what it
    /// was built against — covers first-init AND a post-`reconfigure` format change (RQ).
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        scenes: &[crate::scene::Scene],
    ) {
        if self.renderer_format != Some(format) {
            self.renderer = Some(egui_wgpu::Renderer::new(
                device,
                format,
                egui_wgpu::RendererOptions::default(),
            ));
            self.renderer_format = Some(format);
            // Reset the egui Context alongside the rebuilt Renderer. A freshly built egui-wgpu
            // `Renderer` starts with an EMPTY texture map, and egui emits its font-atlas texture delta
            // only ONCE — its `TextureManager` drains the delta via `take_delta`, so a `ctx` that has
            // already run will NEVER re-emit `TextureId::Managed(0)` to the new `Renderer`. Resetting
            // the context gives it a fresh `TextureManager` that re-emits the FULL atlas on the next
            // `run_ui`, which the `textures_delta.set` loop below uploads into the new `Renderer`.
            // Without this, after a format-changing `reconfigure` the overlay renders BLANK (every
            // default mesh samples the missing atlas white pixel → egui-wgpu's "Missing texture" skip),
            // and a later partial glyph delta panics in `update_texture` ("Tried to update a texture
            // that has not been allocated yet."). The only cost is egui's window-position memory
            // (acceptable on a rare format change); `self.raw_input`/`self.pointer_pos` are separate
            // fields, so buffered input survives. Harmless on first-init (`renderer_format == None`),
            // where the context is already fresh.
            self.ctx = egui::Context::default();
        }
        let renderer = self.renderer.as_mut().expect("egui renderer built above");

        let width = view.texture().width();
        let height = view.texture().height();
        let scene_count = scenes.len();
        let total_tris: usize = scenes.iter().map(|s| s.indices.len() / 3).sum();
        self.last_scene_count = scene_count;

        let raw_input = std::mem::take(&mut self.raw_input);
        let full_output = self.ctx.run_ui(raw_input, |ui| {
            egui::Window::new("fast3d debugger")
                .default_pos(egui::pos2(4.0, 4.0))
                .show(ui.ctx(), |ui| {
                    ui.label(format!("scenes this frame: {scene_count}"));
                    ui.label(format!("triangles: {total_tris}"));
                });
        });

        for (id, image_delta) in &full_output.textures_delta.set {
            renderer.update_texture(device, queue, *id, image_delta);
        }
        let paint_jobs = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let pre_frame = renderer.update_buffers(device, queue, encoder, &paint_jobs, &screen);
        // RR: a callback-free UI (Window + labels only) yields an empty Vec — verified against
        // egui-wgpu 0.35.0 (`update_buffers` fills its return only from `Primitive::Callback` jobs). The
        // assert is the release-compiled-out safety net; `record_and_submit` chains nothing from here.
        debug_assert!(
            pre_frame.is_empty(),
            "the built-in debugger must not emit pre-frame command buffers (no egui paint callbacks)"
        );
        let _ = pre_frame; // silence unused in release (debug_assert is compiled out there)
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("debugger"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut pass, &paint_jobs, &screen);
        }
        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_button_maps_to_egui_pointer_button() {
        // Exercises the PRIVATE mapping fn (not a public `From` impl).
        assert_eq!(
            to_egui_pointer_button(DebugButton::Left),
            egui::PointerButton::Primary
        );
        assert_eq!(
            to_egui_pointer_button(DebugButton::Right),
            egui::PointerButton::Secondary
        );
        assert_eq!(
            to_egui_pointer_button(DebugButton::Middle),
            egui::PointerButton::Middle
        );
    }

    #[test]
    fn debug_key_maps_to_egui_key() {
        // Exercises the PRIVATE key-mapping fn (not a public `From` impl). Spot-checks one variant per
        // group (letter / digit / editing / arrow) against the egui 0.35 `Key` names.
        assert_eq!(to_egui_key(DebugKey::A), egui::Key::A);
        assert_eq!(to_egui_key(DebugKey::Num0), egui::Key::Num0);
        assert_eq!(to_egui_key(DebugKey::Enter), egui::Key::Enter);
        assert_eq!(to_egui_key(DebugKey::ArrowLeft), egui::Key::ArrowLeft);
    }

    #[test]
    fn disabled_debugger_ignores_input() {
        let mut dbg = Debugger::new();
        assert!(!dbg.input(&DebugInput::PointerMoved { x: 1.0, y: 1.0 }));
        assert!(
            dbg.raw_input.events.is_empty(),
            "a disabled debugger buffers nothing"
        );
        dbg.enabled = true;
        dbg.input(&DebugInput::PointerMoved { x: 3.0, y: 4.0 });
        assert_eq!(
            dbg.raw_input.events.len(),
            1,
            "an enabled debugger buffers the event"
        );
    }

    #[test]
    fn enabled_debugger_buffers_scroll_as_mouse_wheel() {
        // Guards the `DebugInput::Scroll → egui::Event::MouseWheel` arm (incl. the required `phase`).
        let mut dbg = Debugger::new();
        dbg.enabled = true;
        dbg.input(&DebugInput::Scroll {
            delta_x: 0.0,
            delta_y: -3.0,
        });
        assert_eq!(
            dbg.raw_input.events.len(),
            1,
            "scroll buffers exactly one event"
        );
        assert!(
            matches!(dbg.raw_input.events[0], egui::Event::MouseWheel { .. }),
            "scroll maps to egui MouseWheel: {:?}",
            dbg.raw_input.events[0]
        );
    }

    #[test]
    fn enabled_debugger_buffers_key_and_text() {
        // Guards the `DebugInput::Key → egui::Event::Key` and `DebugInput::Text → egui::Event::Text`
        // arms — keyboard is part of the v1 `DebugInput` surface. Also proves the non-`Copy` `Text`
        // path clones through the `&DebugInput` borrow.
        let mut dbg = Debugger::new();
        dbg.enabled = true;
        dbg.input(&DebugInput::Key {
            key: DebugKey::A,
            pressed: true,
        });
        dbg.input(&DebugInput::Text("A".to_string()));
        assert_eq!(
            dbg.raw_input.events.len(),
            2,
            "key + text buffer two events"
        );
        assert!(
            matches!(
                dbg.raw_input.events[0],
                egui::Event::Key {
                    key: egui::Key::A,
                    pressed: true,
                    repeat: false,
                    ..
                }
            ),
            "key maps to an egui Key press: {:?}",
            dbg.raw_input.events[0]
        );
        assert!(
            matches!(dbg.raw_input.events[1], egui::Event::Text(ref s) if s == "A"),
            "text maps to egui Text: {:?}",
            dbg.raw_input.events[1]
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod render_tests {
    use super::*;
    use crate::render::headless_device;
    use crate::scene::Scene;

    fn target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dbg-target"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    #[test]
    fn debugger_draw_runs_headless_and_counts_all_scenes() {
        let (device, queue, _dual) = headless_device();
        let (_tex, view) = target(&device);
        let mut dbg = Debugger::new();
        dbg.enabled = true;
        let scenes = vec![Scene::default(), Scene::default()]; // TWO scenes in one frame
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        // Drives the full egui-wgpu pipeline headless; the `debug_assert!` inside asserts the empty
        // pre-frame Vec (RR). This fails RED before `Debugger::draw` exists.
        dbg.draw(
            &device,
            &queue,
            &mut enc,
            &view,
            wgpu::TextureFormat::Rgba8Unorm,
            &scenes,
        );
        queue.submit(std::iter::once(enc.finish()));
        assert_eq!(
            dbg.last_scene_count, 2,
            "draw counted ALL scenes it was handed"
        );
    }

    const RB_W: u32 = 256;
    const RB_H: u32 = 256;

    /// One debugger frame at `format`: clear a 256x256 target to an opaque base color, Load-composite
    /// the debugger over it, submit. When `readback`, also copy the target into a mappable buffer and
    /// return its raw bytes (tightly packed, `format`'s native byte order). The teeth test draws
    /// SEVERAL frames per format so egui's window fade-in and deferred glyph rasterization settle
    /// before the final readback.
    fn debugger_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dbg: &mut Debugger,
        format: wgpu::TextureFormat,
        scenes: &[Scene],
        readback: bool,
    ) -> Option<Vec<u8>> {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dbg-fmt-target"),
            size: wgpu::Extent3d {
                width: RB_W,
                height: RB_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        // Clear to an opaque base color; `Debugger::draw` uses LoadOp::Load, so it composites on top.
        {
            let _clear = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dbg-fmt-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.4,
                            b: 0.8,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        dbg.draw(device, queue, &mut enc, &view, format, scenes);

        if !readback {
            queue.submit(std::iter::once(enc.finish()));
            return None;
        }

        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dbg-fmt-rb"),
            size: (RB_W * RB_H * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(RB_W * 4),
                    rows_per_image: Some(RB_H),
                },
            },
            wgpu::Extent3d {
                width: RB_W,
                height: RB_H,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        rx.recv().unwrap().unwrap();
        Some(slice.get_mapped_range().to_vec())
    }

    /// TEETH for the surface-format self-heal (RQ). A freshly rebuilt egui-wgpu `Renderer` starts with
    /// an EMPTY texture map, and egui emits its font-atlas delta only ONCE per `Context`. Drawing the
    /// SAME `Debugger` at format F1 then at a DIFFERENT format F2 rebuilds the `Renderer`; unless the
    /// egui `Context` is ALSO reset (so its fresh `TextureManager` re-emits the FULL atlas), every
    /// default UI mesh samples the missing atlas white pixel and egui-wgpu SKIPS it → the overlay
    /// paints BLANK at F2. We read back the F2 target and assert the window actually painted (a
    /// meaningful number of top-left pixels differ from a known-background corner pixel). This FAILS
    /// (blank at F2) if the `self.ctx = egui::Context::default();` reset is removed, and PASSES with it.
    ///
    /// Several frames are drawn per format because egui fades a window in and rasterizes glyphs lazily
    /// over the first few frames — a single frame after a Context reset paints ~nothing regardless of
    /// the bug, so we let both formats settle and only then read back F2.
    #[test]
    fn debugger_repaints_after_surface_format_change() {
        let (device, queue, _dual) = headless_device();
        let mut dbg = Debugger::new();
        dbg.enabled = true;
        // Identical content across every frame → a stable glyph set, so the buggy F2 failure mode is a
        // clean BLANK (no new-glyph partial delta to trip `update_texture`), which is what we detect.
        let scenes = vec![Scene::default()];

        // F1 (Rgba8Unorm): build the egui-wgpu Renderer and let the window + font atlas fully settle.
        const FRAMES: usize = 8;
        for _ in 0..FRAMES {
            debugger_frame(
                &device,
                &queue,
                &mut dbg,
                wgpu::TextureFormat::Rgba8Unorm,
                &scenes,
                false,
            );
        }
        // F2 (Bgra8Unorm): the first frame changes format → Renderer (and, with the fix, Context)
        // rebuild; the rest let the fresh Context settle. Read back the final F2 frame.
        for _ in 0..FRAMES - 1 {
            debugger_frame(
                &device,
                &queue,
                &mut dbg,
                wgpu::TextureFormat::Bgra8Unorm,
                &scenes,
                false,
            );
        }
        let f2 = debugger_frame(
            &device,
            &queue,
            &mut dbg,
            wgpu::TextureFormat::Bgra8Unorm,
            &scenes,
            true,
        )
        .expect("readback frame returns bytes");

        const W: usize = RB_W as usize;
        const H: usize = RB_H as usize;
        // Bottom-right corner is far from the top-left debugger window → the untouched clear color.
        // Comparing against the ACTUAL cleared bytes keeps the test format-byte-order agnostic.
        let corner = {
            let off = ((H - 1) * W + (W - 1)) * 4;
            [f2[off], f2[off + 1], f2[off + 2], f2[off + 3]]
        };
        let mut painted = 0usize;
        for y in 2..64 {
            for x in 2..140 {
                let off = (y * W + x) * 4;
                if [f2[off], f2[off + 1], f2[off + 2], f2[off + 3]] != corner {
                    painted += 1;
                }
            }
        }
        assert!(
            painted > 50,
            "the debugger window must PAINT at F2 after a format-change Renderer rebuild (a fresh \
             egui-wgpu Renderer needs the FULL font atlas re-emitted); only {painted} pixels differ \
             from the background corner {corner:?} — blank ⇒ the egui Context was not reset"
        );
    }
}
