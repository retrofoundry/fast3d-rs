use super::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn source(
    source: String,
    mode: String,
    readback: bool,
    one_frame: bool,
) -> Result<String, JsValue> {
    let mode = match mode.as_str() {
        "counters" => Mode::Counters,
        "coarse" => Mode::Coarse,
        "detailed" => Mode::Detailed,
        "trace" => Mode::Trace,
        _ => return Err(JsValue::from_str("unknown mode")),
    };
    let mut frames = Vec::new();
    let setup = source_gpu(
        &source,
        mode,
        readback,
        if one_frame { 1 } else { 2 },
        |frame| frames.push(frame),
    )
    .await
    .map_err(|e| JsValue::from_str(&e))?;
    serde_json::to_string(&json!({"setup":setup,"frames":frames}))
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn preflight(cases: String) -> Result<String, JsValue> {
    let requests = if cases.is_empty() {
        fast3d::profiling::authored_requests()
            .into_iter()
            .flat_map(|r| (0..16).map(move |xor| r.diagnostic_variant(xor)))
            .collect()
    } else {
        serde_json::from_str::<Vec<fast3d::profiling::Request>>(&cases)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
    };
    let hash_inputs = cache::verify_hash_inputs(&requests);
    let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for request in requests {
        if request.rejection.is_none() {
            groups.entry(request.class()).or_default().push(request);
        }
    }
    let mut rows = Vec::new();
    for (index, cases) in groups.values().enumerate() {
        for (set, inputs) in [
            ("hot-distinct-equal", &cases[..1]),
            ("rotating", &cases[..]),
        ] {
            for step in 0..cache::HashAlgorithm::ALL.len() {
                let algorithm = cache::HashAlgorithm::ALL[(index + step) % 2];
                rows.push(cache::benchmark(inputs, set, algorithm, 1.0, 5));
            }
        }
    }
    serde_json::to_string(
        &json!({"clock":fast3d::profiling::clock_probe(),"hash_inputs":hash_inputs,"costs":rows}),
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub async fn sequence(
    bytes: Vec<u8>,
    mode: String,
    readback: bool,
    one_frame: bool,
) -> Result<String, JsValue> {
    let mode = match mode.as_str() {
        "counters" => Mode::Counters,
        "coarse" => Mode::Coarse,
        "detailed" => Mode::Detailed,
        "trace" => Mode::Trace,
        _ => return Err(JsValue::from_str("unknown mode")),
    };
    let sequence = fast3d::capture::Sequence::from_bytes(&bytes)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let (device, queue, adapter) = sequence
        .measurement_device()
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let features = format!("{:?}", device.features());
    let limits = format!("{:?}", device.limits());
    let configuration = format!("{:?}", sequence.frames[0].frame);
    let mut frames = Vec::new();
    let setup = sequence
        .measure(
            device,
            queue,
            fast3d::capture::MeasurementOptions {
                readback,
                mode,
                frames_in_flight: if one_frame { 1 } else { 2 },
                ..Default::default()
            },
            |frame| frames.push(frame_record(&frame)),
        )
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&json!({"adapter":format!("{adapter:?}"),"features":features,"limits":limits,"configuration":configuration,"mode":mode,"frames_in_flight":if one_frame {1}else{2},"setup":setup,"frames":frames}))
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
