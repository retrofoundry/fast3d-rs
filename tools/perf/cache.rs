use fast3d::profiling::{now_ms, Request};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    hint::black_box,
    sync::Arc,
};

pub const HASH_VERSION: &str = "fnv1a64-v1";
pub fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Clone, Copy)]
pub enum HashAlgorithm {
    Fnv1a64,
    Xxh3,
}

impl HashAlgorithm {
    pub const ALL: [Self; 2] = [Self::Fnv1a64, Self::Xxh3];

    pub fn version(self) -> &'static str {
        match self {
            Self::Fnv1a64 => HASH_VERSION,
            Self::Xxh3 => "xxh3-64-v1",
        }
    }

    pub fn digest(self, bytes: &[u8]) -> u64 {
        match self {
            Self::Fnv1a64 => digest(bytes),
            Self::Xxh3 => twox_hash::XxHash3_64::oneshot(bytes),
        }
    }
}

pub fn verify_hash_inputs(requests: &[Request]) -> serde_json::Value {
    let mut canonical = HashMap::new();
    for request in requests.iter().filter(|r| r.rejection.is_none()) {
        let key = request.canonical(&request.executor().prepare().unwrap());
        if let Some(previous) = canonical.insert(key, &request.output) {
            assert_eq!(previous, &request.output, "equal keys must decode equally");
        }
    }
    let hashes: Vec<_> = HashAlgorithm::ALL
        .into_iter()
        .map(|algorithm| {
            let mut digests = HashMap::new();
            let mut collisions = 0;
            for key in canonical.keys() {
                if digests.insert(algorithm.digest(key), key).is_some() {
                    collisions += 1;
                }
            }
            serde_json::json!({"hash_version":algorithm.version(),"distinct_digests":digests.len(),"collisions":collisions})
        })
        .collect();
    serde_json::json!({"requests":requests.len(),"distinct_canonical_inputs":canonical.len(),"hashes":hashes,"equal_key_outputs_verified":true})
}

#[derive(Clone)]
struct Entry {
    key: Vec<u8>,
    pixels: Arc<Vec<u8>>,
    touched: u64,
    pinned_until: u64,
}
impl Entry {
    fn bytes(&self) -> usize {
        self.key.len() + self.pixels.len()
    }
}

