use bitflags::bitflags;

bitflags! {
    pub struct OpCode: u8 {
        const NOOP = 0x00;
        // RDP
        const SETOTHERMODE_H = 0xe3;
        const SETOTHERMODE_L = 0xe2;
        const RDPHALF_1 = 0xe1;
        const RDPHALF_2 = 0xf1;

        const SPNOOP = 0xe0;

        // RSP
        const ENDDL = 0xdf;
        const DL = 0xde;
        const LOAD_UCODE = 0xdd;
        const MOVEMEM = 0xdc;
        const MOVEWORD = 0xdb;
        const MTX = 0xda;
        const GEOMETRYMODE = 0xd9;
        const POPMTX = 0xd8;
        const TEXTURE = 0xd7;

        // DMA
        const VTX = 0x01;
        const MODIFYVTX = 0x02;
        const CULLDL = 0x03;
        const BRANCH_Z = 0x04;
        const TRI1 = 0x05;
        const TRI2 = 0x06;
        const QUAD = 0x07;
        const LINE3D = 0x08;
        const DMA_IO = 0xD6;

        const SPECIAL_1 = 0xD5;
    }
}

bitflags! {
    pub struct GeometryModes: u32 {
        const TEXTURE_ENABLE      = 0x00000002;
        const SHADING_SMOOTH      = 0x00000200;
        const CULL_FRONT          = 0x00001000;
        const CULL_BACK           = 0x00002000;
        const CULL_BOTH           = Self::CULL_FRONT.bits | Self::CULL_BACK.bits;
    }
}

bitflags! {
    pub struct MatrixMode: u8 {
        const MODELVIEW = 0x00000000;
        const PROJECTION = 0x00000004;
    }
}

bitflags! {
    pub struct MatrixOperation: u8 {
        const NOPUSH = 0x00000000;
        const PUSH = 0x00000001;
        const MUL = 0x00000000;
        const LOAD = 0x00000002;
    }
}

bitflags! {
    pub struct MoveWordIndex: u8 {
        const FORCEMTX = 0x0C;
    }
}

bitflags! {
    pub struct MoveWordOffset: u8 {
        const A_LIGHT_2 = 0x18;
        const B_LIGHT_2 = 0x1c;
        const A_LIGHT_3 = 0x30;
        const B_LIGHT_3 = 0x34;
        const A_LIGHT_4 = 0x48;
        const B_LIGHT_4 = 0x4c;
        const A_LIGHT_5 = 0x60;
        const B_LIGHT_5 = 0x64;
        const A_LIGHT_6 = 0x78;
        const B_LIGHT_6 = 0x7c;
        const A_LIGHT_7 = 0x90;
        const B_LIGHT_7 = 0x94;
        const A_LIGHT_8 = 0xa8;
        const B_LIGHT_8 = 0xac;
    }
}

bitflags! {
    pub struct MoveMemoryIndex: u8 {
        const MMTX = 2;
        const PMTX = 6;
        const VIEWPORT = 8;
        const LIGHT = 10;
        const POINT = 12;
        const MATRIX = 14;
    }
}

bitflags! {
    pub struct MoveMemoryOffset: u8 {
        const LOOKATX = 0; // (0 * 24);
        const LOOKATY = 24;
        const L0 = 2 * 24;
        const L1 = 3 * 24;
        const L2 = 4 * 24;
        const L3 = 5 * 24;
        const L4 = 6 * 24;
        const L5 = 7 * 24;
        const L6 = 8 * 24;
        const L7 = 9 * 24;
    }
}
