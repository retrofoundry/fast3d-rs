use std::borrow::Cow;
use ultragfx::{
    Command, DataFormat, Matrix, MemoryError, MemoryErrorKind, RawVertex, Rdram, RdramImage,
};

struct ExternalRam<'a> {
    image: RdramImage<'a>,
}

impl Rdram for ExternalRam<'_> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<ultragfx::capture::SourceLayout> {
        self.image.capture_layout()
    }

    fn set_segment(&mut self, segment: u32, value: u64) {
        Rdram::set_segment(&mut self.image, segment, value);
    }

    fn resolve(&self, address: u64) -> Result<u64, MemoryError> {
        Rdram::resolve(&self.image, address)
    }

    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError> {
        Rdram::resolve_masked(&self.image, address)
    }

    fn read_command(&self, address: u64) -> Result<Command, MemoryError> {
        Rdram::read_command(&self.image, address)
    }

    fn command_stride(&self) -> u64 {
        Rdram::command_stride(&self.image)
    }

    fn in_bounds(&self, address: u64, length: u64) -> bool {
        Rdram::in_bounds(&self.image, address, length)
    }

    fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        Rdram::read_u8(&self.image, address)
    }

    fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        Rdram::read_i8(&self.image, address)
    }

    fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        Rdram::read_i16(&self.image, address)
    }

    fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        Rdram::read_u16(&self.image, address)
    }

    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError> {
        Rdram::read_bytes(&self.image, address, length)
    }

    fn read_matrix(&self, address: u64, format: DataFormat) -> Result<Matrix, MemoryError> {
        Rdram::read_matrix(&self.image, address, format)
    }

    fn vertex_stride(&self, format: DataFormat) -> Result<u64, MemoryError> {
        Rdram::vertex_stride(&self.image, format)
    }

    fn read_vertex(&self, address: u64, format: DataFormat) -> Result<RawVertex, MemoryError> {
        Rdram::read_vertex(&self.image, address, format)
    }

    fn is_rdram_image(&self) -> bool {
        Rdram::is_rdram_image(&self.image)
    }
}

fn error(address: u64, length: u64, kind: MemoryErrorKind) -> MemoryError {
    MemoryError {
        address,
        length,
        kind,
    }
}

#[test]
fn external_rdram_uses_only_public_types() {
    let bytes = [0; 8];
    let mut memory = ExternalRam {
        image: RdramImage::new(&bytes),
    };

    memory.set_segment(3, 0x40);
    assert_eq!(memory.resolve(0x0300_0004), Ok(0x44));
    assert_eq!(memory.command_stride(), 8);
    assert!(memory.is_rdram_image());
    #[cfg(feature = "capture")]
    assert!(memory.capture_layout().is_some());
}

#[test]
fn image_decodes_complete_public_values() {
    let mut bytes = [0u8; 96];
    bytes[..8].copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
    bytes[8..24].copy_from_slice(&[
        0xff, 0xfe, 0x00, 0x03, 0x00, 0x04, 0, 0, 0xff, 0xfb, 0x00, 0x06, 7, 8, 9, 10,
    ]);
    bytes[32..34].copy_from_slice(&1i16.to_be_bytes());
    bytes[64..66].copy_from_slice(&0x8000u16.to_be_bytes());
    let memory = RdramImage::new(&bytes);

    let command = memory.read_command(0).unwrap();
    assert_eq!(
        command,
        Command {
            w0: 0x1234_5678,
            w1: 0x9abc_def0,
            w1_addr: 0x9abc_def0,
        }
    );
    assert_eq!(memory.read_i8(0).unwrap(), 0x12);
    assert_eq!(memory.read_u8(1).unwrap(), 0x34);
    assert_eq!(memory.read_i16(8).unwrap(), -2);
    assert_eq!(memory.read_u16(10).unwrap(), 3);
    assert_eq!(
        &*memory.read_bytes(2, 4).unwrap(),
        &[0x56, 0x78, 0x9a, 0xbc]
    );

    let mut matrix: Matrix = [[0.0; 4]; 4];
    matrix[0][0] = 1.5;
    assert_eq!(memory.read_matrix(32, DataFormat::Fixed).unwrap(), matrix);
    assert_eq!(memory.vertex_stride(DataFormat::Fixed), Ok(16));
    assert_eq!(
        memory.read_vertex(8, DataFormat::Fixed),
        Ok(RawVertex {
            pos: [-2.0, 3.0, 4.0],
            st: [-5, 6],
            rgba: [7, 8, 9, 10],
        })
    );
}

