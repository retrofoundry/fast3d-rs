use fast3d::models::color::R5G5B5A1;
use fast3d::RCP;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn test_color() {
    let color = R5G5B5A1::to_rgba(0x7FFF);
    assert_eq!(color.r, 0.48387095);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn test_rcp() {
    let mut rcp = RCP::default();
    rcp.reset();
    assert!(true);
}