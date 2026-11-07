Raw big-endian RGBA16 words captured from an NTSC N64 on 2026-11-06.
Each threshold file is 128x4 pixels.
Every four columns use one constant primitive RGB value: 0..15, then 240..255.
Alpha is 255, CVG_DST_FULL, alpha dither disabled. Each block covers the full
4x4 RGB dither cell. Files pin magic (selector 0), Bayer (1) and disable (3).
Noise (2) deliberately uses the magic fixture; the hardware RNG is unknown.


`last-writer.rgba16` is a 4x4 (19,85,141) source with magic dither, column 1
then overwritten with dither disabled, followed by a Bayer SetOtherMode.
The first word is `0x1ae5`; column 1 is `0x12a3`. A second capture
repeated all three threshold fixtures with identical CRCs.
