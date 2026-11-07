use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Weak},
};

use crate::{hle::texture_request::TextureKey, profiling::Recorder};

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub entries: usize,
    pub gpu_bytes: u64,
    pub cpu_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 4096,
            gpu_bytes: 64 * 1024 * 1024,
            cpu_bytes: 8 * 1024 * 1024,
        }
    }
}

pub(super) struct Resident<T> {
    pub key: TextureKey,
    pub value: T,
    pub gpu_bytes: u64,
    pub admitted: bool,
    witness: Arc<[u8]>,
}

impl<T> Resident<T> {
    pub fn cpu_bytes(&self) -> usize {
        self.witness.len()
    }
}

struct Entry<T> {
    image: Arc<Resident<T>>,
    last_submission: u64,
    touched: bool,
    order: u64,
}

pub(super) struct Residency<T> {
    limits: Limits,
    buckets: HashMap<TextureKey, Vec<Entry<T>>>,
    pending: HashMap<TextureKey, Vec<Weak<Resident<T>>>>,
    submission: u64,
    entries: usize,
    gpu_bytes: u64,
    cpu_bytes: usize,
    touched: Vec<TextureKey>,
    lru: BTreeMap<u64, TextureKey>,
    next_order: u64,
}

impl<T> Residency<T> {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            buckets: HashMap::new(),
            pending: HashMap::new(),
            submission: 0,
            entries: 0,
            gpu_bytes: 0,
            cpu_bytes: 0,
            touched: Vec::new(),
            lru: BTreeMap::new(),
            next_order: 0,
        }
    }

    pub fn resolve(
        &mut self,
        key: TextureKey,
        witness: Arc<[u8]>,
        gpu_bytes: u64,
        profiling: &Recorder,
        create: impl FnOnce() -> T,
    ) -> Arc<Resident<T>> {
        let equal = |candidate: &[u8]| {
            profiling.count("tmem.witness_comparisons", 1);
            if candidate.len() != witness.len() {
                return false;
            }
            profiling.count("tmem.bytes_compared", witness.len() as u64);
            candidate == &*witness
        };
        if let Some(bucket) = self.buckets.get_mut(&key) {
            for entry in bucket {
                if equal(&entry.image.witness) {
                    if !entry.touched {
                        self.touched.push(key);
                    }
                    entry.touched = true;
                    profiling.count("tmem.hits", 1);
                    return entry.image.clone();
                }
            }
        }
        if let Some(bucket) = self.pending.get(&key) {
            for image in bucket.iter().filter_map(Weak::upgrade) {
                if equal(&image.witness) {
                    profiling.count("tmem.hits", 1);
                    profiling.count("tmem.pending_hits", 1);
                    return image;
                }
            }
        }
        profiling.count("tmem.misses", 1);
        let admitted = self.admit(gpu_bytes, witness.len(), profiling);
        let image = Arc::new(Resident {
            key,
            value: create(),
            gpu_bytes,
            admitted,
            witness,
        });
        if admitted {
            self.entries += 1;
            self.gpu_bytes += gpu_bytes;
            self.cpu_bytes += image.witness.len();
            let order = self.next_order;
            self.next_order = self
                .next_order
                .checked_add(1)
                .expect("texture residency order overflow");
            self.lru.insert(order, key);
            self.touched.push(key);
            self.buckets.entry(key).or_default().push(Entry {
                image: image.clone(),
                last_submission: self.submission,
                touched: true,
                order,
            });
        } else {
            profiling.count("tmem.bypasses", 1);
            self.pending
                .entry(key)
                .or_default()
                .push(Arc::downgrade(&image));
        }
        self.profile_payload(profiling);
        image
    }

    fn admit(&mut self, gpu: u64, cpu: usize, profiling: &Recorder) -> bool {
        if self.limits.entries == 0 || gpu > self.limits.gpu_bytes || cpu > self.limits.cpu_bytes {
            return false;
        }
        while self.entries >= self.limits.entries
            || self.gpu_bytes > self.limits.gpu_bytes - gpu
            || self.cpu_bytes > self.limits.cpu_bytes - cpu
        {
            let victim = self.lru.iter().find_map(|(order, key)| {
                self.buckets[key]
                    .iter()
                    .position(|entry| entry.order == *order && Arc::strong_count(&entry.image) == 1)
                    .map(|slot| (*key, slot, *order))
            });
            let Some((key, slot, order)) = victim else {
                return false;
            };
            self.lru.remove(&order);
            let bucket = self.buckets.get_mut(&key).unwrap();
            let entry = bucket.remove(slot);
            self.entries -= 1;
            self.gpu_bytes -= entry.image.gpu_bytes;
            self.cpu_bytes -= entry.image.witness.len();
            if bucket.is_empty() {
                self.buckets.remove(&key);
            }
            profiling.count("tmem.evictions", 1);
        }
        true
    }

    pub fn submitted(&mut self) {
        self.submission = self.submission.saturating_add(1);
        for key in self.touched.drain(..) {
            if let Some(bucket) = self.buckets.get_mut(&key) {
                for entry in bucket {
                    if entry.touched {
                        entry.last_submission = self.submission;
                        entry.touched = false;
                        self.lru.remove(&entry.order);
                        entry.order = self.next_order;
                        self.next_order = self
                            .next_order
                            .checked_add(1)
                            .expect("texture residency order overflow");
                        self.lru.insert(entry.order, key);
                    }
                }
            }
        }
        self.pending.retain(|_, bucket| {
            bucket.retain(|image| image.strong_count() != 0);
            !bucket.is_empty()
        });
    }

    pub fn is_pending(&self, image: &Arc<Resident<T>>) -> bool {
        self.pending.get(&image.key).is_some_and(|bucket| {
            bucket
                .iter()
                .any(|entry| entry.as_ptr() == Arc::as_ptr(image))
        })
    }

    pub fn retain_pending(&mut self, mut active: impl FnMut(*const Resident<T>) -> bool) {
        self.pending.retain(|_, bucket| {
            bucket.retain(|entry| active(entry.as_ptr()));
            !bucket.is_empty()
        });
    }

    pub fn profile(&self, profiling: &Recorder) {
        if !profiling.active() {
            return;
        }
        self.profile_payload(profiling);
        let pinned = self
            .buckets
            .values()
            .flatten()
            .filter(|entry| Arc::strong_count(&entry.image) > 1);
        let (entries, gpu, cpu) = pinned.fold((0, 0, 0), |(entries, gpu, cpu), entry| {
            (
                entries + 1,
                gpu + entry.image.gpu_bytes,
                cpu + entry.image.witness.len(),
            )
        });
        profiling.gauge("tmem.pinned_entries", entries);
        profiling.gauge("tmem.pinned_gpu_bytes", gpu);
        profiling.gauge("tmem.pinned_cpu_bytes", cpu as u64);
        let overhead = self.buckets.capacity()
            * (std::mem::size_of::<TextureKey>() + std::mem::size_of::<Vec<Entry<T>>>() + 1)
            + self
                .buckets
                .values()
                .map(|bucket| bucket.capacity() * std::mem::size_of::<Entry<T>>())
                .sum::<usize>()
            + self.entries
                * (std::mem::size_of::<Resident<T>>() + 4 * std::mem::size_of::<usize>());
        let overhead = overhead
            + self.touched.capacity() * std::mem::size_of::<TextureKey>()
            + self.lru.len()
                * (std::mem::size_of::<TextureKey>() + 4 * std::mem::size_of::<usize>())
            + self.pending.capacity()
                * (std::mem::size_of::<TextureKey>()
                    + std::mem::size_of::<Vec<Weak<Resident<T>>>>()
                    + 1)
            + self
                .pending
                .values()
                .map(|bucket| bucket.capacity() * std::mem::size_of::<Weak<Resident<T>>>())
                .sum::<usize>();
        profiling.gauge("tmem.resident_allocation_overhead_bytes", overhead as u64);
        profiling.gauge(
            "cpu_upload_cache_allocation_overhead_bytes",
            overhead as u64,
        );
    }

    fn profile_payload(&self, profiling: &Recorder) {
        profiling.gauge("tmem.resident_entries", self.entries as u64);
        profiling.gauge("tmem.resident_gpu_bytes", self.gpu_bytes);
        profiling.gauge("tmem.resident_cpu_bytes", self.cpu_bytes as u64);
        profiling.gauge("gpu_decoded_texture_bytes", self.gpu_bytes);
        profiling.gauge("cpu_upload_cache_bytes", self.cpu_bytes as u64);
    }
}

#[cfg(test)]
mod tests;
