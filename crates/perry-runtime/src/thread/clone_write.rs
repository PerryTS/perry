//! Writer half of the thread clone: a JS value becomes a [`SerializedValue`]
//! made only of owned data, so it can cross to another thread.
//!
//! Two callers with slightly different rules share it:
//! - `perry/thread` (spawn, parallelMap, results): closures and class
//!   instances keep their identity, because both threads run the same image.
//! - Messages (`postMessage`, `workerData`, ports): Node's structured clone.
//!   A function is an error, a class instance becomes a plain object.
//!
//! Message walks root source identities, so getters, lazy-array materialization
//! and Error stack formatting can allocate without invalidating later edges.
//! Thread captures retain their callback-free walk and deferred materialization.

use super::{CloneMessage, CloneNode, CloneShape, CloneSlot};
use crate::fast_hash::PtrHashMap;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::descriptors::{OwnNamesSnapshot, OwnProperty};

use super::{
    fs_thread_codec, unsupported_transfer_type_name, SerializedValue, VIEW_KIND_DATA_VIEW,
};
use super::{BIGINT_TAG, INT32_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
use super::{TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};
use crate::bigint::{self, BigIntHeader, BIGINT_LIMBS};
use crate::closure::{real_capture_count, ClosureHeader};
use crate::gc;
use crate::value::{JSValue, TAG_HOLE};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloneMode {
    /// `perry/thread` captures and results.
    Thread,
    /// `postMessage` and friends: Node's structured clone.
    Message,
}

/// Serialize a NaN-boxed JSValue for `perry/thread`. A value that cannot
/// cross becomes [`SerializedValue::Unsupported`]; the caller turns that into
/// a TypeError (see `guard_transferable`).
///
/// # Safety
/// `bits` must be a valid NaN-boxed JSValue of the current thread.
#[optimize(size)]
pub unsafe fn serialize_nanbox_for_thread(bits: u64) -> SerializedValue {
    write_value(CloneMode::Thread, bits, None, false, &[])
}

/// Serialize one closure capture slot for `perry/thread` (see
/// `Writer::capture`).
///
/// # Safety
/// `slot_bits` must be a capture slot of a live closure of this thread.
#[optimize(size)]
pub(super) unsafe fn serialize_capture_for_thread(slot_bits: u64) -> SerializedValue {
    write_value(CloneMode::Thread, slot_bits, None, true, &[])
}

/// Clone a value for `postMessage` with Node's structured-clone rules.
///
/// `transfer` holds the ArrayBuffers of the transfer list. They are checked
/// first, and detached only after the whole value was cloned, so a failed
/// clone leaves every buffer as it was. A transferred buffer moves its native
/// backing into the message after the walk succeeds. `uncloneable` names objects the host
/// refuses (MessagePorts, objects marked with `markAsUncloneable`).
///
/// On failure returns the `DataCloneError` message.
///
/// # Safety
/// `bits` must be a valid NaN-boxed JSValue of the current thread, and every
/// entry of `transfer` the address of a live buffer of this thread.
#[optimize(size)]
pub unsafe fn serialize_message(
    bits: u64,
    transfer: &[usize],
    uncloneable: Option<&dyn Fn(u64) -> bool>,
) -> Result<SerializedValue, String> {
    #[cfg(perry_gc_instruments)]
    let _timer = super::clone_timing::CloneTimer::new("serialize");
    for (index, &buffer) in transfer.iter().enumerate() {
        if !crate::buffer::is_registered_buffer(buffer)
            || !crate::buffer::is_array_buffer(buffer)
            || crate::shared_sab::is_shared_sab(buffer)
        {
            return Err("Found invalid value in transferList.".to_string());
        }
        if crate::buffer::is_detached_buffer(buffer) {
            return Err("An ArrayBuffer is detached and could not be cloned.".to_string());
        }
        if transfer[..index].contains(&buffer) {
            return Err("Transfer list contains duplicate ArrayBuffer".to_string());
        }
    }
    let scope = gc::RuntimeHandleScope::new();
    let _roots: Vec<_> = transfer
        .iter()
        .map(|&addr| scope.root_raw_mut_ptr(addr as *mut u8))
        .collect();
    let backings: Vec<_> = transfer
        .iter()
        .map(|&addr| crate::buffer::view::backing_of(addr))
        .collect();
    let outcome = crate::exception::catch_js_throw(|| {
        write_value(CloneMode::Message, bits, uncloneable, false, &backings)
    });
    let mut value = match outcome {
        Ok(value) => value,
        Err(error) => {
            drop(backings);
            drop(_roots);
            drop(scope);
            crate::exception::js_throw(error);
        }
    };
    if let Some(name) = super::first_unsupported_transfer_type(&value) {
        return Err(format!("{name} could not be cloned."));
    }
    commit_transfers(&mut value);
    for &buffer in transfer {
        crate::buffer::detach_array_buffer(buffer);
    }
    Ok(value)
}

/// Commit only after unsupported-value validation. There are no GC allocations
/// in this pass, and the transfer roots cover buffers absent from the message.
#[optimize(size)]
unsafe fn commit_transfers(value: &mut SerializedValue) {
    match value {
        SerializedValue::Message { root, data } => {
            commit_transfers(root);
            for node in &mut std::sync::Arc::get_mut(data)
                .expect("unpublished message is uniquely owned")
                .nodes
            {
                if let CloneNode::Value(value) = node {
                    commit_transfers(value);
                }
            }
        }
        SerializedValue::TransferredArrayBuffer(backing) => backing.commit(),
        SerializedValue::Array(values)
        | SerializedValue::Set(values)
        | SerializedValue::ScopeCapture(values) => {
            for value in values {
                commit_transfers(value);
            }
        }
        SerializedValue::Object { fields, .. } => {
            for value in fields {
                commit_transfers(value);
            }
        }
        SerializedValue::Closure { captures, .. } => {
            for value in captures {
                commit_transfers(value);
            }
        }
        SerializedValue::Map(entries) => {
            for (key, value) in entries {
                commit_transfers(key);
                commit_transfers(value);
            }
        }
        SerializedValue::View { buffer, .. } | SerializedValue::BoxedCapture(buffer) => {
            commit_transfers(buffer)
        }
        SerializedValue::Error {
            cause: Some(cause), ..
        } => commit_transfers(cause),
        _ => {}
    }
}

/// Walk `bits`, build anything the walk could not read without allocating,
/// and walk again until nothing is left to build.
#[optimize(size)]
unsafe fn write_value(
    mode: CloneMode,
    bits: u64,
    uncloneable: Option<&dyn Fn(u64) -> bool>,
    capture: bool,
    transfer: &[usize],
) -> SerializedValue {
    // A capture slot may hold a raw box pointer, not a value; such a slot
    // is rooted by its closure, which the caller holds.
    let scope = gc::RuntimeHandleScope::new();
    let root = (!capture).then(|| scope.root_nanbox_u64(bits));
    loop {
        let mut writer = Writer {
            mode,
            transfer,
            uncloneable,
            scope: &scope,
            sources: Vec::new(),
            seen: PtrHashMap::default(),
            layouts: PtrHashMap::default(),
            strings: PtrHashMap::default(),
            data: CloneMessage::default(),
            failure: None,
            next_index: 0,
            to_build: Vec::new(),
        };
        let value = match &root {
            Some(root) => writer.value(root.get_nanbox_u64()),
            None => writer.capture(bits),
        };
        match &writer.failure {
            Some(Failure::Unsupported(name)) => return SerializedValue::Unsupported(name),
            Some(Failure::Thrown(thrown)) => {
                let bits = thrown.get_nanbox_u64();
                drop(value);
                drop(writer);
                drop(scope);
                crate::exception::js_throw(f64::from_bits(bits));
            }
            None => {}
        }
        if writer.to_build.is_empty() {
            return if mode == CloneMode::Message {
                SerializedValue::Message {
                    root: Box::new(value),
                    data: std::sync::Arc::new(writer.data),
                }
            } else {
                value
            };
        }
        drop(value);
        build_pending(&writer.to_build);
    }
}

/// Values the writer met but could not read without allocating.
#[derive(Clone, Copy)]
enum Pending {
    LazyArray(u64),
    ErrorStack(u64),
}

/// Build every pending value. Each is rooted first, because building one
/// allocates and can move the others.
#[optimize(size)]
unsafe fn build_pending(pending: &[Pending]) {
    let scope = gc::RuntimeHandleScope::new();
    let handles: Vec<_> = pending
        .iter()
        .map(|item| match *item {
            Pending::LazyArray(bits) | Pending::ErrorStack(bits) => scope.root_nanbox_u64(bits),
        })
        .collect();
    for (item, handle) in pending.iter().zip(&handles) {
        let addr = (handle.get_nanbox_u64() & POINTER_MASK) as usize;
        match item {
            Pending::LazyArray(_) => {
                crate::json_tape::force_materialize_lazy(
                    addr as *mut crate::json_tape::LazyArrayHeader,
                );
            }
            Pending::ErrorStack(_) => {
                crate::error::js_error_get_stack(addr as *mut crate::error::ErrorHeader);
            }
        }
    }
}

enum Failure<'a> {
    Thrown(RuntimeHandle<'a>),
    Unsupported(&'static str),
}

struct Writer<'a> {
    scope: &'a RuntimeHandleScope,
    sources: Vec<Option<(RuntimeHandle<'a>, usize)>>,
    layouts: PtrHashMap<(usize, u32), (u32, RuntimeHandle<'a>)>,
    strings: PtrHashMap<usize, (u32, RuntimeHandle<'a>)>,
    data: CloneMessage,
    failure: Option<Failure<'a>>,
    mode: CloneMode,
    transfer: &'a [usize],
    uncloneable: Option<&'a dyn Fn(u64) -> bool>,
    /// Identity key of every object written so far → its index (the order
    /// the reader creates objects in). Buffers backing a view use
    /// `addr | 1` so a Uint8Array and the bytes it owns get separate keys.
    seen: PtrHashMap<usize, u32>,
    next_index: u32,
    to_build: Vec<Pending>,
}

impl Writer<'_> {
    fn unsupported(&mut self, name: &'static str) -> SerializedValue {
        if self.mode == CloneMode::Message && self.failure.is_none() {
            self.failure = Some(Failure::Unsupported(name));
        }
        SerializedValue::Unsupported(name)
    }

    /// Give the next object its index, or return a `Ref` when `key` was
    /// written before. Must be called once for every value of a kind the
    /// reader remembers (see `clone_read::remembers`), before its children.
    #[inline(never)]
    #[optimize(size)]
    fn begin(&mut self, key: Option<usize>) -> Result<(), SerializedValue> {
        if let Some(key) = key {
            if let Some(&index) = self.seen.get(&key) {
                return Err(SerializedValue::Ref(index));
            }
            self.seen.insert(key, self.next_index);
        }
        if self.mode == CloneMode::Message {
            self.sources
                .push(key.map(|key| (self.scope.root_raw_mut_ptr((key & !1) as *mut u8), key & 1)));
        }
        self.next_index += 1;
        Ok(())
    }

    #[inline(never)]
    #[optimize(size)]
    fn source_address(&self, index: usize, fallback: usize) -> usize {
        self.sources
            .get(index)
            .and_then(Option::as_ref)
            .map_or(fallback, |(h, _)| h.get_raw_const_ptr::<u8>() as usize)
    }

    #[optimize(size)]
    fn source<T>(&self, index: usize, fallback: *const T) -> *const T {
        self.source_address(index, fallback as usize) as *const T
    }

    /// A lazy JSON array keeps the wrapper's identity, while its elements
    /// belong to the materialized array rooted by that wrapper.
    #[optimize(size)]
    unsafe fn array_source(
        &self,
        index: usize,
        fallback: *const crate::array::ArrayHeader,
    ) -> *const crate::array::ArrayHeader {
        let source = self.source(index, fallback);
        if crate::value::addr_class::try_read_gc_header(source as usize)
            .is_some_and(|header| header.obj_type == gc::GC_TYPE_LAZY_ARRAY)
        {
            (*(source as *const crate::json_tape::LazyArrayHeader)).materialized
        } else {
            source
        }
    }

    #[optimize(size)]
    fn refresh_identities(&mut self) {
        self.seen.clear();
        for (index, source) in self.sources.iter().enumerate() {
            if let Some((handle, low)) = source {
                self.seen.insert(
                    crate::value::resolve_forwarding(handle.get_raw_const_ptr::<u8>() as usize)
                        | low,
                    index as u32,
                );
            }
        }
        self.layouts = std::mem::take(&mut self.layouts)
            .into_iter()
            .map(|((_, count), (index, root))| {
                (
                    (root.get_raw_const_ptr::<u8>() as usize, count),
                    (index, root),
                )
            })
            .collect();
        self.strings = std::mem::take(&mut self.strings)
            .into_values()
            .map(|(index, handle)| (handle.get_raw_const_ptr::<u8>() as usize, (index, handle)))
            .collect();
    }

    #[optimize(size)]
    unsafe fn slot(&mut self, bits: u64) -> CloneSlot {
        let tag = bits & TAG_MASK;
        if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
            return CloneSlot::inline(bits);
        }
        match self.value(bits) {
            SerializedValue::Inline(bits) => CloneSlot::inline(bits),
            SerializedValue::StringRef(index) => CloneSlot::string(index),
            SerializedValue::Hole => CloneSlot::HOLE,
            node => {
                let index = self.data.nodes.len() as u32;
                self.data.nodes.push(match node {
                    SerializedValue::ShapeObject { shape, start } => {
                        CloneNode::Object { shape, start }
                    }
                    SerializedValue::SlotArray {
                        start,
                        len,
                        properties,
                    } => CloneNode::Array {
                        start,
                        len,
                        properties,
                    },
                    SerializedValue::Ref(index) => CloneNode::Ref(index),
                    value => CloneNode::Value(Box::new(value)),
                });
                CloneSlot::node(index)
            }
        }
    }

    #[optimize(size)]
    fn reserve_slots(&mut self, count: usize) -> usize {
        let start = self.data.slots.len();
        self.data.slots.resize(start + count, CloneSlot::HOLE);
        start
    }

    #[optimize(size)]
    unsafe fn value(&mut self, bits: u64) -> SerializedValue {
        if self.failure.is_some() {
            return SerializedValue::Inline(TAG_UNDEFINED);
        }
        match bits {
            TAG_UNDEFINED | TAG_NULL | TAG_TRUE | TAG_FALSE => {
                return SerializedValue::Inline(bits)
            }
            _ => {}
        }
        let tag = bits & TAG_MASK;
        if tag == INT32_TAG {
            return SerializedValue::Inline(bits);
        }
        if tag == STRING_TAG {
            let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
            if !crate::value::addr_class::is_above_handle_band(ptr as usize) {
                return SerializedValue::String(Vec::new());
            }
            if self.mode == CloneMode::Message {
                let addr = ptr as usize;
                let index = if let Some((index, _)) = self.strings.get(&addr) {
                    *index
                } else {
                    let index = self.data.strings.len() as u32;
                    self.data.strings.push(string_bytes(ptr));
                    self.strings
                        .insert(addr, (index, self.scope.root_raw_const_ptr(ptr)));
                    index
                };
                return SerializedValue::StringRef(index);
            }
            return SerializedValue::String(string_bytes(ptr));
        }
        if tag == BIGINT_TAG {
            let ptr = bigint::clean_bigint_ptr((bits & POINTER_MASK) as *const BigIntHeader);
            if ptr.is_null() {
                return SerializedValue::BigInt(vec![0u64; BIGINT_LIMBS]);
            }
            return SerializedValue::BigInt(crate::bigint::BigIntHeader::all_limbs(ptr).to_vec());
        }
        if tag == crate::value::HANDLE_TAG {
            return self.unsupported("native handle");
        }
        if tag == POINTER_TAG {
            return self.pointer(bits);
        }
        // Numeric transfer ingress preserves the NaN-box representation contract.
        // Short strings and other tagged values retain their representation.
        let value = crate::value::JSValue::from_bits(bits);
        let bits = if value.is_number() {
            crate::value::JSValue::number(f64::from_bits(bits)).bits()
        } else {
            bits
        };
        SerializedValue::Inline(bits)
    }

    #[optimize(size)]
    unsafe fn pointer(&mut self, bits: u64) -> SerializedValue {
        let addr = (bits & POINTER_MASK) as usize;
        if addr == 0 {
            return SerializedValue::Inline(TAG_UNDEFINED);
        }
        // A handle id (fetch, zlib, Proxy, …) is not heap memory.
        if crate::value::addr_class::is_small_handle(addr) {
            return self.unsupported("native handle");
        }
        if let Some(refuse) = self.uncloneable {
            if refuse(bits) {
                return self.unsupported("object");
            }
        }
        // SharedArrayBuffer storage is process-global and never freed, so it
        // crosses by address and both threads see the same bytes (#4913).
        if let Some(addr) = crate::shared_sab::shared_store_owner(addr) {
            return SerializedValue::SharedArrayBuffer { addr };
        }
        if crate::buffer::is_registered_buffer(addr) || crate::buffer::is_uint8array_buffer(addr) {
            return self.buffer(addr);
        }
        if let Some(kind) = crate::typedarray_hook_slots::lookup_typed_array_kind(addr) {
            return self.typed_array(addr, kind);
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return self.unsupported("value of an unsupported type");
        };
        if header.gc_flags & gc::GC_FLAG_FORWARDED != 0 {
            let live = crate::value::resolve_forwarding(addr);
            return self.pointer(POINTER_TAG | live as u64);
        }
        match header.obj_type {
            gc::GC_TYPE_ARRAY => self.array(addr, addr),
            gc::GC_TYPE_LAZY_ARRAY => {
                let lazy = addr as *const crate::json_tape::LazyArrayHeader;
                if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
                    return self.unsupported("value of an unsupported type");
                }
                let built = (*lazy).materialized;
                if built.is_null() && self.mode == CloneMode::Message {
                    let lazy = self.scope.root_nanbox_u64(bits);
                    crate::json_tape::force_materialize_lazy(
                        (lazy.get_nanbox_u64() & POINTER_MASK)
                            as *mut crate::json_tape::LazyArrayHeader,
                    );
                    self.refresh_identities();
                    let addr = (lazy.get_nanbox_u64() & POINTER_MASK) as usize;
                    return self.array(
                        addr,
                        (*(addr as *const crate::json_tape::LazyArrayHeader)).materialized as usize,
                    );
                }
                if built.is_null() {
                    self.to_build.push(Pending::LazyArray(bits));
                    return SerializedValue::Inline(TAG_UNDEFINED);
                }
                self.array(addr, built as usize)
            }
            gc::GC_TYPE_OBJECT => {
                if crate::regex::regexp_data_of(f64::from_bits(bits)).is_some() {
                    self.regexp(addr)
                } else {
                    self.object_value(bits, addr)
                }
            }
            gc::GC_TYPE_CLOSURE => match self.mode {
                CloneMode::Thread => self.closure(addr),
                CloneMode::Message => self.unsupported("function"),
            },
            gc::GC_TYPE_DATE_CELL => {
                if let Err(seen) = self.begin(Some(addr)) {
                    return seen;
                }
                // #2089: the reader allocates a fresh cell.
                SerializedValue::Date((*(addr as *const crate::date::DateCell)).ts)
            }
            gc::GC_TYPE_MAP if crate::map::is_registered_map(addr) => self.map(addr),
            gc::GC_TYPE_SET if crate::set::is_registered_set(addr) => self.set(addr),
            gc::GC_TYPE_ERROR => self.error(bits, addr),
            other => self.unsupported(unsupported_transfer_type_name(other)),
        }
    }

    // Customized writable/configurable attributes do not change clone values.
    // Prove the array's own indexed layout from its existing descriptor holder,
    // once for this array; named enumerable properties and index accessors
    // retain the descriptor walk.
    #[optimize(size)]
    unsafe fn data_array_layout(&self, arr: *const crate::array::ArrayHeader) -> bool {
        let holder = crate::array::array_property_bag(arr);
        if holder.is_null() {
            return !crate::array::array_has_named_properties_resolved(arr);
        }
        let keys = crate::object::object_keys(holder);
        let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..keys.count() {
            let entry = crate::object::key_attrs::keys_entry(keys.arr(), i);
            let Some(name) = crate::string::js_string_key_bytes(keys.get(i), &mut short) else {
                continue;
            };
            let index = std::str::from_utf8(name)
                .ok()
                .and_then(crate::object::canonical_array_index);
            if let Some(index) = index {
                if entry
                    & (crate::object::key_attrs::ENTRY_NON_ENUMERABLE
                        | crate::object::key_attrs::ENTRY_ACCESSOR)
                    != 0
                    || index >= (*arr).length
                    || index >= (*arr).capacity
                    || *(crate::array::array_elements_ptr(arr) as *const u64).add(index as usize)
                        == TAG_HOLE
                {
                    return false;
                }
            } else if entry & crate::object::key_attrs::ENTRY_NON_ENUMERABLE == 0 {
                return false;
            }
        }
        true
    }

    #[optimize(size)]
    unsafe fn array_descriptor_stamp(arr: *const crate::array::ArrayHeader) -> u32 {
        let holder = crate::array::array_property_bag(arr);
        if holder.is_null() {
            0
        } else {
            crate::object::shapes::object_shape_stamp(holder).0
        }
    }

    /// Elements are read through `js_array_get_f64` (it resolves far slots
    /// of a sparse array and every storage layout); the raw slot is only
    /// asked whether it is a hole. `key` is the identity user code saw: the
    /// lazy header for a built `JSON.parse` array.
    #[optimize(size)]
    unsafe fn array(&mut self, key: usize, addr: usize) -> SerializedValue {
        // #6518: follow a forwarding stub left by growth.
        let arr = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader);
        let key = if key == addr { arr as usize } else { key };
        if let Err(seen) = self.begin(Some(key)) {
            return seen;
        }
        if arr.is_null() {
            return SerializedValue::Array(Vec::new());
        }
        let index = self.next_index as usize - 1;
        let len = (*arr).length as usize;
        let capacity = (*arr).capacity as usize;
        if self.mode == CloneMode::Message {
            let data_layout = self.data_array_layout(arr);
            if data_layout
                && crate::array::array_numeric_layout_resolved(arr)
                    == Some(crate::array::NumericArrayLayout::RawF64)
            {
                return SerializedValue::RawArray(
                    std::slice::from_raw_parts(
                        crate::array::array_elements_ptr(arr) as *const u64,
                        len,
                    )
                    .to_vec(),
                );
            }
            if data_layout && len <= capacity {
                let descriptor_stamp = Self::array_descriptor_stamp(arr);
                let start = self.reserve_slots(len);
                // Snapshot initial own presence; nested getters may mutate later indices.
                for i in 0..len {
                    self.data.slots[start + i] = CloneSlot::inline(
                        if *(crate::array::array_elements_ptr(arr) as *const u64).add(i) == TAG_HOLE
                        {
                            TAG_HOLE
                        } else {
                            TAG_UNDEFINED
                        },
                    );
                }
                for i in 0..len {
                    if self.data.slots[start + i].is_hole() {
                        continue;
                    }
                    let arr = crate::array::clean_arr_ptr(self.array_source(index, arr));
                    let bits = if Self::array_descriptor_stamp(arr) != descriptor_stamp {
                        let name = i.to_string();
                        let Some(bits) = self.own_value(
                            JSValue::pointer(arr.cast()).bits(),
                            name.as_bytes(),
                            None,
                        ) else {
                            self.data.slots[start + i] = CloneSlot::HOLE;
                            if self.failure.is_some() {
                                break;
                            }
                            continue;
                        };
                        bits
                    } else if i < (*arr).capacity as usize {
                        *(crate::array::array_elements_ptr(arr) as *const u64).add(i)
                    } else {
                        TAG_HOLE
                    };
                    self.data.slots[start + i] = self.slot(bits);
                }
                return SerializedValue::SlotArray {
                    start: start as u32,
                    len: len as u32,
                    properties: None,
                };
            }
            return self.descriptor_array(index, arr, len);
        }
        let mut elements = Vec::with_capacity(len);
        for i in 0..len {
            if i < capacity
                && *(crate::array::array_elements_ptr(arr) as *const u64).add(i) == TAG_HOLE
            {
                elements.push(SerializedValue::Hole);
            } else {
                elements.push(self.value(crate::array::js_array_get_f64(arr, i as u32).to_bits()));
            }
        }
        SerializedValue::Array(elements)
    }

    #[cold]
    #[optimize(size)]
    unsafe fn descriptor_array(
        &mut self,
        index: usize,
        arr: *const crate::array::ArrayHeader,
        len: usize,
    ) -> SerializedValue {
        let owner = self
            .scope
            .root_nanbox_u64(JSValue::pointer(arr.cast()).bits());
        let snapshot = OwnNamesSnapshot::collect(self.scope, &owner);
        let mut names = self.snapshot_candidates(&snapshot, &owner);
        self.refresh_identities();
        // The public array snapshot can name an index from both dense and
        // descriptor storage. Keep one visit in numeric own-key order.
        let key_index = |entry: &(Vec<u8>, u32)| {
            std::str::from_utf8(&entry.0)
                .ok()
                .and_then(crate::object::canonical_array_index)
        };
        crate::cold_sort::sort_by(&mut names, |a, b| match (key_index(a), key_index(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        names.dedup_by(|a, b| a.0 == b.0);
        let start = self.reserve_slots(len);
        let mut keys = Vec::new();
        let mut fields = Vec::new();
        for (name, position) in names {
            let arr = crate::array::clean_arr_ptr(self.array_source(index, arr));
            let Some(bits) = self.own_value(
                JSValue::pointer(arr.cast()).bits(),
                &name,
                Some((&snapshot, position)),
            ) else {
                if self.failure.is_some() {
                    break;
                }
                continue;
            };
            let value = self.slot(bits);
            if let Some(i) = std::str::from_utf8(&name)
                .ok()
                .and_then(crate::object::canonical_array_index)
            {
                if (i as usize) < len {
                    self.data.slots[start + i as usize] = value;
                }
            } else {
                keys.push(name);
                fields.push(value);
            }
        }
        let properties = if keys.is_empty() {
            None
        } else {
            let SerializedValue::ShapeObject { shape, start } = self.fields_record(keys, fields)
            else {
                unreachable!()
            };
            Some((shape, start))
        };
        SerializedValue::SlotArray {
            start: start as u32,
            len: len as u32,
            properties,
        }
    }

    #[optimize(size)]
    unsafe fn object_value(&mut self, bits: u64, addr: usize) -> SerializedValue {
        let value = f64::from_bits(bits);
        if fs_thread_codec().is_some_and(|codec| (codec.is_filehandle)(value)) {
            return match self.mode {
                CloneMode::Thread => SerializedValue::DetachedFileHandle,
                CloneMode::Message => self.unsupported("FileHandle"),
            };
        }
        // #340/#341: native-backed builtins (TextEncoder, Timeout, the
        // `perry/tui` handles, …) are ordinary objects whose state lives
        // outside the heap; a copy would be an empty `{}` (#6185).
        let obj = addr as *const crate::object::ObjectHeader;
        let class_id = (*obj).class_id;
        if let Some(name) =
            crate::event_target_hook_slots::native_class_name(class_id).or_else(|| {
                crate::event_target_hook_slots::state_transfer_family(
                    addr as *mut crate::object::ObjectHeader,
                )
            })
        {
            return self.unsupported(name);
        }
        if crate::native_class_ids::is_native_backed_class_id(class_id) {
            return self.unsupported("native handle");
        }
        let meta = (*obj).meta;
        if !meta.is_null() && crate::native_payload::is_payload_state_word((*meta).native_state) {
            return self.unsupported("native handle");
        }
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        if class_id != 0 && self.mode == CloneMode::Thread {
            return self.class_instance(obj, class_id);
        }
        self.plain_object(obj)
    }

    /// Own enumerable string-keyed properties, in key order. Values are read
    /// per key, so fields stored past the inline slots (an object built by
    /// spread or `Object.assign`) keep their values. A class instance sent
    /// as a message becomes a plain object, as in Node.
    #[optimize(size)]
    unsafe fn plain_object(&mut self, obj: *const crate::object::ObjectHeader) -> SerializedValue {
        if self.mode == CloneMode::Message {
            return self.message_object(obj);
        }
        let keys_view = crate::object::object_keys(obj);
        let mut names = Vec::new();
        let mut fields = Vec::new();
        if !keys_view.is_null() {
            let keys = keys_view.arr();
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..keys_view.count() {
                let slots = crate::array::array_elements_ptr(keys) as *const u64;
                let key_bits = *slots.add(i as usize);
                if key_bits == TAG_HOLE
                    || crate::object::key_view::position_is_hole(obj, keys_view, i)
                {
                    continue;
                }
                let entry = crate::object::key_attrs::keys_entry(keys, i);
                if crate::object::key_attrs::entry_is_private(entry)
                    || entry & crate::object::key_attrs::ENTRY_NON_ENUMERABLE != 0
                {
                    continue;
                }
                // Symbol keys are not cloned.
                let Some(name) =
                    crate::string::js_string_key_bytes(JSValue::from_bits(key_bits), &mut short)
                        .map(<[u8]>::to_vec)
                else {
                    continue;
                };
                // An accessor's slot holds its getter pair, not a value. The
                // getter is user code and could allocate, so it is not run:
                // the property crosses as `undefined`.
                let field_bits = if crate::object::key_attrs::key_is_accessor_at(keys, i) {
                    TAG_UNDEFINED
                } else {
                    crate::object::js_object_get_key_field(obj, i).bits()
                };
                names.push(name);
                fields.push(self.value(field_bits));
            }
        }
        let final_constfn = match self.mode {
            CloneMode::Thread => super::constfn_transfer::snapshot(obj, Some(&names), fields.len()),
            CloneMode::Message => None,
        };
        SerializedValue::Object {
            final_constfn,
            class_id: 0,
            parent_class_id: 0,
            fields,
            keys: Some(names),
        }
    }

    // Ordinary facts come from the shared rooted own-name snapshot. Only
    // exotic public descriptors and getters can allocate during a read.
    #[cold]
    #[optimize(size)]
    unsafe fn own_value(
        &mut self,
        receiver: u64,
        name: &[u8],
        snapshot: Option<(&OwnNamesSnapshot<'_>, u32)>,
    ) -> Option<u64> {
        if self.failure.is_some() {
            return None;
        }
        let owner = self.scope.root_nanbox_u64(receiver);
        let property = if let Some((snapshot, position)) = snapshot {
            snapshot
                .property(&owner, position)
                .map(|(_, property)| property)
        } else {
            // A shape invalidated by a nested getter retains its original
            // message-key order. Resolve those keys through the same authority.
            let key = self.scope.root_nanbox_u64(
                JSValue::string_ptr(crate::string::js_string_from_bytes(
                    name.as_ptr(),
                    name.len() as u32,
                ))
                .bits(),
            );
            self.refresh_identities();
            crate::object::descriptors::own_property(owner.get_nanbox_f64(), key.get_nanbox_f64())
        }?;
        if matches!(property, OwnProperty::Public(_)) {
            self.refresh_identities();
        }
        let (value, flags) = property.value_and_flags();
        if flags & crate::object::key_attrs::ENTRY_NON_ENUMERABLE != 0 {
            return None;
        }
        if flags & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
            return self.getter(value, owner.get_nanbox_u64());
        }
        Some(value)
    }

    #[cold]
    #[optimize(size)]
    unsafe fn snapshot_candidates(
        &mut self,
        snapshot: &OwnNamesSnapshot<'_>,
        owner: &RuntimeHandle<'_>,
    ) -> Vec<(Vec<u8>, u32)> {
        let mut candidates = Vec::with_capacity(snapshot.count() as usize);
        let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..snapshot.count() {
            let Some((_, property)) = snapshot.property(owner, i) else {
                continue;
            };
            let (_, flags) = property.value_and_flags();
            if flags & crate::object::key_attrs::ENTRY_NON_ENUMERABLE != 0 {
                continue;
            }
            if let Some(name) = crate::string::js_string_key_bytes(
                JSValue::from_bits(snapshot.key(i).to_bits()),
                &mut short,
            ) {
                candidates.push((name.to_vec(), i));
            }
        }
        candidates
    }

    #[cold]
    #[optimize(size)]
    unsafe fn enumerable_keys(&mut self, receiver: u64) -> Vec<Vec<u8>> {
        let owner = self.scope.root_nanbox_u64(receiver);
        let snapshot = OwnNamesSnapshot::collect(self.scope, &owner);
        self.refresh_identities();
        let mut names = Vec::with_capacity(snapshot.count() as usize);
        let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..snapshot.count() {
            let key = snapshot.key(i).to_bits();
            if let Some(name) =
                crate::string::js_string_key_bytes(JSValue::from_bits(key), &mut short)
            {
                names.push(name.to_vec());
            }
        }
        names
    }

    #[optimize(size)]
    fn fields_record(&mut self, keys: Vec<Vec<u8>>, fields: Vec<CloneSlot>) -> SerializedValue {
        let shape = self.data.shapes.len() as u32;
        self.data.shapes.push(CloneShape {
            keys,
            positions: Vec::new(),
        });
        let start = self.data.slots.len() as u32;
        self.data.slots.extend(fields);
        SerializedValue::ShapeObject { shape, start }
    }

    #[cold]
    #[optimize(size)]
    unsafe fn descriptor_fields(
        &mut self,
        index: usize,
        obj: *const crate::object::ObjectHeader,
        candidates: Vec<(Vec<u8>, u32)>,
        snapshot: Option<&OwnNamesSnapshot<'_>>,
        mut names: Vec<Vec<u8>>,
        mut fields: Vec<CloneSlot>,
    ) -> SerializedValue {
        for (name, position) in candidates {
            let obj = self.source(index, obj);
            let Some(bits) = self.own_value(
                JSValue::pointer(obj.cast()).bits(),
                &name,
                snapshot.map(|s| (s, position)),
            ) else {
                if self.failure.is_some() {
                    break;
                }
                continue;
            };
            names.push(name);
            fields.push(self.slot(bits));
        }
        self.fields_record(names, fields)
    }

    #[optimize(size)]
    unsafe fn getter(&mut self, get: u64, receiver: u64) -> Option<u64> {
        if self.failure.is_some() {
            return None;
        }
        if get == 0 || get == TAG_UNDEFINED {
            return Some(TAG_UNDEFINED);
        }
        let result = crate::exception::catch_js_throw(|| {
            crate::object::invoke_accessor_getter(get, f64::from_bits(receiver)).bits()
        });
        self.refresh_identities();
        match result {
            Ok(bits) => Some(bits),
            Err(error) => {
                self.failure = Some(Failure::Thrown(self.scope.root_nanbox_f64(error)));
                None
            }
        }
    }

    #[optimize(size)]
    unsafe fn message_object(
        &mut self,
        obj: *const crate::object::ObjectHeader,
    ) -> SerializedValue {
        let index = self.next_index as usize - 1;
        let id = crate::object::shapes::object_shape_stamp(obj);
        let descriptor = crate::object::shapes::shape_descriptor_for_word(id);
        let eligible = descriptor.is_some_and(|d| {
            (d.object_kind.is_ordinary_layout()
                || d.object_kind == crate::object::shapes::ShapeObjectKind::Class)
                && crate::object::shapes::is_site_matchable_layout_word(id.0)
                && d.hole_count == 0
                && d.brands().is_empty()
                && d.record_ref()
                    .is_some_and(|record| record.proves_no_symbols())
                && d.summary
                    & (crate::object::key_attrs::SUMMARY_ACCESSOR
                        | crate::object::key_attrs::SUMMARY_NON_ENUMERABLE
                        | crate::object::key_attrs::SUMMARY_PRIVATE
                        | crate::object::key_attrs::SUMMARY_HOLES)
                    == 0
        });
        if eligible {
            let keys = descriptor.unwrap().keys_view();
            let key = (keys.arr() as usize, keys.count());
            let shape = if let Some((shape, _)) = self.layouts.get(&key) {
                *shape
            } else {
                // Own-key ordering is the runtime's authority, once per layout.
                let names = self.enumerable_keys(JSValue::pointer(obj.cast()).bits());
                let obj = self.source(index, obj);
                let keys = crate::object::object_keys(obj);
                let positions = names
                    .iter()
                    .map(|name| {
                        let position = crate::object::keys_find_property_slot_by_bytes(
                            keys,
                            keys.count(),
                            name,
                        )
                        .expect("shape key is own");
                        // The value's storage slot under this layout.
                        keys.storage_slot(position)
                    })
                    .collect();
                let shape = self.data.shapes.len() as u32;
                self.data.shapes.push(CloneShape {
                    keys: names,
                    positions,
                });
                let key = (keys.arr() as usize, keys.count());
                self.layouts
                    .insert(key, (shape, self.scope.root_raw_mut_ptr(keys.arr())));
                shape
            };
            let count = self.data.shapes[shape as usize].keys.len();
            let start = self.reserve_slots(count);
            let live = descriptor.unwrap().live_inline_slot_count;
            for i in 0..count {
                let obj = self.source(index, obj);
                if crate::object::shapes::object_shape_stamp(obj) != id {
                    let names = self.data.shapes[shape as usize].keys[..i].to_vec();
                    let candidates = self.data.shapes[shape as usize].keys[i..]
                        .iter()
                        .cloned()
                        .map(|name| (name, 0))
                        .collect();
                    let fields = self.data.slots[start..start + i].to_vec();
                    return self.descriptor_fields(index, obj, candidates, None, names, fields);
                }
                let position = self.data.shapes[shape as usize].positions[i];
                let bits = crate::object::object_field_at_with_live(obj, position, live).bits();
                self.data.slots[start + i] = self.slot(bits);
            }
            return SerializedValue::ShapeObject {
                shape,
                start: start as u32,
            };
        }
        let owner = self
            .scope
            .root_nanbox_u64(JSValue::pointer(obj.cast()).bits());
        let snapshot = OwnNamesSnapshot::collect(self.scope, &owner);
        let candidates = self.snapshot_candidates(&snapshot, &owner);
        self.refresh_identities();
        self.descriptor_fields(
            index,
            obj,
            candidates,
            Some(&snapshot),
            Vec::new(),
            Vec::new(),
        )
    }

    /// A class instance for `perry/thread`: every slot in order plus its
    /// class id, so the other thread gets an instance of the same class.
    #[optimize(size)]
    unsafe fn class_instance(
        &mut self,
        obj: *const crate::object::ObjectHeader,
        class_id: u32,
    ) -> SerializedValue {
        // #6759 C3c: the header word may hold a ShapeId stamp; the parent
        // edge comes from the class registry.
        let parent_class_id = crate::object::get_parent_class_id(class_id).unwrap_or(0);
        let field_count = crate::object::object_live_slot_count(obj) as usize;
        let keys_view = crate::object::object_keys(obj);
        let keys_arr = keys_view.arr();
        let key_count = if keys_view.is_null() {
            0
        } else {
            keys_view.count() as usize
        };
        let key_bits = |i: usize| -> u64 {
            let slots = crate::array::array_elements_ptr(keys_arr) as *const u64;
            *slots.add(i)
        };
        // #9029: a deleted key leaves a hole; skip the key and its slot
        // together so the reader's positional pairs stay aligned.
        let fields_ptr = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        // An optional key whose slot holds the hole is absent: skip it the
        // same way, key and slot together.
        let absent = |i: usize| -> bool {
            i < key_count
                && (key_bits(i) == TAG_HOLE
                    || crate::object::key_view::position_is_hole(obj, keys_view, i as u32))
        };
        let mut fields = Vec::with_capacity(field_count);
        for i in 0..field_count {
            if absent(i) {
                continue;
            }
            let bits = if crate::object::key_attrs::key_is_accessor_at(keys_arr, i as u32) {
                TAG_UNDEFINED
            } else {
                *fields_ptr.add(keys_view.storage_slot(i as u32) as usize)
            };
            fields.push(self.value(bits));
        }
        let keys = (!keys_view.is_null()).then(|| {
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            (0..key_count)
                .filter(|&i| !absent(i))
                .map(|i| {
                    crate::string::js_string_key_bytes(JSValue::from_bits(key_bits(i)), &mut short)
                        .map_or_else(Vec::new, <[u8]>::to_vec)
                })
                .collect::<Vec<_>>()
        });
        SerializedValue::Object {
            final_constfn: super::constfn_transfer::snapshot(obj, keys.as_deref(), fields.len()),
            class_id,
            parent_class_id,
            fields,
            keys,
        }
    }

    #[optimize(size)]
    unsafe fn closure(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let closure = addr as *const ClosureHeader;
        let capture_count = (*closure).capture_count;
        let count = real_capture_count(capture_count) as usize;
        let base = (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const u64;
        let mut captures = Vec::with_capacity(count);
        for i in 0..count {
            captures.push(self.capture(*base.add(i)));
        }
        SerializedValue::Closure {
            info: (*closure).info as usize,
            capture_count,
            captures,
        }
    }

    /// A capture slot of a boxed local (every `async` body local, every
    /// mutable capture) holds a box pointer from this thread's registry. The
    /// box cannot cross, so its current value does, and the reader boxes it
    /// again (#6520). A scope object crosses as the words of its slots.
    #[optimize(size)]
    unsafe fn capture(&mut self, slot_bits: u64) -> SerializedValue {
        if let Some(words) = crate::r#box::scope::scope_slot_contents(slot_bits) {
            return SerializedValue::ScopeCapture(
                words.into_iter().map(|bits| self.value(bits)).collect(),
            );
        }
        match crate::r#box::box_slot_contents_bits(slot_bits) {
            Some(inner) => SerializedValue::BoxedCapture(Box::new(self.value(inner))),
            None => self.value(slot_bits),
        }
    }

    /// ArrayBuffer, DataView, Uint8Array or Node Buffer: all are
    /// `BufferHeader`s; a view finds its bytes through the view table.
    #[optimize(size)]
    unsafe fn buffer(&mut self, addr: usize) -> SerializedValue {
        let header = addr as *const crate::buffer::BufferHeader;
        if crate::buffer::is_detached_buffer(addr) {
            return self.unsupported("detached ArrayBuffer");
        }
        if crate::buffer::is_array_buffer(addr) {
            // An ArrayBuffer made for `buf.buffer` is a view over buf's bytes.
            return self.backing(crate::buffer::view::backing_of(addr));
        }
        let kind = if crate::buffer::is_data_view(addr) {
            VIEW_KIND_DATA_VIEW
        } else {
            crate::typedarray_hook_slots::KIND_UINT8
        };
        let length = crate::buffer::store::raw_length(header as usize);
        if self.mode == CloneMode::Thread {
            if kind == crate::typedarray_hook_slots::KIND_UINT8 {
                // Keep perry/thread's plain-bytes form: no backing copy.
                if let Err(seen) = self.begin(Some(addr)) {
                    return seen;
                }
                return SerializedValue::Uint8Array(crate::buffer::bytes::no_gc(|scope| {
                    crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(addr as i64), scope)
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default()
                }));
            }
            return self.visible_bytes_view(addr, kind, length);
        }
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let (backing, byte_offset) = match crate::buffer::view::lookup(addr) {
            Some(info) => (info.backing, info.offset),
            None => (addr, 0),
        };
        SerializedValue::View {
            kind,
            buffer: Box::new(self.backing(backing)),
            byte_offset,
            length,
        }
    }

    #[optimize(size)]
    unsafe fn typed_array(&mut self, addr: usize, kind: u8) -> SerializedValue {
        let length = crate::buffer::store::length(addr) as u32;
        let meta = crate::typedarray_view::view_meta_of(addr);
        if meta.is_some_and(|meta| crate::buffer::is_detached_buffer(meta.backing)) {
            return self.unsupported("detached ArrayBuffer");
        }
        if self.mode == CloneMode::Thread {
            let bytes = length * crate::typedarray_hook_slots::elem_size_for_kind(kind) as u32;
            return self.visible_bytes_view(addr, kind, bytes);
        }
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        // A typed array that owns its elements is its own backing store.
        let (buffer, byte_offset) = match meta {
            Some(meta) => (self.backing(meta.backing), meta.byte_offset),
            None => (self.backing(addr), 0),
        };
        SerializedValue::View {
            kind,
            buffer: Box::new(buffer),
            byte_offset,
            length,
        }
    }

    /// `perry/thread` form of a view: only the bytes it shows, in a fresh
    /// buffer of their own.
    #[optimize(size)]
    unsafe fn visible_bytes_view(
        &mut self,
        addr: usize,
        kind: u8,
        byte_len: u32,
    ) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let _ = self.begin(None);
        let elem = if kind == VIEW_KIND_DATA_VIEW {
            1
        } else {
            crate::typedarray_hook_slots::elem_size_for_kind(kind) as u32
        };
        SerializedValue::View {
            kind,
            buffer: Box::new(SerializedValue::ArrayBuffer(crate::buffer::bytes::no_gc(
                |scope| {
                    crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(addr as i64), scope)
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default()
                },
            ))),
            byte_offset: 0,
            length: byte_len / elem,
        }
    }

    /// The whole store behind a view (or an ArrayBuffer itself), written
    /// once per message however many views share it.
    #[optimize(size)]
    unsafe fn backing(&mut self, backing: usize) -> SerializedValue {
        if let Some(addr) = crate::shared_sab::shared_store_owner(backing) {
            return SerializedValue::SharedArrayBuffer { addr };
        }
        if crate::buffer::is_detached_buffer(backing) {
            return self.unsupported("detached ArrayBuffer");
        }
        if let Err(seen) = self.begin(Some(backing | 1)) {
            return seen;
        }
        if self.transfer.contains(&backing) {
            // A typed owner stores an element count; its ArrayBuffer carries bytes.
            let length = crate::buffer::store::owner_byte_length(backing) as u32;
            return SerializedValue::TransferredArrayBuffer(
                crate::buffer::TransferredBacking::pending(backing, length),
            );
        }
        let bytes = crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(backing as i64), scope)
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        });
        SerializedValue::ArrayBuffer(bytes)
    }

    /// Entries are read from the raw table; deleted entries are holes.
    #[optimize(size)]
    unsafe fn map(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let index = self.next_index as usize - 1;
        let map = addr as *const crate::map::MapHeader;
        let used = (*map).used as usize;
        let mut entries = Vec::with_capacity((*map).size as usize);
        for i in 0..used {
            let map = self.source(index, map);
            let slots = (*map).entries as *const u64;
            let key = *slots.add(2 * i);
            if key == crate::map::MAP_HOLE_KEY_BITS {
                continue;
            }
            let value = self.scope.root_nanbox_u64(*slots.add(2 * i + 1));
            let key = self.value(key);
            let value = self.value(value.get_nanbox_u64());
            entries.push((key, value));
        }
        SerializedValue::Map(entries)
    }

    #[optimize(size)]
    unsafe fn set(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let index = self.next_index as usize - 1;
        let set = addr as *const crate::set::SetHeader;
        let used = (*set).used as usize;
        let mut values = Vec::with_capacity((*set).size as usize);
        for i in 0..used {
            let set = self.source(index, set);
            let bits = *((*set).elements as *const u64).add(i);
            if bits == crate::set::SET_HOLE_VALUE_BITS {
                continue;
            }
            values.push(self.value(bits));
        }
        SerializedValue::Set(values)
    }

    #[optimize(size)]
    unsafe fn error(&mut self, bits: u64, addr: usize) -> SerializedValue {
        let err = addr as *const crate::error::ErrorHeader;
        if crate::event_target_hook_slots::is_dom_exception_error(err) {
            return self.unsupported("DOMException");
        }
        // The stack text is formatted on first read, which allocates.
        if (*err).stack.is_null() && self.mode == CloneMode::Message {
            let root = self.scope.root_nanbox_u64(bits);
            crate::error::js_error_get_stack(err as *mut crate::error::ErrorHeader);
            self.refresh_identities();
            return self.error(
                root.get_nanbox_u64(),
                (root.get_nanbox_u64() & POINTER_MASK) as usize,
            );
        }
        if (*err).stack.is_null() {
            self.to_build.push(Pending::ErrorStack(bits));
            return SerializedValue::Inline(TAG_UNDEFINED);
        }
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let flags = (*err).flags;
        let name = if (*err).name.is_null() {
            b"Error".to_vec()
        } else {
            string_bytes((*err).name)
        };
        let message = (flags & crate::error::ERROR_FLAG_HAS_MESSAGE != 0
            && !(*err).message.is_null())
        .then(|| string_bytes((*err).message));
        let stack = Some(string_bytes((*err).stack));
        let cause = (flags & crate::error::ERROR_FLAG_HAS_CAUSE != 0)
            .then(|| Box::new(self.value((*err).cause.to_bits())));
        SerializedValue::Error {
            name,
            message,
            stack,
            cause,
        }
    }

    #[optimize(size)]
    unsafe fn regexp(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let (source, flags) =
            crate::regex::regexp_source_and_flags(addr as *const crate::regex::RegExpHeader);
        SerializedValue::RegExp {
            source: source.map_or_else(Vec::new, |s| string_bytes(s)),
            flags: flags.map_or_else(Vec::new, |s| string_bytes(s)),
        }
    }
}

#[optimize(size)]
unsafe fn string_bytes(ptr: *const crate::string::StringHeader) -> Vec<u8> {
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::slice::from_raw_parts(data, len).to_vec()
}