pub struct Simulator {
    entries: HashMap<u64, Vec<Entry>>,
    budget: usize,
    retained: usize,
    clock: u64,
    pub evictions: u64,
    pub bypasses: u64,
    pub high_water: usize,
    pub pin_frames: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Hits {
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub rejected: u64,
    pub bypasses: u64,
    pub encoded_bytes: u64,
    pub hashed_bytes: u64,
    pub decoded_bytes: u64,
}

impl Simulator {
    pub fn new(budget: usize, pin_frames: u64) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            retained: 0,
            clock: 0,
            evictions: 0,
            bypasses: 0,
            high_water: 0,
            pin_frames,
        }
    }
    pub fn pinned_bytes(&self, serial: u64) -> usize {
        self.entries
            .values()
            .flatten()
            .filter(|e| e.pinned_until >= serial)
            .map(Entry::bytes)
            .sum()
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained
    }
    fn clear_for_miss(&mut self) {
        self.evictions += self.entries.values().map(|b| b.len() as u64).sum::<u64>();
        self.entries.clear();
        self.retained = 0;
    }
    fn lookup(&mut self, key: &[u8], serial: u64, hash: u64) -> Option<Arc<Vec<u8>>> {
        self.clock += 1;
        let entry = self
            .entries
            .get_mut(&hash)?
            .iter_mut()
            .find(|e| e.key == key)?;
        entry.touched = self.clock;
        entry.pinned_until = serial + self.pin_frames;
        Some(Arc::clone(&entry.pixels))
    }
    pub fn access(&mut self, key: Vec<u8>, pixels: &[u8], serial: u64, hash: u64) -> (bool, bool) {
        if let Some(image) = self.lookup(&key, serial, hash) {
            black_box(image);
            return (true, false);
        }
        (false, !self.insert(key, pixels.to_vec(), serial, hash))
    }
    fn insert(&mut self, key: Vec<u8>, pixels: Vec<u8>, serial: u64, hash: u64) -> bool {
        self.clock += 1;
        let size = key.len() + pixels.len();
        while self.retained + size > self.budget {
            let oldest = self
                .entries
                .iter()
                .flat_map(|(&hash, bucket)| {
                    bucket.iter().enumerate().map(move |(i, e)| (hash, i, e))
                })
                .filter(|(_, _, e)| e.pinned_until < serial)
                .min_by_key(|(_, _, e)| e.touched)
                .map(|(h, i, _)| (h, i));
            let Some((hash, i)) = oldest else {
                self.bypasses += 1;
                return false;
            };
            let bucket = self.entries.get_mut(&hash).unwrap();
            self.retained -= bucket.swap_remove(i).bytes();
            if bucket.is_empty() {
                self.entries.remove(&hash);
            }
            self.evictions += 1;
        }
        self.entries.entry(hash).or_default().push(Entry {
            key,
            pixels: Arc::new(pixels),
            touched: self.clock,
            pinned_until: serial + self.pin_frames,
        });
        self.retained += size;
        self.high_water = self.high_water.max(self.retained);
        true
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceRow {
    pub serial: u64,
    pub observed: bool,
    pub classes: BTreeMap<String, Hits>,
    pub retained_bytes: usize,
    pub pinned_bytes: usize,
    pub high_water_bytes: usize,
    pub evictions: u64,
    pub bypasses: u64,
    pub upper_bound: bool,
}

pub fn trace_frame(
    sim: &mut Simulator,
    serial: u64,
    observed: bool,
    requests: &[Request],
) -> TraceRow {
    let mut classes: BTreeMap<String, Hits> = BTreeMap::new();
    for request in requests {
        let row = classes
            .entry(format!("{}.{}", request.class(), request.role))
            .or_default();
        row.requests += 1;
        if request.rejection.is_some() {
            row.rejected += 1;
            continue;
        }
        let linear = request.executor().prepare().expect("trace reconstruction");
        let key = request.canonical(&linear);
        row.encoded_bytes += request.reachable_bytes as u64;
        row.hashed_bytes += key.len() as u64;
        row.decoded_bytes += request.output.len() as u64;
        let hash = digest(&key);
        let (hit, bypass) = sim.access(key, &request.output, serial, hash);
        row.hits += u64::from(hit);
        row.misses += u64::from(!hit);
        row.bypasses += u64::from(bypass);
    }
    TraceRow {
        serial,
        observed,
        classes,
        retained_bytes: sim.retained_bytes(),
        pinned_bytes: sim.pinned_bytes(serial),
        high_water_bytes: sim.high_water,
        evictions: sim.evictions,
        bypasses: sim.bypasses,
        upper_bound: true,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Costs {
    pub d: f64,
    pub k: f64,
    pub h: f64,
    pub m: f64,
}
impl Costs {
    pub fn threshold(self) -> Option<f64> {
        let denominator = self.d - self.h + self.m;
        (denominator > 0.0).then(|| (self.k + self.m) / denominator)
    }
    pub fn saving(self, hit_rate: f64) -> f64 {
        self.d - (self.k + hit_rate * self.h + (1.0 - hit_rate) * (self.d + self.m))
    }
    pub fn perfect_viable(self) -> bool {
        self.d > self.k + self.h
    }
}

#[derive(Serialize)]
pub struct CostRow {
    pub class: String,
    pub set: String,
    pub requests_in_set: usize,
    pub encoded_bytes: usize,
    pub bank_bytes: usize,
    pub hashed_bytes: usize,
    pub output_extent: [u32; 2],
    pub output_bytes: usize,
    pub components_ns: BTreeMap<String, Vec<f64>>,
    pub costs_ns: Costs,
    pub h_min: Option<f64>,
    pub perfect_hit_viable: bool,
    pub uncertainty_ns: f64,
    pub hash_bytes_per_second: f64,
    pub hash_version: &'static str,
    pub copy_multiplier_in_d: usize,
    pub baseline_model_residual_ns: f64,
    pub hit_model_residual_ns: f64,
    pub miss_model_residual_ns: f64,
    pub insertion_budget_bytes: usize,
    pub minimum_batch_ms: f64,
    pub clock_resolution_ns: f64,
    pub clock_event_ns: f64,
    pub batch_counts: BTreeMap<String, Vec<usize>>,
}

pub fn median(samples: &[f64]) -> f64 {
    let mut values = samples.to_vec();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn batch(mut operation: impl FnMut(usize), count: usize) -> f64 {
    let start = now_ms();
    for i in 0..count {
        operation(i);
    }
    (now_ms() - start) * 1_000_000.0 / count as f64
}

fn insertion_batch(
    requests: &[Request],
    keys: &[Vec<u8>],
    hashes: &[u64],
    budget: usize,
    count: usize,
) -> f64 {
    let mut cache = Simulator::new(budget, 0);
    let mut elapsed = 0.0;
    for start in (0..count).step_by(128) {
        let owned: Vec<_> = (start..(start + 128).min(count))
            .map(|i| {
                let j = i % requests.len();
                (
                    keys[j].clone(),
                    requests[j].output.clone(),
                    hashes[j],
                    i as u64 + 1,
                )
            })
            .collect();
        let start = now_ms();
        for (key, pixels, hash, serial) in owned {
            if requests.len() == 1 {
                cache.clear_for_miss();
            }
            assert!(cache.insert(key, pixels, serial, hash));
        }
        elapsed += now_ms() - start;
    }
    elapsed * 1_000_000.0 / count as f64
}

pub fn benchmark(
    requests: &[Request],
    set: &str,
    algorithm: HashAlgorithm,
    minimum_ms: f64,
    repeats: usize,
) -> CostRow {
    assert!(!requests.is_empty());
    let (resolution_ms, event_ms) = fast3d::profiling::clock_probe();
    assert!(
        resolution_ms.is_finite(),
        "clock resolution could not be measured"
    );
    let minimum_ms = minimum_ms.max(resolution_ms * 100.0);
    let mut batch_counts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let first = &requests[0];
    let executors: Vec<_> = requests.iter().map(Request::executor).collect();
    let linear: Vec<_> = executors.iter().map(|e| e.prepare().unwrap()).collect();
    let keys: Vec<_> = requests
        .iter()
        .zip(&linear)
        .map(|(r, l)| r.canonical(l))
        .collect();
    let distinct_equal = keys.clone();
    let images: Vec<_> = requests
        .iter()
        .map(|r| Arc::new(r.output.clone()))
        .collect();
    let hashes: Vec<_> = keys.iter().map(|k| algorithm.digest(k)).collect();
    let map: HashMap<_, _> = hashes
        .iter()
        .copied()
        .enumerate()
        .map(|(i, h)| (h, i))
        .collect();
    for (executor, (linear, request)) in executors.iter().zip(linear.iter().zip(requests)) {
        assert_eq!(executor.decode(linear).unwrap(), request.output);
        assert_eq!(executor.baseline_decode().unwrap(), request.output);
    }
    let names = [
        "validation",
        "reconstruction",
        "common_validation_reconstruction",
        "snapshot",
        "canonical_copy_control",
        "snapshot_and_hash",
        "hash",
        "hash_bank_only_control",
        "hash_metadata_only_control",
        "lookup",
        "equality",
        "handoff",
        "decode_with_allocation",
        "allocation_only",
        "owned_copy",
        "miss_insert_retain_evict",
        "baseline_total",
        "candidate_hit_total",
        "candidate_miss_total",
    ];
    let mut samples: BTreeMap<String, Vec<f64>> =
        names.iter().map(|n| ((*n).into(), Vec::new())).collect();
    for repeat in 0..repeats {
        for step in 0..names.len() {
            let name = names[(step + repeat) % names.len()];
            let mut retention = Simulator::new(
                (keys[0].len() + first.output.len()) * (requests.len() / 2).max(1),
                0,
            );
            let mut hits = Simulator::new(
                keys.iter()
                    .zip(requests)
                    .map(|(k, r)| k.len() + r.output.len())
                    .sum(),
                0,
            );
            for j in 0..requests.len() {
                assert!(hits.insert(keys[j].clone(), requests[j].output.clone(), 0, hashes[j]));
            }
            let mut cursor = 0;
            let mut operation = |_: usize| {
                let i = cursor;
                cursor += 1;
                let j = i % requests.len();
                match name {
                    "validation" => {
                        black_box(executors[j].validate()).unwrap();
                    }
                    "reconstruction" => {
                        black_box(executors[j].reconstruct().unwrap());
                    }
                    "common_validation_reconstruction" => {
                        black_box(executors[j].prepare().unwrap());
                    }
                    "snapshot" => {
                        black_box(requests[j].canonical(&linear[j]));
                    }
                    "canonical_copy_control" => {
                        black_box(black_box(&keys[j]).clone());
                    }
                    "snapshot_and_hash" => {
                        let key = requests[j].canonical(&linear[j]);
                        black_box(algorithm.digest(black_box(&key)));
                    }
                    "hash" => {
                        black_box(algorithm.digest(black_box(&keys[j])));
                    }
                    "hash_bank_only_control" => {
                        black_box(algorithm.digest(black_box(&keys[j][45..4141])));
                    }
                    "hash_metadata_only_control" => {
                        black_box(algorithm.digest(black_box(&keys[j][..45])));
                    }
                    "lookup" => {
                        black_box(map.get(black_box(&hashes[j])));
                    }
                    "equality" => {
                        assert!(black_box(&keys[j]) == black_box(&distinct_equal[j]));
                    }
                    "handoff" => {
                        black_box(Arc::clone(black_box(&images[j])));
                    }
                    "decode_with_allocation" => {
                        black_box(executors[j].decode(black_box(&linear[j])).unwrap());
                    }
                    "allocation_only" => {
                        black_box(vec![0u8; requests[j].output.len()]);
                    }
                    "owned_copy" => {
                        black_box(black_box(&requests[j].output).clone());
                    }
                    "baseline_total" => {
                        black_box(executors[j].baseline_decode().unwrap());
                    }
                    "candidate_hit_total" => {
                        let l = executors[j].prepare().unwrap();
                        let key = requests[j].canonical(&l);
                        black_box(
                            hits.lookup(&key, i as u64 + 1, algorithm.digest(&key))
                                .expect("resident hit"),
                        );
                    }
                    "candidate_miss_total" => {
                        if requests.len() == 1 {
                            retention.clear_for_miss();
                        }
                        let l = executors[j].prepare().unwrap();
                        let key = requests[j].canonical(&l);
                        let hash = algorithm.digest(&key);
                        assert!(
                            retention.lookup(&key, i as u64 + 1, hash).is_none(),
                            "churning miss must miss"
                        );
                        let decoded = executors[j].decode(&l).unwrap();
                        assert!(retention.insert(key, decoded, i as u64 + 1, hash));
                    }
                    _ => unreachable!(),
                }
            };
            let mut count = 64;
            let value = loop {
                let value = if name == "miss_insert_retain_evict" {
                    insertion_batch(
                        requests,
                        &keys,
                        &hashes,
                        (keys[0].len() + first.output.len()) * (requests.len() / 2).max(1),
                        count,
                    )
                } else {
                    batch(&mut operation, count)
                };
                if value * count as f64 >= minimum_ms * 1_000_000.0 || count >= 1_048_576 {
                    break value;
                }
                count *= 2;
            };
            samples.get_mut(name).unwrap().push(value);
            batch_counts.entry(name.into()).or_default().push(count);
        }
    }
    let d = median(&samples["decode_with_allocation"]);
    let k = median(&samples["snapshot"]) + median(&samples["hash"]) + median(&samples["lookup"]);
    let h = median(&samples["equality"]) + median(&samples["handoff"]);
    let m = median(&samples["miss_insert_retain_evict"]);
    let costs = Costs { d, k, h, m };
    let range = |values: &[f64]| {
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - values.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let component_range: f64 = [
        "decode_with_allocation",
        "snapshot",
        "hash",
        "lookup",
        "equality",
        "handoff",
        "miss_insert_retain_evict",
    ]
    .iter()
    .map(|name| range(&samples[*name]))
    .sum();
    let uncertainty = component_range
        .max(range(&samples["candidate_hit_total"]))
        .max(range(&samples["candidate_miss_total"]))
        .max(resolution_ms * 1_000_000.0 * 2.0 / 64.0);
    CostRow {
        class: first.class(),
        set: set.into(),
        requests_in_set: requests.len(),
        encoded_bytes: first.reachable_bytes,
        bank_bytes: 4096,
        hashed_bytes: keys[0].len(),
        output_extent: first.extent,
        output_bytes: first.output.len(),
        costs_ns: costs,
        h_min: costs.threshold(),
        perfect_hit_viable: costs.perfect_viable(),
        uncertainty_ns: uncertainty,
        hash_bytes_per_second: keys[0].len() as f64 * 1e9 / median(&samples["hash"]),
        hash_version: algorithm.version(),
        copy_multiplier_in_d: 0,
        baseline_model_residual_ns: median(&samples["baseline_total"])
            - median(&samples["common_validation_reconstruction"])
            - d,
        hit_model_residual_ns: median(&samples["candidate_hit_total"])
            - median(&samples["common_validation_reconstruction"])
            - k
            - h,
        miss_model_residual_ns: median(&samples["candidate_miss_total"])
            - median(&samples["common_validation_reconstruction"])
            - k
            - d
            - m,
        insertion_budget_bytes: (keys[0].len() + first.output.len()) * (requests.len() / 2).max(1),
        minimum_batch_ms: minimum_ms,
        clock_resolution_ns: resolution_ms * 1_000_000.0,
        clock_event_ns: event_ms * 1_000_000.0,
        batch_counts,
        components_ns: samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxh3_matches_rt64_bundled_reference_vectors() {
        // xxHash 0.8.2, both XXH3_64bits and reset/update/digest over the same bytes.
        let bytes: Vec<_> = (0..6253).map(|i| (i * 37 + 11) as u8).collect();
        for (len, expected) in [
            (0, 0x2d06800538d394c2),
            (1, 0x4a4139caf4136257),
            (45, 0x0b0b3f7105c27619),
            (4096, 0x95e9755b36d83075),
            (4141, 0xf84f6915a410c5da),
            (6253, 0x556fa5d7a1a241a4),
        ] {
            assert_eq!(HashAlgorithm::Xxh3.digest(&bytes[..len]), expected);
        }
    }

    #[test]
    fn cache_preflight_respects_budget_and_order() {
        let mut sim = Simulator::new(8, 1);
        assert_eq!(sim.access(vec![1; 2], &[4; 2], 1, 0), (false, false));
        assert_eq!(sim.access(vec![1; 2], &[4; 2], 1, 0), (true, false));
        assert_eq!(sim.access(vec![2; 2], &[5; 2], 1, 0), (false, false));
        assert_eq!(sim.access(vec![3; 2], &[6; 2], 2, 0), (false, true));
        assert_eq!(sim.pinned_bytes(2), 8);
        assert_eq!(sim.access(vec![3; 2], &[6; 2], 3, 0), (false, false));
        assert_eq!(sim.evictions, 1);
        assert_eq!(sim.bypasses, 1);
        assert_eq!(sim.high_water, 8);
        assert_eq!(sim.retained_bytes(), 8);
    }
    #[test]
    fn cache_break_even_handles_unprofitable_hits() {
        let small = Costs {
            d: 10.0,
            k: 9.0,
            h: 2.0,
            m: 3.0,
        };
        assert!(!small.perfect_viable());
        assert!(small.threshold().unwrap() >= 1.0);
        assert!(small.saving(1.0) < 0.0);
        let impossible = Costs {
            d: 1.0,
            k: 1.0,
            h: 4.0,
            m: 2.0,
        };
        assert!(impossible.threshold().is_none());
        let large = Costs {
            d: 100.0,
            k: 9.0,
            h: 2.0,
            m: 3.0,
        };
        let threshold = large.threshold().unwrap();
        assert!(large.saving(threshold).abs() < 1e-12);
        assert!(large.saving(threshold - 0.01) < 0.0);
        assert!(large.saving(threshold + 0.01) > 0.0);
        let viable: Vec<_> = [small, large, small, large]
            .iter()
            .enumerate()
            .filter(|(_, c)| c.perfect_viable())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(viable, [1, 3]);
    }
    #[test]
    fn cache_preflight_accounts_for_hit_and_miss_work() {
        assert_eq!(digest(b""), 0xcbf29ce484222325);
        assert_eq!(digest(b"a"), 0xaf63dc4c8601ec8c);
        let request = fast3d::profiling::authored_requests().remove(0);
        let mut sim = Simulator::new(1024 * 1024, 2);
        let frame = trace_frame(&mut sim, 1, false, &[request.clone(), request.clone()]);
        let row = frame.classes.values().next().unwrap();
        assert_eq!((row.requests, row.hits, row.misses), (2, 1, 1));
        assert_eq!(
            row.hashed_bytes,
            2 * request
                .canonical(&request.executor().prepare().unwrap())
                .len() as u64
        );
        assert_eq!(row.decoded_bytes, 2 * request.output.len() as u64);
        let expected = (request.canonical(&[]).len() + request.output.len()) as u64;
        assert_eq!(sim.retained_bytes() as u64, expected);
    }
}
