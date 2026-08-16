//! node:perf_hooks histograms — `createHistogram()` (RecordableHistogram) and
//! `monitorEventLoopDelay()` (ELDHistogram).
//!
//! Node backs both with an HDR histogram (`deps/histogram`), and every number a
//! JS caller can read — `min`/`max`/`mean`/`stddev`, `percentile(p)`, the
//! `percentiles` Map — is a function of that structure's bucketed counts, not
//! of the raw samples. Reproducing the bucketing is therefore not an
//! implementation detail: `percentile(1) === min` and `percentile(100) === max`
//! hold only because both sides quantize identically. The code below is a port
//! of the parts of `hdr_histogram.c` those accessors reach (`counts_index_for`,
//! `hdr_value_at_index`, `hdr_value_at_percentile`, `percentile_iter_next`,
//! `hdr_mean`, `hdr_stddev`, `hdr_add`).
//!
//! Instances are `perf_histogram`-tagged native-module namespace objects
//! carrying a registry index in field[1] — the same shape `perf_observer` uses.
//! Property reads land in `object::native_module::constants` and method calls
//! in `object::native_module_dispatch::dispatch_m_p`; both re-derive the index
//! from the receiver, so two handles are genuinely independent state.

use crate::object::ObjectHeader;
use crate::value::JSValue;
use std::cell::RefCell;
use std::time::Instant;

/// The two constructors Node hands back. They share every accessor; the kind
/// decides `constructor.name` and whether `enable()`/`disable()` do anything.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistogramKind {
    Recordable,
    Eld,
}

impl HistogramKind {
    fn ctor_name(self) -> &'static str {
        match self {
            HistogramKind::Recordable => "RecordableHistogram",
            HistogramKind::Eld => "ELDHistogram",
        }
    }
}

/// Port of `struct hdr_histogram` plus the JS-visible extras Node keeps beside
/// it (`exceeds`, the enable/disable flag, the `recordDelta` timestamp, and the
/// shared percentiles Map).
pub(crate) struct Histogram {
    kind: HistogramKind,
    unit_magnitude: u32,
    sub_bucket_half_count_magnitude: u32,
    sub_bucket_count: i64,
    sub_bucket_half_count: i64,
    sub_bucket_mask: i64,
    counts: Vec<u64>,
    total_count: u64,
    /// Smallest non-zero value recorded; `i64::MAX` while empty — this is what
    /// `hdr_min` reports for an empty histogram, and what
    /// `histogram.minBigInt === 9223372036854775807n` asserts.
    min_value: i64,
    max_value: i64,
    exceeds: u64,
    enabled: bool,
    prev_delta: Option<Instant>,
    /// Cached `percentiles` Map (NaN-boxed bits, 0 = uninit). Node's
    /// `percentiles` and `percentilesBigInt` getters clear and refill ONE Map
    /// held on the instance, so reading both hands back the same object holding
    /// the second getter's value types.
    map_bits: u64,
}

/// `hdr_iter` (the "all values" walk) — the shared cursor `hdr_mean`,
/// `hdr_value_at_percentile` and the percentile iterator all drive.
struct BasicIter<'h> {
    h: &'h Histogram,
    index: i64,
    cumulative: u64,
    count: u64,
    value: i64,
}

impl BasicIter<'_> {
    fn step(&mut self) -> bool {
        self.index += 1;
        if self.index < 0 || self.index as usize >= self.h.counts.len() {
            return false;
        }
        self.count = self.h.counts[self.index as usize];
        self.cumulative += self.count;
        self.value = self.h.value_at_index(self.index as usize);
        true
    }
}

