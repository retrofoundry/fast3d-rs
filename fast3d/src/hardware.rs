//! Public hardware and memory boundary.
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
pub use crate::hle::host_mem::HostRam;
pub use crate::hle::mem::{
    Command, Matrix, MemoryError, MemoryErrorKind, RawVertex, Rdram, RdramImage,
};

/// Raw VI register words, exactly as the N64 VI presents them; `fast3d` owns the bit-decode
/// (spec §3.3). v1 scanout uses only `origin` (FB-select) and `width`; the rest are carried so
/// the struct is stable as scanout grows overscan-crop / interlace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViRegisters {
    pub status: u32,
    pub origin: u32,
    pub width: u32,
    pub x_scale: u32,
    pub y_scale: u32,
    pub h_start: u32,
    pub v_start: u32,
    pub v_current: u32,
}

/// Consumer-implemented N64-machine boundary (spec §3.2). `rdram` returns an owned reader that
/// borrows the byte source for `'_`; the walk takes it by value and mutates its own segment table.
/// Because `rdram` uses RPITIT (`impl Rdram`), `Hardware` is NOT dyn-compatible, so `process_dl`
/// and `present` are generic methods (`&impl Hardware`), never `&dyn Hardware`.
pub trait Hardware {
    /// The memory reader for the CURRENT walk. Called EXCLUSIVELY inside `process_dl`, on the
    /// calling thread; fully consumed before returning. Each walk owns its reader and segment table.
    fn rdram(&self) -> impl Rdram + '_;

    /// Live VI registers. With `None`, presentation falls back to the last rendered framebuffer.
    fn vi(&self) -> Option<ViRegisters> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdram_image_reports_is_rdram_image_true() {
        let img = RdramImage::new(&[0u8; 8]);
        assert!(
            img.is_rdram_image(),
            "RdramImage must override is_rdram_image -> true"
        );
    }
}

#[cfg(test)]
mod hardware_tests {
    use super::*;

    struct WebHardware {
        rdram: Vec<u8>,
    }
    impl Hardware for WebHardware {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.rdram)
        }
    }

    struct WafelHardware<'a> {
        ram: &'a [u8],
        vi: ViRegisters,
    }
    impl Hardware for WafelHardware<'_> {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(self.ram)
        }
        fn vi(&self) -> Option<ViRegisters> {
            Some(self.vi)
        }
    }

    // Locks the generic-method / RPITIT shape process_dl(&impl Hardware) will use in P3.8.
    fn walk_is_image(hw: &impl Hardware) -> bool {
        hw.rdram().is_rdram_image()
    }

    #[test]
    fn web_hardware_defaults_vi_none_and_image_backend() {
        let hw = WebHardware {
            rdram: vec![0u8; 16],
        };
        assert!(hw.vi().is_none(), "defaulted vi() is None");
        assert!(walk_is_image(&hw), "WebHardware backs onto RdramImage");
    }

    #[test]
    fn wafel_hardware_carries_vi_and_image_backend() {
        // RH: ViRegisters has no PartialEq — compare a representative field.
        let regs = ViRegisters {
            origin: 0x0010_0000,
            width: 320,
            ..Default::default()
        };
        let ram = [0u8; 16];
        let hw = WafelHardware {
            ram: &ram,
            vi: regs,
        };
        let got = hw.vi().expect("vi is Some");
        assert_eq!(got.origin, 0x0010_0000);
        assert_eq!(got.width, 320);
        assert!(walk_is_image(&hw));
    }

    #[test]
    fn vi_registers_default_is_all_zero() {
        let v = ViRegisters::default();
        assert_eq!(
            (
                v.status,
                v.origin,
                v.width,
                v.x_scale,
                v.y_scale,
                v.h_start,
                v.v_start,
                v.v_current
            ),
            (0, 0, 0, 0, 0, 0, 0, 0)
        );
    }
}
