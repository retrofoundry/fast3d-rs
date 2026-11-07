pub fn expected(x: u32, y: u32) -> [u8; 4] {
    if (32..208).contains(&y) && (32..144).contains(&x) {
        [255, 0, 0, 255]
    } else if (32..208).contains(&y) && (144..176).contains(&x) {
        [0, 0, 255, 255]
    } else {
        [0, 0, 0, 255]
    }
}
