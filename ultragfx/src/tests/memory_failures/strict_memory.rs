pub(crate) struct StrictMemory<'a>(crate::RdramImage<'a>);

impl<'a> StrictMemory<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self(crate::RdramImage::new(bytes))
    }
}

impl crate::Rdram for StrictMemory<'_> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        self.0.capture_layout()
    }
    fn set_segment(&mut self, segment: u32, value: u64) {
        self.0.set_segment(segment, value);
    }
    fn resolve(&self, address: u64) -> Result<u64, crate::MemoryError> {
        self.0.resolve(address)
    }
    fn resolve_masked(&self, address: u64) -> Result<u64, crate::MemoryError> {
        self.0.resolve_masked(address)
    }
    fn read_command(&self, address: u64) -> Result<crate::Command, crate::MemoryError> {
        self.0.read_command(address)
    }
    fn command_stride(&self) -> u64 {
        8
    }
    fn in_bounds(&self, address: u64, length: u64) -> bool {
        self.0.in_bounds(address, length)
    }
    fn read_u8(&self, address: u64) -> Result<u8, crate::MemoryError> {
        Ok(self.0.read_slice(address, 1)?[0])
    }
    fn read_i8(&self, address: u64) -> Result<i8, crate::MemoryError> {
        Ok(self.read_u8(address)? as i8)
    }
    fn read_u16(&self, address: u64) -> Result<u16, crate::MemoryError> {
        Ok(u16::from_be_bytes(
            self.0.read_slice(address, 2)?.try_into().unwrap(),
        ))
    }
    fn read_i16(&self, address: u64) -> Result<i16, crate::MemoryError> {
        Ok(self.read_u16(address)? as i16)
    }
    fn read_bytes(
        &self,
        address: u64,
        length: usize,
    ) -> Result<std::borrow::Cow<'_, [u8]>, crate::MemoryError> {
        Ok(std::borrow::Cow::Borrowed(
            self.0.read_slice(address, length)?,
        ))
    }
    fn read_vertex(
        &self,
        address: u64,
        format: crate::DataFormat,
    ) -> Result<crate::RawVertex, crate::MemoryError> {
        self.0.read_slice(address, 16)?;
        self.0.read_vertex(address, format)
    }
    fn read_matrix(
        &self,
        address: u64,
        format: crate::DataFormat,
    ) -> Result<crate::Matrix, crate::MemoryError> {
        self.0.read_slice(address, 64)?;
        self.0.read_matrix(address, format)
    }
    fn read_obj_bg(
        &self,
        address: u64,
        kind: crate::ObjBgKind,
    ) -> Result<crate::ObjBg, crate::MemoryError> {
        crate::hle::mem::decode_obj_bg(address, kind, 4, |address, length| {
            Ok(self
                .0
                .read_slice(address, length)?
                .iter()
                .fold(0, |v, &b| (v << 8) | u64::from(b)))
        })
    }
    fn is_rdram_image(&self) -> bool {
        true
    }
}