impl Histogram {
    fn new(kind: HistogramKind, lowest: i64, highest: i64, figures: u32) -> Self {
        let largest_value_with_single_unit_resolution = 2 * 10i64.pow(figures);
        let unit_magnitude = (lowest as f64).log2().floor() as u32;
        let sub_bucket_count_magnitude = (largest_value_with_single_unit_resolution as f64)
            .log2()
            .ceil() as u32;
        let sub_bucket_half_count_magnitude = sub_bucket_count_magnitude.max(1) - 1;
        let sub_bucket_count = 1i64 << (sub_bucket_half_count_magnitude + 1);
        let sub_bucket_half_count = sub_bucket_count / 2;
        let sub_bucket_mask = (sub_bucket_count - 1) << unit_magnitude;

        // `buckets_needed_to_cover_value`: keep doubling the smallest value the
        // current bucket set cannot track until it passes `highest`.
        let mut smallest_untrackable = sub_bucket_count << unit_magnitude;
        let mut bucket_count: u32 = 1;
        while smallest_untrackable <= highest {
            if smallest_untrackable > i64::MAX / 2 {
                bucket_count += 1;
                break;
            }
            smallest_untrackable <<= 1;
            bucket_count += 1;
        }
        let counts_len = (bucket_count as usize + 1) * sub_bucket_half_count as usize;

        Self {
            kind,
            unit_magnitude,
            sub_bucket_half_count_magnitude,
            sub_bucket_count,
            sub_bucket_half_count,
            sub_bucket_mask,
            counts: vec![0; counts_len],
            total_count: 0,
            min_value: i64::MAX,
            max_value: 0,
            exceeds: 0,
            enabled: false,
            prev_delta: None,
            map_bits: 0,
        }
    }

