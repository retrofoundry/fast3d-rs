use crate::capture::Fixture;

pub(super) fn fixtures() -> Vec<(&'static str, Fixture)> {
    use super::sm64_surface_fixtures::Surface;
    vec![
        ("mario-metal-butt", super::texgen::metal_butt_fixture()),
        ("jrb-mixed-fog", super::fog::jrb_fixture()),
        ("sm64-hud-us-copy", super::texrect::hud_fixture(false)),
        ("sm64-hud-eu-point", super::texrect::hud_fixture(true)),
        (
            "sm64-transparent-mario",
            super::alpha_dither_fixture::fixture(),
        ),
        (
            "sm64-power-meter-point",
            super::filter_fixtures::power_meter_fixture(),
        ),
        (
            "sm64-castle-trilerp",
            super::filter_fixtures::castle_fixture(),
        ),
        ("sm64-shadow-decal", Surface::Shadow.fixture()),
        ("sm64-water-translucency", Surface::Water.fixture()),
        ("sm64-cutout-foliage", Surface::Foliage.fixture()),
    ]
}

#[test]
fn sm64_corpus_roundtrips_without_diagnostics() {
    for (name, fixture) in fixtures() {
        let decoded = Fixture::from_bytes(&fixture.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded, fixture, "{name}");
        assert!(!fixture.frame.dual_source_blending, "{name}");
        for task in &decoded.tasks {
            assert_eq!(task.final_color_image().unwrap().width, 320, "{name}");
        }
    }
}

#[test]
fn sm64_corpus_public_facade() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("sm64 corpus requires a GPU adapter");
        let info = adapter.get_info();
        eprintln!("sm64 corpus adapter: {:?} {}", info.backend, info.name);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sm64-corpus-fallback"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .unwrap();
        for (name, fixture) in fixtures() {
            let internal = fixture.replay(device.clone(), queue.clone()).await.unwrap();
            let decoded = Fixture::from_bytes(&fixture.to_bytes().unwrap()).unwrap();
            let public = decoded.replay_headless().await.unwrap();
            let public_info = public
                .adapter_info
                .as_ref()
                .expect("public replay must report its adapter");
            assert_eq!(
                public_info, &info,
                "{name}: compare on the same device class"
            );
            assert_eq!(
                (public.width, public.height),
                (internal.width, internal.height),
                "{name}"
            );
            assert_eq!(public.rgba8, internal.rgba8, "{name}");
            assert_eq!(public.summaries, internal.summaries, "{name}");
            assert_eq!(public.diagnostics, internal.diagnostics, "{name}");
            assert!(
                public
                    .summaries
                    .iter()
                    .all(|s| s.renderable && s.errors == 0),
                "{name}"
            );
            assert!(
                public.diagnostics.iter().all(Vec::is_empty),
                "{name}: {:?}",
                public.diagnostics
            );
        }
    });
}