#[test]
fn image_data_reads_zero_fill_but_commands_remain_strict() {
    let bytes = [0xab; 16];
    let memory = RdramImage::new(&bytes);
    assert_eq!(
        memory.read_command(12),
        Err(error(12, 8, MemoryErrorKind::OutOfBounds))
    );
    assert_eq!(memory.read_u16(15), Ok(0xab00));
    assert_eq!(&*memory.read_bytes(14, 3).unwrap(), &[0xab, 0xab, 0]);
    assert_eq!(&*memory.read_bytes(100, 3).unwrap(), &[0; 3]);
    assert_eq!(memory.take_zero_filled_read(), Some((15, 2)));
    assert_eq!(memory.take_zero_filled_read(), None);
    let vertex = memory.read_vertex(8, DataFormat::Fixed).unwrap();
    assert_eq!(vertex.pos, [-21589.0; 3]);
    assert_eq!(vertex.st, [0; 2]);
    assert_eq!(vertex.rgba, [0; 4]);
    assert_eq!(memory.read_matrix(16, DataFormat::Fixed), Ok([[0.0; 4]; 4]));
    assert!(!memory.in_bounds(u64::MAX, 1));
}

#[test]
fn image_data_reads_mask_each_physical_byte_before_zero_filling() {
    let bytes = [0x12, 0x34, 0x56, 0x78];
    let memory = RdramImage::new(&bytes);
    assert_eq!(memory.read_u16(0x8000_0001), Ok(0x3456));
    assert_eq!(memory.take_zero_filled_read(), None);
    assert_eq!(
        &*memory.read_bytes(0x8000_0002, 4).unwrap(),
        &[0x56, 0x78, 0, 0]
    );
    assert_eq!(memory.take_zero_filled_read(), Some((2, 4)));
    assert_eq!(
        &*memory.read_bytes(0x80ff_fffe, 6).unwrap(),
        &[0, 0, 0x12, 0x34, 0x56, 0x78]
    );
    assert_eq!(memory.take_zero_filled_read(), Some((0x00ff_fffe, 6)));
    assert_eq!(
        memory.read_slice(0x8000_0000, 4),
        Err(error(0x8000_0000, 4, MemoryErrorKind::OutOfBounds))
    );
    assert_eq!(
        memory.read_command(0x8000_0000),
        Err(error(0x8000_0000, 8, MemoryErrorKind::OutOfBounds))
    );
}

#[test]
fn image_rejects_overflow_and_unsupported_float_layouts() {
    let mut memory = RdramImage::new(&[]);

    assert_eq!(
        memory.read_bytes(u64::MAX, 2),
        Err(error(u64::MAX, 2, MemoryErrorKind::AddressOverflow))
    );
    assert_eq!(
        memory.read_u8(u32::MAX as u64 + 1),
        Err(error(
            u32::MAX as u64 + 1,
            1,
            MemoryErrorKind::AddressOverflow
        ))
    );
    assert_eq!(
        memory.resolve(u32::MAX as u64 + 1),
        Err(error(
            u32::MAX as u64 + 1,
            0,
            MemoryErrorKind::AddressOverflow,
        ))
    );
    assert_eq!(
        memory.vertex_stride(DataFormat::Float),
        Err(error(0, 0, MemoryErrorKind::UnsupportedFormat))
    );
    assert_eq!(
        memory.read_matrix(7, DataFormat::Float),
        Err(error(7, 64, MemoryErrorKind::UnsupportedFormat))
    );
    assert_eq!(
        memory.read_vertex(9, DataFormat::Float),
        Err(error(9, 24, MemoryErrorKind::UnsupportedFormat))
    );

    memory.set_segment(2, u32::MAX as u64);
    assert_eq!(memory.resolve(0x0200_0002), Ok(1));
    assert_eq!(memory.resolve_masked(0x0200_000a), Ok(0x0000_0008));
}

