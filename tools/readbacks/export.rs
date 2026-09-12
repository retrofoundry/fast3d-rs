use std::io::Write;

fn quoted(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            ch if ch.is_control() => result.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

fn write_new(path: &std::path::Path, bytes: &[u8]) {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap_or_else(|error| panic!("readback {}: {error}", path.display()))
        .write_all(bytes)
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
pub fn write_row(
    env_var: &str,
    id: &str,
    stage: &str,
    role: &str,
    blend_path: &str,
    pixels: &[u8],
    width: u32,
    height: u32,
    input: &[u8],
    device: Option<&wgpu::Device>,
) {
    let Some(directory) = std::env::var_os(env_var) else {
        return;
    };
    assert!(id
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_'));
    assert_eq!(pixels.len(), width as usize * height as usize * 4);
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let adapter = device.map_or_else(
        || String::from("null"),
        |device| {
            let info = device.adapter_info();
            format!(
                "{{\"name\":{},\"vendor\":{},\"device\":{},\"device_type\":{},\"driver\":{},\"driver_info\":{},\"backend\":{},\"features\":{}}}",
                quoted(&info.name), info.vendor, info.device, quoted(&format!("{:?}", info.device_type)),
                quoted(&info.driver), quoted(&info.driver_info), quoted(&format!("{:?}", info.backend)),
                quoted(&format!("{:?}", device.features())),
            )
        },
    );
    let thread = std::thread::current();
    let test_id = thread
        .name()
        .expect("readback writer must run in a named test");
    let metadata = format!(
        "{{\"id\":{},\"stage\":{},\"role\":{},\"blend_path\":{},\"test_id\":{},\"width\":{},\"height\":{},\"channels\":\"RGBA\",\"adapter\":{}}}",
        quoted(id), quoted(stage), quoted(role), quoted(blend_path), quoted(test_id), width, height, adapter,
    );
    write_new(&directory.join(format!("{id}.bin")), pixels);
    write_new(&directory.join(format!("{id}.input.bin")), input);
    write_new(&directory.join(format!("{id}.json")), metadata.as_bytes());
}

#[test]
fn raw_export_preserves_alpha_and_refuses_an_overwrite() {
    let thread = std::thread::current();
    let name = thread.name().unwrap().replace("::", "_");
    let variable = format!("FAST3D_EXPORT_TEST_{}_{}", std::process::id(), name);
    let directory = std::env::temp_dir().join(&variable);
    std::env::set_var(&variable, &directory);
    let export = || {
        write_row(
            &variable,
            "tile",
            "decode",
            "cpu-oracle",
            "none",
            &[17, 33, 65, 7],
            1,
            1,
            &[9, 8],
            None,
        );
    };
    export();
    assert_eq!(
        std::fs::read(directory.join("tile.bin")).unwrap(),
        [17, 33, 65, 7]
    );
    assert_eq!(
        std::fs::read(directory.join("tile.input.bin")).unwrap(),
        [9, 8]
    );
    let metadata = std::fs::read_to_string(directory.join("tile.json")).unwrap();
    assert!(metadata.contains("\"channels\":\"RGBA\""));
    assert!(metadata.contains("\"role\":\"cpu-oracle\""));
    assert!(metadata.contains(thread.name().unwrap()));
    assert!(std::panic::catch_unwind(export).is_err());
    assert_eq!(
        std::fs::read(directory.join("tile.bin")).unwrap(),
        [17, 33, 65, 7]
    );
    std::env::remove_var(variable);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn adapter_text_escapes_json_control_characters() {
    let name = "driver\\gpu\"\n\u{7} 東京";
    assert_eq!(quoted(name), "\"driver\\\\gpu\\\"\\u000a\\u0007 東京\"");
}
