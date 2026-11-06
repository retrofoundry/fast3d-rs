use super::workload::TargetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TargetExtents {
    pub native: (u32, u32),
    pub logical: (u32, u32),
    pub presentation: (u32, u32),
}

impl TargetExtents {
    pub fn new(
        id: TargetId,
        logical: (u32, u32),
        legacy_canvas: (u32, u32),
        multiplier: u32,
    ) -> Option<Self> {
        let native = match id {
            TargetId::Legacy => legacy_canvas,
            TargetId::Guest(_) => logical,
        };
        let presentation = (
            native.0.checked_mul(multiplier)?,
            native.1.checked_mul(multiplier)?,
        );
        if logical.0 == 0 || logical.1 == 0 || presentation.0 == 0 || presentation.1 == 0 {
            return None;
        }
        Some(Self {
            native,
            logical,
            presentation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_scaling_checks_each_axis_without_changing_native_or_logical() {
        let extents =
            TargetExtents::new(TargetId::Guest(0x1000), (193, 132), (640, 480), 3).unwrap();
        assert_eq!(extents.native, (193, 132));
        assert_eq!(extents.logical, (193, 132));
        assert_eq!(extents.presentation, (579, 396));
    }

    #[test]
    fn legacy_maps_logical_draws_to_the_base_canvas() {
        let extents = TargetExtents::new(TargetId::Legacy, (320, 240), (193, 132), 1).unwrap();
        let scissor = crate::render::workload::output_scissor(
            crate::scene::Scissor {
                ulx: 160,
                uly: 120,
                lrx: 320,
                lry: 240,
                ..Default::default()
            },
            extents.logical,
            extents.presentation,
        );
        assert_eq!(extents.native, (193, 132));
        assert_eq!(
            crate::render::clamp_scissor(&scissor, 193, 132),
            (96, 66, 97, 66)
        );
    }

    #[test]
    fn rejects_empty_extents_and_zero_multiplier() {
        for id in [TargetId::Legacy, TargetId::Guest(0x1000)] {
            for logical in [(0, 240), (320, 0)] {
                assert!(TargetExtents::new(id, logical, (193, 132), 1).is_none());
            }
            assert!(TargetExtents::new(id, (320, 240), (193, 132), 0).is_none());
        }
        for canvas in [(0, 132), (193, 0)] {
            assert!(TargetExtents::new(TargetId::Legacy, (320, 240), canvas, 1).is_none());
        }
    }

    #[test]
    fn checked_scaling_accepts_the_boundary_and_rejects_axis_overflow() {
        let id = TargetId::Guest(0x1000);
        assert_eq!(
            TargetExtents::new(id, (u32::MAX / 3, 1), (1, 1), 3)
                .unwrap()
                .presentation,
            (u32::MAX, 3)
        );
        for native in [(u32::MAX / 3 + 1, 1), (1, u32::MAX / 3 + 1)] {
            assert!(TargetExtents::new(id, native, (1, 1), 3).is_none());
            assert!(TargetExtents::new(TargetId::Legacy, (320, 240), native, 3).is_none());
        }
    }
}
