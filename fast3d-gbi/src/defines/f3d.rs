use bitflags::bitflags;

bitflags! {
    pub struct OpCode: u8 {
        const NOOP = 0xc0;
        const SETOTHERMODE_H = 0xBA;
        const SETOTHERMODE_L = 0xB9;
        const RDPHALF_1 = 0xB4;
        const RDPHALF_2 = 0xB3;
        const SPNOOP = 0x00;
        const ENDDL = 0xB8;
        const DL = 0x06;
        const MOVEMEM = 0x03;
        const MOVEWORD = 0xBC;
        const MTX = 0x01;
        const POPMTX = 0xBD;
        const TEXTURE = 0xBB;
        const VTX = 0x04;
        const CULLDL = 0xBE;
        const TRI1 = 0xBF;
        const QUAD = 0xB5;
        const SPRITE2D_BASE = 0x09;
        const SETGEOMETRYMODE = 0xB7;
        const CLEARGEOMETRYMODE = 0xB6;
    }
}
