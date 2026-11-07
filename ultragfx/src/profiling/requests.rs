use crate::hle::{
    rdp::{Rdp, TileDescriptor},
    texture_request::{
        EncodedInput, EncodedTextureRequest, Representation as EncodedRepresentation,
    },
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub enum Representation {
    Tile,
    Lookup,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub struct Request {
    pub version: u32,
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

pub const TRACE_VERSION: u32 = 3;

impl Request {
    pub(crate) fn capture(
        rdp: &Rdp,
        tile: &TileDescriptor,
        tlut: u8,
        role: &str,
        encoded: Result<&EncodedTextureRequest, crate::DiagKind>,
    ) -> Self {
        let representation = encoded.as_ref().map_or_else(
            |_| representation(tile, tlut),
            |request| match request.recipe().representation {
                EncodedRepresentation::Tile => Representation::Tile,
                EncodedRepresentation::Lookup4096x4 => Representation::Lookup,
            },
        );
        let bank = match encoded {
            Ok(request) => {
                let EncodedInput::Tmem(bytes) = request.input();
                Bank {
                    bytes: bytes.to_vec(),
                }
            }
            Err(_) => rdp.tmem_bank.profile_bank(),
        };
        Self {
            version: TRACE_VERSION,
            representation,
            role: role.into(),
            tile: TileDescriptor {
                width: tile.width.max(1),
                height: tile.height.max(1),
                ..tile.clone()
            },
            tlut,
            extent: encoded.as_ref().map_or_else(
                |_| TileSampling::from_tile(tile, tlut).allocation_extent(),
                |request| request.recipe().output,
            ),
            bank,
            reachable_bytes: 0,
            read_operations: 0,
            palette_bytes: 0,
            output: Vec::new(),
            rejection: encoded.err().map(|e| format!("{e:?}")),
        }
    }

    /// Offline CPU analysis for developer tools. Live trace capture leaves these fields empty.
    pub fn analyze(&mut self) -> Result<(), &'static str> {
        self.validate()?;
        let mut reads = [false; 4096];
        let mut read_operations = 0;
        let output = self.encoded_request()?.decode_observed(&mut |addr| {
            reads[addr] = true;
            read_operations += 1;
        });
        self.output = output;
        self.read_operations = read_operations;
        self.reachable_bytes = reads.iter().filter(|&&read| read).count();
        self.palette_bytes = if self.tlut >= 2 {
            reads[2048..].iter().filter(|&&read| read).count()
        } else {
            0
        };
        Ok(())
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

    pub fn validate(&self) -> Result<(), &'static str> {
        let request = self;
        let tile = &self.tile;
        let fi = crate::hle::texdec::FormatInfo {
            fmt: tile.fmt,
            siz: tile.siz,
        };
        fi.validate_tlut(request.tlut)
            .map_err(|_| "unsupported texture format")?;
        let unavailable = "invalid diagnostic texture request";
        if request.version != crate::profiling::TRACE_VERSION
            || request.rejection.is_some()
            || request
                .extent
                .iter()
                .any(|&extent| extent == 0 || extent > 4096)
            || tile.width == 0
            || tile.width > 4096
            || tile.height == 0
            || tile.height > 4096
            || tile.tmem_addr > 511
            || tile.line > 511
            || tile.palette > 15
            || request.tlut > 3
            || tile.masks > 15
            || tile.maskt > 15
        {
            return Err(unavailable);
        }
        let _: &[u8; 4096] = request
            .bank
            .bytes
            .as_slice()
            .try_into()
            .map_err(|_| unavailable)?;
        if (self.representation == Representation::Lookup && self.extent != [4096, 4])
            || (self.representation == Representation::Tile
                && self.extent != [u32::from(tile.width), u32::from(tile.height)])
        {
            return Err(unavailable);
        }
        Ok(())
    }

    pub fn decode(&self) -> Result<Vec<u8>, &'static str> {
        self.validate()?;
        Ok(self.encoded_request()?.decode())
    }

    /// Conservative diagnostic identity, with explicit little-endian metadata and bank bytes.
    pub fn canonical(&self) -> Vec<u8> {
        let mut bytes = b"fast3d-b1-bank-v3\0".to_vec();
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
        bytes
    }
}

impl Request {
    pub(crate) fn encoded_request(&self) -> Result<EncodedTextureRequest, &'static str> {
        EncodedTextureRequest::from_diagnostic(self)
    }

    /// Compute the production identity from an owned diagnostic snapshot without expanding pixels.
    pub fn encoded_identity(&self) -> Result<(u64, Vec<u8>), &'static str> {
        let request = self.encoded_request()?;
        let (key, witness) = request.identity_profiled(&Default::default());
        Ok((key.xxh3_64, witness.to_vec()))
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
        let binding =
            crate::hle::texture_request::prepare(&rdp, &self.tile, self.tlut, &Default::default());
        let encoded = binding.as_ref().map(|binding| {
            let crate::hle::texture_request::TextureSource::Encoded(request) = &binding.source
            else {
                unreachable!()
            };
            request.as_ref()
        });
        let mut request = Self::capture(
            &rdp,
            &self.tile,
            self.tlut,
            &self.role,
            encoded.map_err(|error| *error),
        );
        if request.rejection.is_none() {
            request.analyze().expect("validated diagnostic input");
        }
        request
    }
}

pub(crate) fn representation(tile: &TileDescriptor, tlut: u8) -> Representation {
    if TileSampling::from_tile(tile, tlut).image[2] == 1 {
        Representation::Lookup
    } else {
        Representation::Tile
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
            for representation in [Representation::Tile, Representation::Lookup] {
                let width = side;
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
                rdp.tmem_bank.write_tile(
                    &data,
                    0,
                    usize::from(tile.line),
                    side.into(),
                    row.div_ceil(8),
                    row.div_ceil(8) * 8,
                    siz,
                );
                if fmt == 2 {
                    rdp.tmem_bank.write_tlut(&data[..512], 256, 256);
                }
                for tlut in if fmt == 2 { vec![2, 3] } else { vec![0] } {
                    let binding = crate::hle::texture_request::prepare(
                        &rdp,
                        &tile,
                        tlut,
                        &Default::default(),
                    );
                    let encoded = binding.as_ref().map(|binding| {
                        let crate::hle::texture_request::TextureSource::Encoded(request) =
                            &binding.source
                        else {
                            unreachable!()
                        };
                        request.as_ref()
                    });
                    let mut request = Request::capture(
                        &rdp,
                        &tile,
                        tlut,
                        "authored",
                        encoded.map_err(|error| *error),
                    );
                    if request.rejection.is_none() {
                        request.analyze().expect("validated authored input");
                    }
                    requests.push(request);
                }
            }
        }
    }
    requests
}