    fn iter(&self) -> BasicIter<'_> {
        BasicIter {
            h: self,
            index: -1,
            cumulative: 0,
            count: 0,
            value: 0,
        }
    }

    fn reset(&mut self) {
        self.counts.iter_mut().for_each(|c| *c = 0);
        self.total_count = 0;
        self.min_value = i64::MAX;
        self.max_value = 0;
        self.exceeds = 0;
    }

    fn bucket_index(&self, value: i64) -> i32 {
        let pow2ceiling = 64 - (value | self.sub_bucket_mask).leading_zeros() as i32;
        pow2ceiling - self.unit_magnitude as i32 - (self.sub_bucket_half_count_magnitude as i32 + 1)
    }

    fn sub_bucket_index(&self, value: i64, bucket_index: i32) -> i32 {
        (value >> (bucket_index + self.unit_magnitude as i32)) as i32
    }

    fn counts_index_for(&self, value: i64) -> i32 {
        let bucket_index = self.bucket_index(value);
        let sub_bucket_index = self.sub_bucket_index(value, bucket_index);
        let bucket_base_index = (bucket_index + 1) << self.sub_bucket_half_count_magnitude;
        let offset_in_bucket = sub_bucket_index - self.sub_bucket_half_count as i32;
        bucket_base_index + offset_in_bucket
    }

    fn value_at_index(&self, index: usize) -> i64 {
        let mut bucket_index = (index as i32 >> self.sub_bucket_half_count_magnitude) - 1;
        let mut sub_bucket_index = (index as i32 & (self.sub_bucket_half_count as i32 - 1))
            + self.sub_bucket_half_count as i32;
        if bucket_index < 0 {
            sub_bucket_index -= self.sub_bucket_half_count as i32;
            bucket_index = 0;
        }
        (sub_bucket_index as i64) << (bucket_index + self.unit_magnitude as i32)
    }

    fn size_of_equivalent_range(&self, value: i64) -> i64 {
        let bucket_index = self.bucket_index(value);
        let sub_bucket_index = self.sub_bucket_index(value, bucket_index);
        let adjusted_bucket = if sub_bucket_index as i64 >= self.sub_bucket_count {
            bucket_index + 1
        } else {
            bucket_index
        };
        1i64 << (self.unit_magnitude as i32 + adjusted_bucket)
    }

    fn lowest_equivalent(&self, value: i64) -> i64 {
        let bucket_index = self.bucket_index(value);
        let sub_bucket_index = self.sub_bucket_index(value, bucket_index);
        (sub_bucket_index as i64) << (bucket_index + self.unit_magnitude as i32)
    }

    fn highest_equivalent(&self, value: i64) -> i64 {
        self.lowest_equivalent(value) + self.size_of_equivalent_range(value) - 1
    }

    fn median_equivalent(&self, value: i64) -> i64 {
        self.lowest_equivalent(value) + (self.size_of_equivalent_range(value) >> 1)
    }

    /// `hdr_record_values` — false when `value` falls outside the tracked
    /// range, which is what Node counts as `exceeds`.
    fn record_values(&mut self, value: i64, count: u64) -> bool {
        if value < 0 {
            return false;
        }
        let index = self.counts_index_for(value);
        if index < 0 || index as usize >= self.counts.len() {
            return false;
        }
        self.counts[index as usize] += count;
        self.total_count += count;
        if value != 0 && value < self.min_value {
            self.min_value = value;
        }
        if value > self.max_value {
            self.max_value = value;
        }
        true
    }

    fn record(&mut self, value: i64) {
        if !self.record_values(value, 1) {
            self.exceeds += 1;
        }
    }

    fn min(&self) -> i64 {
        if self.counts.first().copied().unwrap_or(0) > 0 {
            return 0;
        }
        self.min_value
    }

    fn max(&self) -> i64 {
        if self.max_value == 0 {
            0
        } else {
            self.highest_equivalent(self.max_value)
        }
    }

    fn mean(&self) -> f64 {
        if self.total_count == 0 {
            return f64::NAN;
        }
        let mut total = 0i128;
        let mut it = self.iter();
        while it.step() {
            if it.count != 0 {
                total += it.count as i128 * self.median_equivalent(it.value) as i128;
            }
        }
        total as f64 / self.total_count as f64
    }

    fn stddev(&self) -> f64 {
        if self.total_count == 0 {
            return f64::NAN;
        }
        let mean = self.mean();
        let mut geometric_dev_total = 0.0f64;
        let mut it = self.iter();
        while it.step() {
            if it.count != 0 {
                let dev = self.median_equivalent(it.value) as f64 - mean;
                geometric_dev_total += dev * dev * it.count as f64;
            }
        }
        (geometric_dev_total / self.total_count as f64).sqrt()
    }

    /// `hdr_value_at_percentile`.
    fn percentile(&self, percentile: f64) -> i64 {
        let requested = if percentile < 100.0 {
            percentile
        } else {
            100.0
        };
        let mut count_at_percentile = ((requested / 100.0) * self.total_count as f64 + 0.5) as i64;
        if count_at_percentile < 1 {
            count_at_percentile = 1;
        }
        let mut total = 0i64;
        let mut it = self.iter();
        while it.step() {
            total += it.count as i64;
            if total >= count_at_percentile {
                return self.highest_equivalent(it.value);
            }
        }
        0
    }

    /// `hdr_iter_percentile` with `ticks_per_half_distance = 1` — the iterator
    /// `Histogram::GetPercentiles` drives, reporting
    /// `(percentile, highest_equivalent_value)` pairs.
    fn percentile_entries(&self) -> Vec<(f64, i64)> {
        let mut out: Vec<(f64, i64)> = Vec::new();
        if self.total_count == 0 {
            return out;
        }
        let mut it = self.iter();
        let mut percentile_to_iterate_to = 0.0f64;
        loop {
            if it.cumulative >= self.total_count {
                // `seen_last_value`: one final report pinned at 100%.
                out.push((100.0, self.highest_equivalent(it.value)));
                break;
            }
            if it.index == -1 && !it.step() {
                break;
            }
            let mut reported = false;
            loop {
                let current_percentile = 100.0 * it.cumulative as f64 / self.total_count as f64;
                if it.count != 0 && percentile_to_iterate_to <= current_percentile {
                    let temp = ((100.0 / (100.0 - percentile_to_iterate_to)).ln()
                        / std::f64::consts::LN_2) as i64
                        + 1;
                    let half_distance = 2f64.powi(temp as i32);
                    out.push((percentile_to_iterate_to, self.highest_equivalent(it.value)));
                    percentile_to_iterate_to += 100.0 / half_distance;
                    reported = true;
                    break;
                }
                if !it.step() {
                    break;
                }
            }
            if !reported {
                break;
            }
        }
        out
    }

    /// `hdr_add` — fold every recorded value of `other` into `self`.
    fn add(&mut self, other: &Histogram) {
        let mut it = other.iter();
        while it.step() {
            if it.count != 0 && !self.record_values(it.value, it.count) {
                self.exceeds += it.count;
            }
        }
    }
}

crate::perry_thread_local! {
    /// Per-thread histogram registry. A `perf_histogram` namespace object holds
    /// only its index here, so `first !== second` and their state is disjoint.
    static HISTOGRAMS: RefCell<Vec<Histogram>> = const { RefCell::new(Vec::new()) };
}

