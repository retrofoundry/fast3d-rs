use crate::hle::{
    combiner,
    rdp::{Rdp, TileDescriptor},
    texdec::FormatInfo,
    tile_sampling::TileSampling,
    tmem::Tmem,
};

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub struct DecodeCounts {
    pub requests: u64,
    pub executions: u64,
    pub rejected: u64,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub struct Bank {
    pub bytes: Vec<u8>,
    pub sources: Vec<[u16; 2]>,
    pub blocks: Vec<(u16, u16, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub enum Representation {
    Tile,
    Linear,
    Lookup,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub struct Request {
    pub representation: Representation,
    pub role: String,
    pub tile: TileDescriptor,
    pub tlut: u8,
    pub extent: [u32; 2],
    pub bank: Bank,
    pub reachable_bytes: usize,
    pub read_operations: u64,
    pub palette_bytes: usize,
    pub output: Vec<u8>,
    pub rejection: Option<String>,
}

impl Request {
    pub(crate) fn capture(
        rdp: &Rdp,
        tile: &TileDescriptor,
        tlut: u8,
        role: &str,
        output: Result<&[u8], crate::DiagKind>,
    ) -> Self {
        let representation = representation(rdp, tile, tlut);
        let mut request = Self {
            representation,
            role: role.into(),
            tile: tile.clone(),
            tlut,
            extent: TileSampling::from_tile(tile, tlut).allocation_extent(),
            bank: rdp
                .tmem_bank
                .profile_bank(representation == Representation::Linear),
            reachable_bytes: 0,
            read_operations: 0,
            palette_bytes: 0,
            output: output.as_ref().map(|v| v.to_vec()).unwrap_or_default(),
            rejection: output.err().map(|e| format!("{e:?}")),
        };
        if request.rejection.is_none() {
            let mut reads = [false; 4096];
            let mut read_operations = 0;
            let mut read = |addr| {
                reads[addr] = true;
                read_operations += 1;
            };
            let decoded = match representation {
                Representation::Tile => rdp.tmem_bank.sample_tile_observed(tile, tlut, &mut read),
                Representation::Lookup => rdp
                    .tmem_bank
                    .sampling_lookup_observed(tile, tlut, &mut read),
                Representation::Linear => {
                    let bytes = request.prepare().expect("successful reconstruction");
                    request.reachable_bytes = bytes.len();
                    read_operations += bytes.len() as u64;
                    if tile.fmt == 2 {
                        let mut palette_read = |offset| {
                            reads[2048 + offset] = true;
                            read_operations += 1;
                        };
                        let pixels = if tile.siz == 0 {
                            crate::hle::texdec::decode_ci4_observed(
                                &bytes,
                                tile.width.into(),
                                tile.height.into(),
                                rdp.tmem_bank.palette(),
                                tile.palette,
                                tlut,
                                &mut palette_read,
                            )
                        } else {
                            crate::hle::texdec::decode_ci8_observed(
                                &bytes,
                                tile.width.into(),
                                tile.height.into(),
                                rdp.tmem_bank.palette(),
                                tlut,
                                &mut palette_read,
                            )
                        };
                        assert_eq!(pixels, request.output);
                    }
                    request.decode_prepared(&bytes)
                }
            }
            .expect("diagnostic decode must match successful decode");
            assert_eq!(decoded, request.output, "diagnostic decoder differs");
            request.read_operations = read_operations;
            request.reachable_bytes += reads.iter().filter(|&&read| read).count();
            if tile.fmt == 2 {
                request.palette_bytes = reads[2048..].iter().filter(|&&read| read).count();
            }
        }
        request
    }

    pub fn class(&self) -> String {
        format!(
            "{:?}.{}-{}.tlut{}.{}x{}.input{}",
            self.representation,
            self.tile.fmt,
            self.tile.siz,
            self.tlut,
            self.extent[0],
            self.extent[1],
            self.reachable_bytes
        )
    }

    /// Repeats required format validation and compatibility reconstruction before a lookup.
    pub fn prepare(&self) -> Result<Vec<u8>, crate::DiagKind> {
        let fi = FormatInfo {
            fmt: self.tile.fmt,
            siz: self.tile.siz,
        };
        fi.validate()?;
        if self.rejection.is_some() {
            return Err(crate::DiagKind::TextureBytesUnavailable {
                tmem_addr: self.tile.tmem_addr,
            });
        }
        match self.representation {
            Representation::Linear => Tmem::from_profile(&self.bank).linear_bytes(
                &self.tile,
                fi.tmem_bytes(self.tile.width.into(), self.tile.height.into()),
            ),
            _ => Ok(Vec::new()),
        }
    }

    pub fn decode_prepared(&self, linear: &[u8]) -> Result<Vec<u8>, crate::DiagKind> {
        self.prepare()?;
        match self.representation {
            Representation::Tile => {
                Tmem::from_profile(&self.bank).sample_tile(&self.tile, self.tlut)
            }
            Representation::Lookup => {
                Tmem::from_profile(&self.bank).sampling_lookup(&self.tile, self.tlut)
            }
            Representation::Linear => FormatInfo {
                fmt: self.tile.fmt,
                siz: self.tile.siz,
            }
            .decode(
                linear,
                self.tile.width.into(),
                self.tile.height.into(),
                &self.bank.bytes[2048..],
                self.tile.palette,
                self.tlut,
            ),
        }
    }

    /// Conservative diagnostic identity, with explicit little-endian metadata and normalized linear input.
    pub fn canonical(&self, linear: &[u8]) -> Vec<u8> {
        let mut bytes = b"fast3d-b1-bank-v1\0".to_vec();
        bytes.push(self.representation as u8);
        let t = &self.tile;
        for value in [
            t.uls,
            t.ult,
            t.lrs,
            t.lrt,
            t.width,
            t.height,
            t.line,
            t.tmem_addr,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[
            t.fmt, t.siz, t.palette, t.cms, t.cmt, t.masks, t.maskt, t.shifts, t.shiftt, self.tlut,
        ]);
        bytes.extend_from_slice(&self.bank.bytes);
        bytes.extend_from_slice(linear);
        bytes
    }
}

pub struct DecodeExecutor {
    rdp: Rdp,
    tile: TileDescriptor,
    tlut: u8,
    representation: Representation,
    rejected: bool,
}

impl Request {
    /// Rebuild the owned production request for an untimed trace/identity inspection.
    pub fn encoded_identity(&self) -> Result<(u64, Vec<u8>), crate::DiagKind> {
        self.prepare()?;
        let rdp = Rdp {
            tmem_bank: Tmem::from_profile(&self.bank),
            ..Default::default()
        };
        let binding = crate::hle::texture_request::prepare(
            &rdp,
            &self.tile,
            self.tlut,
            &crate::profiling::Recorder::default(),
        )?;
        let crate::hle::texture_request::TextureSource::Encoded(request) = binding.source else {
            unreachable!()
        };
        Ok((request.key().xxh3_64, request.witness().to_vec()))
    }

    pub fn diagnostic_variant(&self, xor: u8) -> Self {
        let mut bank = self.bank.clone();
        for byte in &mut bank.bytes {
            *byte ^= xor;
        }
        let rdp = Rdp {
            tmem_bank: Tmem::from_profile(&bank),
            ..Default::default()
        };
        let output = combiner::decode_sampling_texture(&rdp, &self.tile, self.tlut)
            .map(|v| v.texture.decode().into_owned());
        Self::capture(
            &rdp,
            &self.tile,
            self.tlut,
            &self.role,
            output.as_ref().map(|v| v.as_slice()).map_err(|e| *e),
        )
    }

    pub fn executor(&self) -> DecodeExecutor {
        DecodeExecutor {
            rdp: Rdp {
                tmem_bank: Tmem::from_profile(&self.bank),
                ..Default::default()
            },
            tile: self.tile.clone(),
            tlut: self.tlut,
            representation: self.representation,
            rejected: self.rejection.is_some(),
        }
    }
}

impl DecodeExecutor {
    pub fn baseline_decode(&self) -> Result<Vec<u8>, crate::DiagKind> {
        if self.rejected {
            return Err(crate::DiagKind::TextureBytesUnavailable {
                tmem_addr: self.tile.tmem_addr,
            });
        }
        if let Some(diag) = self.rdp.tmem_bank.rejection(&self.tile) {
            return Err(diag.kind);
        }
        combiner::decode_sampling_texture(&self.rdp, &self.tile, self.tlut)
            .map(|level| level.texture.decode().into_owned())
    }
    pub fn prepare(&self) -> Result<Vec<u8>, crate::DiagKind> {
        self.validate()?;
        self.reconstruct()
    }
    pub fn validate(&self) -> Result<(), crate::DiagKind> {
        combiner::validate_tile_texture(&self.rdp, &self.tile)?;
        if self.representation != Representation::Lookup {
            combiner::validate_tile_texture(&self.rdp, &self.tile)?;
            let faithful = combiner::tile_takes_faithful_path(
                &self.rdp,
                &self.tile,
                self.tile.width.max(1).into(),
            );
            if faithful != (self.representation == Representation::Tile) {
                return Err(crate::DiagKind::TextureBytesUnavailable {
                    tmem_addr: self.tile.tmem_addr,
                });
            }
        }
        if self.rejected {
            return Err(crate::DiagKind::TextureBytesUnavailable {
                tmem_addr: self.tile.tmem_addr,
            });
        }
        if let Some(diag) = self.rdp.tmem_bank.rejection(&self.tile) {
            return Err(diag.kind);
        }
        Ok(())
    }
    pub fn reconstruct(&self) -> Result<Vec<u8>, crate::DiagKind> {
        if self.representation == Representation::Linear {
            self.rdp.tmem_bank.linear_bytes(
                &self.tile,
                FormatInfo {
                    fmt: self.tile.fmt,
                    siz: self.tile.siz,
                }
                .tmem_bytes(self.tile.width.into(), self.tile.height.into()),
            )
        } else {
            Ok(Vec::new())
        }
    }

    pub fn decode(&self, linear: &[u8]) -> Result<Vec<u8>, crate::DiagKind> {
        self.validate()?;
        match self.representation {
            Representation::Tile => self.rdp.tmem_bank.sample_tile(&self.tile, self.tlut),
            Representation::Lookup => self.rdp.tmem_bank.sampling_lookup(&self.tile, self.tlut),
            Representation::Linear => FormatInfo {
                fmt: self.tile.fmt,
                siz: self.tile.siz,
            }
            .decode(
                linear,
                self.tile.width.into(),
                self.tile.height.into(),
                self.rdp.tmem_bank.palette(),
                self.tile.palette,
                self.tlut,
            ),
        }
    }
}

pub(crate) fn representation(rdp: &Rdp, tile: &TileDescriptor, tlut: u8) -> Representation {
    if TileSampling::from_tile(tile, tlut).image[2] == 1 {
        Representation::Lookup
    } else if combiner::tile_takes_faithful_path(rdp, tile, tile.width.max(1).into()) {
        Representation::Tile
    } else {
        Representation::Linear
    }
}

/// Authored format/extent sweep. These requests are not attributed to a captured workload.
pub fn authored_requests() -> Vec<Request> {
    let mut requests = Vec::new();
    for (fmt, siz) in [
        (0, 2),
        (0, 3),
        (2, 0),
        (2, 1),
        (3, 0),
        (3, 1),
        (3, 2),
        (4, 0),
        (4, 1),
    ] {
        for side in [1u16, 2, 4, 8, 16, 32, 64] {
            for representation in [
                Representation::Tile,
                Representation::Linear,
                Representation::Lookup,
            ] {
                if representation == Representation::Linear && siz == 3 {
                    continue;
                }
                let width = if representation == Representation::Linear {
                    side + 1
                } else {
                    side
                };
                let row = (usize::from(width) << siz).div_ceil(2);
                if row * usize::from(side) > if fmt == 2 { 2048 } else { 4096 } {
                    continue;
                }
                let mut rdp = Rdp::default();
                let tile = TileDescriptor {
                    fmt,
                    siz,
                    width,
                    height: side,
                    line: row.div_ceil(8) as u16,
                    masks: if representation == Representation::Lookup {
                        10
                    } else {
                        0
                    },
                    ..Default::default()
                };
                let data: Vec<u8> = (0..4096).map(|i| (i * 37 + 11) as u8).collect();
                if representation == Representation::Linear {
                    if (usize::from(width) << (siz + 2)).is_multiple_of(64) || side == 1 {
                        continue;
                    }
                    rdp.tmem_bank.write_block(
                        &data,
                        0,
                        0,
                        0x800,
                        (row * usize::from(side)).div_ceil(8),
                        siz,
                    );
                } else {
                    rdp.tmem_bank.write_tile(
                        &data,
                        0,
                        usize::from(tile.line),
                        side.into(),
                        row.div_ceil(8),
                        row.div_ceil(8) * 8,
                        siz,
                    );
                }
                if fmt == 2 {
                    rdp.tmem_bank.write_tlut(&data[..512], 256, 256);
                }
                for tlut in if fmt == 2 { vec![2, 3] } else { vec![0] } {
                    let output = combiner::decode_sampling_texture(&rdp, &tile, tlut)
                        .map(|v| v.texture.decode().into_owned());
                    requests.push(Request::capture(
                        &rdp,
                        &tile,
                        tlut,
                        "authored",
                        output.as_ref().map(|v| v.as_slice()).map_err(|e| *e),
                    ));
                }
            }
        }
    }
    requests
}
