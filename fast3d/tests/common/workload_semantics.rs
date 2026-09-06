const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255; 4];

fn in_rect(x: u32, y: u32, [left, top, right, bottom]: [u32; 4]) -> bool {
    (left..right).contains(&x) && (top..bottom).contains(&y)
}

pub fn expected(x: u32, y: u32) -> [u8; 4] {
    if in_rect(x, y, [72, 72, 88, 88]) {
        WHITE
    } else if in_rect(x, y, [64, 64, 96, 96]) {
        [255, 0, 0, 0]
    } else if in_rect(x, y, [192, 144, 256, 176]) {
        CYAN
    } else if in_rect(x, y, [144, 128, 240, 192]) {
        MAGENTA
    } else if in_rect(x, y, [144, 80, 272, 192]) {
        YELLOW
    } else if in_rect(x, y, [112, 64, 224, 112]) {
        GREEN
    } else if in_rect(x, y, [48, 48, 160, 160]) {
        RED
    } else if in_rect(x, y, [32, 32, 288, 208]) {
        BLUE
    } else {
        BLACK
    }
}