struct LittleEndian<'a>(RdramImage<'a>);

impl Rdram for LittleEndian<'_> {
    fn set_segment(&mut self, segment: u32, value: u64) {
        self.0.set_segment(segment, value);
    }
    fn resolve(&self, address: u64) -> Result<u64, MemoryError> {
        self.0.resolve(address)
    }
    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError> {
        self.0.resolve_masked(address)
    }
    fn read_command(&self, address: u64) -> Result<Command, MemoryError> {
        self.0.read_command(address)
    }
    fn command_stride(&self) -> u64 {
        8
    }
    fn in_bounds(&self, address: u64, length: u64) -> bool {
        self.0.in_bounds(address, length)
    }
    fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        self.0.read_u8(address)
    }
    fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        self.0.read_i8(address)
    }
    fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        Ok(self.read_u16(address)? as i16)
    }
    fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        let bytes = self.read_bytes(address, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError> {
        self.0.read_bytes(address, length)
    }
    fn read_matrix(&self, address: u64, _: DataFormat) -> Result<Matrix, MemoryError> {
        Err(error(address, 64, MemoryErrorKind::UnsupportedFormat))
    }
}

#[test]
fn default_vertex_uses_the_backends_decoded_scalars() {
    let bytes = [
        0x34, 0x12, 0xfe, 0xff, 0, 1, 0, 0, 0x20, 0, 0xc0, 0xff, 10, 20, 30, 255,
    ];
    let memory = LittleEndian(RdramImage::new(&bytes));
    assert_eq!(
        memory.read_vertex(0, DataFormat::Fixed).unwrap(),
        RawVertex {
            pos: [4660., -2., 256.],
            st: [32, -64],
            rgba: [10, 20, 30, 255],
        }
    );
}

#[test]
fn external_rdram_without_bg_support_still_compiles() {
    let memory = ExternalRam {
        image: RdramImage::new(&[]),
    };
    assert_eq!(
        memory.read_obj_bg(12, ultragfx::ObjBgKind::Scaled),
        Err(error(12, 0, MemoryErrorKind::UnsupportedFormat))
    );
}

#[test]
fn obj_bg_does_not_read_padding() {
    let mut bytes = [0; 40];
    bytes[32..36].copy_from_slice(&(-0x123456i32).to_be_bytes());
    let scaled = RdramImage::new(&bytes[..36])
        .read_obj_bg(0, ultragfx::ObjBgKind::Scaled)
        .unwrap();
    assert_eq!(scaled.image_y_orig, -0x123456);
    assert!(RdramImage::new(&bytes)
        .read_obj_bg(0, ultragfx::ObjBgKind::Copy)
        .is_ok());
}

#[test]
fn obj_bg_missing_fields_read_as_zero() {
    for (end, kind, address, length) in [
        (19, ultragfx::ObjBgKind::Scaled, 16, 4),
        (35, ultragfx::ObjBgKind::Scaled, 32, 4),
        (39, ultragfx::ObjBgKind::Copy, 38, 2),
    ] {
        let memory = RdramImage::new(&[0; 40][..end]);
        assert_eq!(
            memory.read_obj_bg(0, kind).unwrap(),
            ultragfx::ObjBg::default()
        );
        assert_eq!(memory.take_zero_filled_read(), Some((address, length)));
    }
}
