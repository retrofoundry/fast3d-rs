use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    #[default]
    Counters,
    Coarse,
    Detailed,
    Trace,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize))]
pub struct Timing {
    pub calls: u64,
    pub inclusive_ms: f64,
    pub exclusive_ms: f64,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize))]
pub struct Snapshot {
    pub counters: BTreeMap<String, u64>,
    pub timings: BTreeMap<String, Timing>,
    pub decodes: BTreeMap<String, super::DecodeCounts>,
    pub requests: Vec<super::Request>,
    pub gauges: BTreeMap<String, u64>,
}

#[derive(Debug)]
struct State {
    mode: Mode,
    snapshot: Snapshot,
    children: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Recorder(Option<Rc<RefCell<State>>>);

impl Recorder {
    pub fn new(mode: Mode) -> Self {
        Self(Some(Rc::new(RefCell::new(State {
            mode,
            snapshot: Snapshot::default(),
            children: Vec::new(),
        }))))
    }

    pub fn snapshot(&self) -> Snapshot {
        self.0
            .as_ref()
            .map(|state| state.borrow().snapshot.clone())
            .unwrap_or_default()
    }

    pub fn drain(&self) -> Snapshot {
        self.0
            .as_ref()
            .map(|state| std::mem::take(&mut state.borrow_mut().snapshot))
            .unwrap_or_default()
    }

    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    pub fn tracing(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|state| state.borrow().mode == Mode::Trace)
    }

    pub fn count(&self, name: &'static str, value: u64) {
        if let Some(state) = &self.0 {
            *state
                .borrow_mut()
                .snapshot
                .counters
                .entry(name.into())
                .or_default() += value;
        }
    }

    pub fn gauge(&self, name: &'static str, value: u64) {
        if let Some(state) = &self.0 {
            let mut state = state.borrow_mut();
            state.snapshot.gauges.insert(name.into(), value);
            let peak = state
                .snapshot
                .gauges
                .entry(format!("{name}.peak"))
                .or_default();
            *peak = (*peak).max(value);
        }
    }

    pub fn target(&self, logical: (u32, u32), output: (u32, u32)) {
        if let Some(state) = &self.0 {
            *state
                .borrow_mut()
                .snapshot
                .counters
                .entry(format!(
                    "target.{}x{}.output{}x{}",
                    logical.0, logical.1, output.0, output.1
                ))
                .or_default() += 1;
        }
    }

    pub fn buffer(&self, class: &'static str, capacity: u64, initialized: u64) {
        self.resource("buffer", class, capacity, initialized);
    }

    pub fn texture(&self, class: &'static str, capacity: u64, uploaded: u64) {
        self.resource("texture", class, capacity, uploaded);
    }

    fn resource(&self, kind: &str, class: &str, capacity: u64, uploaded: u64) {
        if let Some(state) = &self.0 {
            let mut state = state.borrow_mut();
            for (field, n) in [
                ("creations", 1),
                ("capacity_bytes", capacity),
                ("upload_bytes", uploaded),
            ] {
                *state
                    .snapshot
                    .counters
                    .entry(format!("{kind}.{class}.{field}"))
                    .or_default() += n;
            }
        }
    }

    pub fn decode(&self, key: String, bytes: Option<usize>) {
        if let Some(state) = &self.0 {
            let mut state = state.borrow_mut();
            let row = state.snapshot.decodes.entry(key).or_default();
            row.requests += 1;
            if let Some(bytes) = bytes {
                row.executions += 1;
                row.output_bytes += bytes as u64;
            } else {
                row.rejected += 1;
            }
        }
    }

    pub fn request(&self, request: super::Request) {
        if let Some(state) = &self.0 {
            state.borrow_mut().snapshot.requests.push(request);
        }
    }

    pub fn span(&self, name: &'static str) -> Span {
        let active = self
            .0
            .as_ref()
            .is_some_and(|state| match state.borrow().mode {
                Mode::Detailed => true,
                Mode::Coarse => matches!(name, "process_dl" | "begin_frame" | "presentation"),
                Mode::Counters | Mode::Trace => false,
            });
        if active {
            self.0.as_ref().unwrap().borrow_mut().children.push(0.0);
            Span {
                recorder: self.clone(),
                name,
                start: Some(now_ms()),
            }
        } else {
            Span {
                recorder: Self::default(),
                name,
                start: None,
            }
        }
    }
}

pub struct Span {
    recorder: Recorder,
    name: &'static str,
    start: Option<f64>,
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let elapsed = now_ms() - start;
            let mut state = self.recorder.0.as_ref().unwrap().borrow_mut();
            let children = state.children.pop().unwrap();
            if let Some(parent) = state.children.last_mut() {
                *parent += elapsed;
            }
            let row = state.snapshot.timings.entry(self.name.into()).or_default();
            row.calls += 1;
            row.inclusive_ms += elapsed;
            row.exclusive_ms += (elapsed - children).max(0.0);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

#[cfg(all(target_arch = "wasm32", feature = "profiling"))]
pub fn now_ms() -> f64 {
    web_sys::window()
        .expect("profiling requires a browser window")
        .performance()
        .expect("performance clock")
        .now()
}

#[cfg(all(target_arch = "wasm32", not(feature = "profiling")))]
pub fn now_ms() -> f64 {
    0.0
}

pub fn clock_probe() -> (f64, f64) {
    let start = now_ms();
    let mut previous = start;
    let mut resolution = f64::INFINITY;
    for _ in 0..10_000 {
        let next = std::hint::black_box(now_ms());
        if next > previous {
            resolution = resolution.min(next - previous);
        }
        previous = next;
    }
    (resolution, (now_ms() - start) / 10_000.0)
}