/// Keep each instance's cached `percentiles` Map alive across GC. The Map is a
/// heap pointer held outside the GC heap (in `HISTOGRAMS`), so without this it
/// is a classic unrooted runtime cache.
pub fn scan_histogram_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    HISTOGRAMS.with(|store| {
        if let Ok(mut store) = store.try_borrow_mut() {
            for h in store.iter_mut() {
                if h.map_bits != 0 {
                    visitor.visit_nanbox_u64_slot(&mut h.map_bits);
                }
            }
        }
    });
}

// ── JS surface ───────────────────────────────────────────────────────────────

fn nan_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn number(value: f64) -> f64 {
    f64::from_bits(JSValue::number(value).bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn bigint(value: i64) -> f64 {
    let ptr = crate::bigint::js_bigint_from_i64(value);
    f64::from_bits(JSValue::bigint_ptr(ptr).bits())
}

fn throw_invalid_arg_type(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_out_of_range(message: &str) -> ! {
    crate::fs::validate::throw_range_error_with_code(message)
}

/// Node's `validateObject(value, name)` — rejects null, primitives, arrays and
/// functions. `undefined` is the caller's "argument omitted" signal and is
/// filtered before this runs.
unsafe fn validate_object(value: f64, name: &str) -> *const ObjectHeader {
    match crate::perf_hooks::as_object_ptr(value) {
        Some(obj) => obj,
        None => throw_invalid_arg_type(&format!("The \"{name}\" argument must be of type object")),
    }
}

/// Node's `validateInteger(value, name, min, max)`: a type error for anything
/// that is not a number, then a range error for non-integers and out-of-bounds
/// values (`NaN` lands in the second arm, matching Node).
fn validate_integer(value: f64, name: &str, min: f64, max: f64) -> i64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_number() {
        throw_invalid_arg_type(&format!("The \"{name}\" argument must be of type number"));
    }
    let n = jv.as_number();
    if !n.is_finite() || n.fract() != 0.0 {
        throw_out_of_range(&format!(
            "The value of \"{name}\" is out of range. It must be an integer. Received {n}"
        ));
    }
    if n < min || n > max {
        throw_out_of_range(&format!(
            "The value of \"{name}\" is out of range. It must be >= {min} && <= {max}. Received {n}"
        ));
    }
    n as i64
}

unsafe fn option_value(options: *const ObjectHeader, key: &str) -> f64 {
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let v = crate::object::js_object_get_field_by_name(options, key_str);
    f64::from_bits(v.bits())
}

/// Register `histogram` and return its `perf_histogram` namespace object.
unsafe fn make_histogram_object(histogram: Histogram) -> f64 {
    let id = HISTOGRAMS.with(|store| {
        let mut store = store.borrow_mut();
        store.push(histogram);
        store.len() - 1
    });
    crate::object::install_native_module_vtable();
    // These namespace tags never appear in user source (they are handed out as
    // return values), so codegen emits no `js_nm_install_perf()` for them and
    // the dispatch bucket would be empty — every method call on the object
    // would resolve to `undefined` and silently do nothing. Arm it here.
    crate::object::js_nm_install_perf();
    let obj = crate::object::js_object_alloc(crate::object::NATIVE_MODULE_CLASS_ID, 2);
    let module = b"perf_histogram";
    let mname = crate::string::js_string_from_bytes(module.as_ptr(), module.len() as u32);
    crate::object::js_object_set_field(obj, 0, JSValue::string_ptr(mname));
    crate::object::js_object_set_field(obj, 1, JSValue::number(id as f64));
    let mut keys = crate::array::js_array_alloc(2);
    for k in [b"__module__".as_slice(), b"__histogram_id__".as_slice()] {
        let kp = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
        keys = crate::array::js_array_push(keys, JSValue::string_ptr(kp));
    }
    crate::object::js_object_set_keys(obj, keys);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Recover the registry index from a `perf_histogram` namespace object value.
/// Returns `None` for anything else, which is what `add()`'s brand check needs.
pub(crate) unsafe fn histogram_id_from_value(value: f64) -> Option<usize> {
    let obj = crate::perf_hooks::as_object_ptr(value)?;
    if (*obj).class_id != crate::object::NATIVE_MODULE_CLASS_ID {
        return None;
    }
    let module_field = crate::object::js_object_get_field(obj as *mut _, 0);
    if !module_field.is_string() {
        return None;
    }
    let str_ptr = module_field.as_string_ptr();
    let len = (*str_ptr).byte_len as usize;
    let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name = std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("");
    if name != "perf_histogram" {
        return None;
    }
    let id_field = crate::object::js_object_get_field(obj as *mut _, 1);
    if !id_field.is_number() {
        return None;
    }
    let id = id_field.as_number() as usize;
    HISTOGRAMS.with(|store| (id < store.borrow().len()).then_some(id))
}

unsafe fn histogram_id_from_receiver(obj: *const ObjectHeader) -> Option<usize> {
    histogram_id_from_value(crate::value::js_nanbox_pointer(obj as i64))
}

/// `perf_hooks.createHistogram(options?)` — a RecordableHistogram.
#[no_mangle]
pub extern "C" fn js_perf_create_histogram(options: f64) -> f64 {
    unsafe {
        let (lowest, highest, figures) = if JSValue::from_bits(options.to_bits()).is_undefined() {
            (1i64, 9007199254740991i64, 3u32)
        } else {
            let obj = validate_object(options, "options");
            let lowest_val = option_value(obj, "lowest");
            let lowest = if JSValue::from_bits(lowest_val.to_bits()).is_undefined() {
                1
            } else {
                validate_integer(lowest_val, "options.lowest", 1.0, 9007199254740991.0)
            };
            let highest_val = option_value(obj, "highest");
            let highest = if JSValue::from_bits(highest_val.to_bits()).is_undefined() {
                9007199254740991
            } else {
                validate_integer(
                    highest_val,
                    "options.highest",
                    (2 * lowest) as f64,
                    9007199254740991.0,
                )
            };
            let figures_val = option_value(obj, "figures");
            let figures = if JSValue::from_bits(figures_val.to_bits()).is_undefined() {
                3
            } else {
                validate_integer(figures_val, "options.figures", 1.0, 5.0) as u32
            };
            (lowest, highest, figures)
        };
        make_histogram_object(Histogram::new(
            HistogramKind::Recordable,
            lowest,
            highest,
            figures,
        ))
    }
}

/// `perf_hooks.monitorEventLoopDelay(options?)` — an ELDHistogram. Perry has no
/// libuv loop to sample, so the handle starts and stays empty; the enable /
/// disable / reset lifecycle and every accessor are real.
#[no_mangle]
pub extern "C" fn js_perf_monitor_event_loop_delay(options: f64) -> f64 {
    unsafe {
        if !JSValue::from_bits(options.to_bits()).is_undefined() {
            let obj = validate_object(options, "options");
            let resolution = option_value(obj, "resolution");
            if !JSValue::from_bits(resolution.to_bits()).is_undefined() {
                validate_integer(resolution, "options.resolution", 1.0, f64::MAX);
            }
        }
        // Node's ELD histogram: lowest 1ns, highest 1h, 3 significant figures.
        make_histogram_object(Histogram::new(HistogramKind::Eld, 1, 3_600_000_000_000, 3))
    }
}

/// Fill (and cache) the instance's percentiles `Map`. Node shares one Map
/// between `percentiles` and `percentilesBigInt`, so the object identity — and
/// with it the value types a caller observes after reading both — is part of
/// the contract.
unsafe fn percentiles_map(id: usize, as_bigint: bool) -> f64 {
    let entries = HISTOGRAMS.with(|store| store.borrow()[id].percentile_entries());
    let cached = HISTOGRAMS.with(|store| store.borrow()[id].map_bits);
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_value = if cached != 0 {
        let handle = scope.root_nanbox_u64(cached);
        let map = JSValue::from_bits(handle.get_nanbox_u64()).as_pointer::<crate::map::MapHeader>();
        crate::map::js_map_clear(map as *mut _);
        handle
    } else {
        let map = crate::map::js_map_alloc(entries.len().max(4) as u32);
        scope.root_nanbox_f64(crate::value::js_nanbox_pointer(map as i64))
    };
    for (percentile, value) in entries {
        let map = JSValue::from_bits(map_value.get_nanbox_u64())
            .as_pointer::<crate::map::MapHeader>() as *mut crate::map::MapHeader;
        let boxed = if as_bigint {
            bigint(value)
        } else {
            number(value as f64)
        };
        crate::map::js_map_set(map, number(percentile), boxed);
    }
    let bits = map_value.get_nanbox_u64();
    HISTOGRAMS.with(|store| store.borrow_mut()[id].map_bits = bits);
    f64::from_bits(bits)
}

/// `histogram.toJSON()` — the plain object Node serializes. `percentiles`
/// flattens the Map into a numeric-keyed object (Node's serializer does the
/// same, so `toJSON().percentiles instanceof Map` is false).
const HISTOGRAM_JSON_SHAPE: u32 = 0x7FFF_FF53;
const HISTOGRAM_JSON_KEYS: &[u8] = b"count\0min\0max\0mean\0exceeds\0stddev\0percentiles\0";

unsafe fn histogram_to_json(id: usize) -> f64 {
    let (count, min, max, mean, exceeds, stddev, entries) = HISTOGRAMS.with(|store| {
        let store = store.borrow();
        let h = &store[id];
        (
            h.total_count as f64,
            h.min() as f64,
            h.max() as f64,
            h.mean(),
            h.exceeds as f64,
            h.stddev(),
            h.percentile_entries(),
        )
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let percentiles = crate::object::js_object_alloc(0, 0);
    let percentiles_handle =
        scope.root_nanbox_f64(crate::value::js_nanbox_pointer(percentiles as i64));
    for (percentile, value) in entries {
        let key = format!("{percentile}");
        let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let target = JSValue::from_bits(percentiles_handle.get_nanbox_u64())
            .as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(target, key_str, number(value as f64));
    }
    let obj = crate::object::js_object_alloc_with_shape(
        HISTOGRAM_JSON_SHAPE,
        7,
        HISTOGRAM_JSON_KEYS.as_ptr(),
        HISTOGRAM_JSON_KEYS.len() as u32,
    );
    crate::object::js_object_set_field(obj, 0, JSValue::number(count));
    crate::object::js_object_set_field(obj, 1, JSValue::number(min));
    crate::object::js_object_set_field(obj, 2, JSValue::number(max));
    crate::object::js_object_set_field(obj, 3, JSValue::number(mean));
    crate::object::js_object_set_field(obj, 4, JSValue::number(exceeds));
    crate::object::js_object_set_field(obj, 5, JSValue::number(stddev));
    crate::object::js_object_set_field(
        obj,
        6,
        JSValue::from_bits(percentiles_handle.get_nanbox_u64()),
    );
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Record a `timerify` call duration into the handle captured by the wrapper.
/// A non-histogram value (the common "no options.histogram" case) is ignored.
pub(crate) fn record_timerify_duration(histogram: f64, nanoseconds: i64) {
    let Some(id) = (unsafe { histogram_id_from_value(histogram) }) else {
        return;
    };
    HISTOGRAMS.with(|store| store.borrow_mut()[id].record(nanoseconds));
}

/// Property reads on a `perf_histogram` namespace object.
pub(crate) unsafe fn histogram_property(namespace_obj: f64, property: &str) -> Option<f64> {
    let id = histogram_id_from_value(namespace_obj)?;
    let stat = |f: &dyn Fn(&Histogram) -> i64| HISTOGRAMS.with(|store| f(&store.borrow()[id]));
    Some(match property {
        "count" => number(stat(&|h| h.total_count as i64) as f64),
        "countBigInt" => bigint(stat(&|h| h.total_count as i64)),
        "min" => number(stat(&|h| h.min()) as f64),
        "minBigInt" => bigint(stat(&|h| h.min())),
        "max" => number(stat(&|h| h.max()) as f64),
        "maxBigInt" => bigint(stat(&|h| h.max())),
        "mean" => number(HISTOGRAMS.with(|store| store.borrow()[id].mean())),
        "stddev" => number(HISTOGRAMS.with(|store| store.borrow()[id].stddev())),
        "exceeds" => number(stat(&|h| h.exceeds as i64) as f64),
        "exceedsBigInt" => bigint(stat(&|h| h.exceeds as i64)),
        "percentiles" => percentiles_map(id, false),
        "percentilesBigInt" => percentiles_map(id, true),
        "constructor" => {
            let kind = HISTOGRAMS.with(|store| store.borrow()[id].kind);
            crate::object::bound_native_callable_export_value("perf_histogram", kind.ctor_name())
        }
        _ => return None,
    })
}

/// Method calls on a `perf_histogram` namespace object.
pub(crate) unsafe fn histogram_method(
    obj: *const ObjectHeader,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    let arg = |n: usize| -> f64 { args.get(n).copied().unwrap_or_else(nan_undefined) };
    // `new h.constructor()` — Node's histograms are not constructible.
    if method == "RecordableHistogram" || method == "ELDHistogram" {
        crate::fs::validate::throw_type_error_with_code(
            "Illegal constructor",
            "ERR_ILLEGAL_CONSTRUCTOR",
        );
    }
    let id = histogram_id_from_receiver(obj)?;
    Some(match method {
        "record" => {
            let value = arg(0);
            let jv = JSValue::from_bits(value.to_bits());
            let recorded = if jv.is_bigint() {
                crate::bigint::js_bigint_to_f64(jv.as_bigint_ptr()) as i64
            } else {
                validate_integer(value, "val", 1.0, 9007199254740991.0)
            };
            HISTOGRAMS.with(|store| store.borrow_mut()[id].record(recorded));
            nan_undefined()
        }
        "recordDelta" => {
            HISTOGRAMS.with(|store| {
                let mut store = store.borrow_mut();
                let now = Instant::now();
                let previous = store[id].prev_delta.replace(now);
                if let Some(previous) = previous {
                    let delta = now.duration_since(previous).as_nanos();
                    store[id].record(delta.min(i64::MAX as u128) as i64);
                }
            });
            nan_undefined()
        }
        "add" => {
            let other = arg(0);
            let other_jv = JSValue::from_bits(other.to_bits());
            if other_jv.is_undefined() || other_jv.is_null() {
                // Node reads a private symbol off the argument first, so these
                // two surface as a plain TypeError with no `code`.
                crate::perf_hooks::throw_plain_type_error(
                    "Cannot read properties of undefined (reading 'Symbol(kRecordable)')",
                );
            }
            let Some(other_id) = histogram_id_from_value(other) else {
                throw_invalid_arg_type(
                    "The \"other\" argument must be an instance of RecordableHistogram",
                );
            };
            HISTOGRAMS.with(|store| {
                let mut store = store.borrow_mut();
                let (target, source) = if id < other_id {
                    let (a, b) = store.split_at_mut(other_id);
                    (&mut a[id], &b[0])
                } else if id > other_id {
                    let (a, b) = store.split_at_mut(id);
                    (&mut b[0], &a[other_id])
                } else {
                    return;
                };
                target.add(source);
            });
            nan_undefined()
        }
        "reset" => {
            HISTOGRAMS.with(|store| store.borrow_mut()[id].reset());
            nan_undefined()
        }
        "enable" | "disable" => {
            let want = method == "enable";
            bool_value(HISTOGRAMS.with(|store| {
                let mut store = store.borrow_mut();
                if store[id].kind != HistogramKind::Eld || store[id].enabled == want {
                    return false;
                }
                store[id].enabled = want;
                true
            }))
        }
        "percentile" | "percentileBigInt" => {
            let value = arg(0);
            let jv = JSValue::from_bits(value.to_bits());
            if !jv.is_number() {
                throw_invalid_arg_type("The \"percentile\" argument must be of type number");
            }
            let p = jv.as_number();
            if p.is_nan() || p <= 0.0 || p > 100.0 {
                throw_out_of_range(&format!(
                    "The value of \"percentile\" is out of range. It must be > 0 && <= 100. Received {p}"
                ));
            }
            let result = HISTOGRAMS.with(|store| store.borrow()[id].percentile(p));
            if method == "percentile" {
                number(result as f64)
            } else {
                bigint(result)
            }
        }
        "toJSON" => histogram_to_json(id),
        _ => return None,
    })
}

#[cfg(test)]
mod hdr_tests {
    use super::*;

    /// `createHistogram()`'s defaults — lowest 1, highest MAX_SAFE_INTEGER,
    /// 3 significant figures.
    fn default_histogram() -> Histogram {
        Histogram::new(HistogramKind::Recordable, 1, 9007199254740991, 3)
    }

    /// Every expectation below is the value Node 26.5.1 prints for the same
    /// sequence, captured from the oracle rather than derived from this
    /// implementation — the point of porting the HDR bucketing is that
    /// `percentile(1) === min` and `percentile(100) === max` hold for the same
    /// reason they do in Node.
    #[test]
    fn percentiles_match_the_node_oracle() {
        let mut h = default_histogram();
        for n in [1, 2, 4, 8] {
            h.record(n);
        }
        assert_eq!(h.min(), 1);
        assert_eq!(h.max(), 8);
        assert_eq!(h.percentile(1.0), h.min());
        assert_eq!(h.percentile(100.0), h.max());
        assert_eq!(h.percentile(25.0), 1);
        assert_eq!(h.percentile(75.0), 4);
        assert_eq!(h.mean(), 3.75);
        assert!((h.stddev() - 2.680951323690902).abs() < 1e-12);
        assert_eq!(
            h.percentile_entries(),
            vec![(0.0, 1), (50.0, 2), (75.0, 4), (87.5, 8), (100.0, 8)]
        );
    }

    /// The `percentiles` Map Node builds for two samples: keys 0/50/75/100.
    #[test]
    fn two_sample_percentile_map_keys() {
        let mut h = default_histogram();
        h.record(2);
        h.record(3);
        assert_eq!(
            h.percentile_entries(),
            vec![(0.0, 2), (50.0, 2), (75.0, 3), (100.0, 3)]
        );
    }

    /// `histogram/to-json`'s "stable fields: 2 3 7 0".
    #[test]
    fn to_json_stats_match_the_node_oracle() {
        let mut h = default_histogram();
        h.record(3);
        h.record(7);
        assert_eq!(h.total_count, 2);
        assert_eq!(h.min(), 3);
        assert_eq!(h.max(), 7);
        assert_eq!(h.mean(), 5.0);
        assert_eq!(h.stddev(), 2.0);
        assert_eq!(h.exceeds, 0);
    }

    /// An empty histogram reports `hdr_min`'s INT64_MAX sentinel, a zero max,
    /// and NaN for both moments — `histogram/empty-state` asserts
    /// `minBigInt === 9223372036854775807n` on exactly this.
    #[test]
    fn empty_state_sentinels() {
        let h = default_histogram();
        assert_eq!(h.min(), i64::MAX);
        assert_eq!(h.max(), 0);
        assert_eq!(h.total_count, 0);
        assert!(h.mean().is_nan());
        assert!(h.stddev().is_nan());
        assert!(h.percentile_entries().is_empty());
    }

    /// `reset()` returns to the empty sentinels.
    #[test]
    fn reset_restores_the_empty_state() {
        let mut h = default_histogram();
        h.record(5);
        h.reset();
        assert_eq!(h.min(), i64::MAX);
        assert_eq!(h.max(), 0);
        assert_eq!(h.total_count, 0);
        assert!(h.mean().is_nan());
    }

    /// `histogram/add-isolation`: `add()` copies the source's samples and the
    /// two histograms stay independent afterwards.
    #[test]
    fn add_copies_and_isolates() {
        let mut source = default_histogram();
        let mut target = default_histogram();
        source.record(4);
        source.record(8);
        target.add(&source);
        assert_eq!((target.total_count, target.min(), target.max()), (2, 4, 8));
        source.record(16);
        assert_eq!((target.total_count, target.max()), (2, 8));
    }

    /// Sub-2048 values land in unit-resolution buckets, so a Number/BigInt
    /// twin pair reads back exactly (`histogram/record-number-bigint`).
    #[test]
    fn small_values_are_exact() {
        let mut h = default_histogram();
        h.record(5);
        h.record(9);
        assert_eq!((h.total_count, h.min(), h.max()), (2, 5, 9));
        assert!(h.mean() >= 5.0 && h.mean() <= 9.0);
        assert!(h.stddev() >= 0.0);
    }

    /// A value past `highest` is not recorded — it counts as `exceeds`, which
    /// is the only thing that separates Node's `count` from `exceeds`.
    #[test]
    fn out_of_range_values_count_as_exceeds() {
        let mut h = Histogram::new(HistogramKind::Recordable, 1, 100, 3);
        h.record(50);
        h.record(1_000_000_000_000);
        assert_eq!(h.total_count, 1);
        assert_eq!(h.exceeds, 1);
    }

    /// The event-loop-delay handle's range (1 ns .. 1 h) still resolves
    /// nanosecond deltas exactly at the low end, which is what
    /// `histogram/record-delta`'s `minBigInt > 0n` needs.
    #[test]
    fn eld_range_resolves_small_deltas() {
        let mut h = Histogram::new(HistogramKind::Eld, 1, 3_600_000_000_000, 3);
        h.record(1);
        assert_eq!(h.min(), 1);
        assert!(h.max() >= 1);
    }
}
