//! Writer half of the thread clone: a JS value becomes a [`SerializedValue`]
//! made only of owned data, so it can cross to another thread.
//!
//! Two callers with slightly different rules share it:
//! - `perry/thread` (spawn, parallelMap, results): closures and class
//!   instances keep their identity, because both threads run the same image.
//! - Messages (`postMessage`, `workerData`, ports): Node's structured clone.
//!   A function is an error, a class instance becomes a plain object.
//!
//! The writer never allocates on the JS heap, so no object can move while it
//! walks and a source address is a stable identity for the whole walk. Two
//! values can only be read by allocating: a lazy `JSON.parse` array that was
//! never built, and an Error whose `stack` text was never formatted. The
//! writer notes them, the caller builds them with the walk finished, and the
//! walk runs again.

use std::collections::HashMap;

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
pub unsafe fn serialize_nanbox_for_thread(bits: u64) -> SerializedValue {
    write_value(CloneMode::Thread, bits, None, false, &[])
}

/// Serialize one closure capture slot for `perry/thread` (see
/// `Writer::capture`).
///
/// # Safety
/// `slot_bits` must be a capture slot of a live closure of this thread.
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
pub unsafe fn serialize_message(
    bits: u64,
    transfer: &[usize],
    uncloneable: Option<&dyn Fn(u64) -> bool>,
) -> Result<SerializedValue, String> {
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
    let mut value = write_value(CloneMode::Message, bits, uncloneable, false, &backings);
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
unsafe fn commit_transfers(value: &mut SerializedValue) {
    match value {
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
            seen: HashMap::new(),
            next_index: 0,
            to_build: Vec::new(),
        };
        let value = match &root {
            Some(root) => writer.value(root.get_nanbox_u64()),
            None => writer.capture(bits),
        };
        if writer.to_build.is_empty() {
            return value;
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

struct Writer<'a> {
    mode: CloneMode,
    transfer: &'a [usize],
    uncloneable: Option<&'a dyn Fn(u64) -> bool>,
    /// Identity key of every object written so far → its index (the order
    /// the reader creates objects in). Buffers backing a view use
    /// `addr | 1` so a Uint8Array and the bytes it owns get separate keys.
    seen: HashMap<usize, u32>,
    next_index: u32,
    to_build: Vec<Pending>,
}

impl Writer<'_> {
    /// Give the next object its index, or return a `Ref` when `key` was
    /// written before. Must be called once for every value of a kind the
    /// reader remembers (see `clone_read::remembers`), before its children.
    fn begin(&mut self, key: Option<usize>) -> Result<(), SerializedValue> {
        if let Some(key) = key {
            if let Some(&index) = self.seen.get(&key) {
                return Err(SerializedValue::Ref(index));
            }
            self.seen.insert(key, self.next_index);
        }
        self.next_index += 1;
        Ok(())
    }

    unsafe fn value(&mut self, bits: u64) -> SerializedValue {
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
            if ptr.is_null() || (ptr as usize) < 0x1000 {
                return SerializedValue::String(Vec::new());
            }
            return SerializedValue::String(string_bytes(ptr));
        }
        if tag == BIGINT_TAG {
            let ptr = bigint::clean_bigint_ptr((bits & POINTER_MASK) as *const BigIntHeader);
            if ptr.is_null() {
                return SerializedValue::BigInt([0u64; BIGINT_LIMBS]);
            }
            return SerializedValue::BigInt((*ptr).limbs);
        }
        if tag == POINTER_TAG {
            return self.pointer(bits);
        }
        // Numbers, and short strings stored inside the NaN box.
        SerializedValue::Inline(bits)
    }

    unsafe fn pointer(&mut self, bits: u64) -> SerializedValue {
        let addr = (bits & POINTER_MASK) as usize;
        if addr < 0x1000 {
            return SerializedValue::Inline(TAG_UNDEFINED);
        }
        // A handle id (fetch, zlib, Proxy, …) is not heap memory.
        if crate::value::addr_class::is_small_handle(addr) || addr < 0x10000 {
            return SerializedValue::Unsupported("native handle");
        }
        if let Some(refuse) = self.uncloneable {
            if refuse(bits) {
                return SerializedValue::Unsupported("object");
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
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
            return self.typed_array(addr, kind);
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return SerializedValue::Unsupported("value of an unsupported type");
        };
        match header.obj_type {
            gc::GC_TYPE_ARRAY => self.array(addr, addr),
            gc::GC_TYPE_LAZY_ARRAY => {
                let lazy = addr as *const crate::json_tape::LazyArrayHeader;
                if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
                    return SerializedValue::Unsupported("value of an unsupported type");
                }
                let built = (*lazy).materialized;
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
                CloneMode::Message => SerializedValue::Unsupported("function"),
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
            other => SerializedValue::Unsupported(unsupported_transfer_type_name(other)),
        }
    }

    /// Elements are read through `js_array_get_f64` (it resolves far slots
    /// of a sparse array and every storage layout); the raw slot is only
    /// asked whether it is a hole. `key` is the identity user code saw: the
    /// lazy header for a built `JSON.parse` array.
    unsafe fn array(&mut self, key: usize, addr: usize) -> SerializedValue {
        // #6518: follow a forwarding stub left by growth.
        let arr = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader);
        if let Err(seen) = self.begin(Some(key)) {
            return seen;
        }
        if arr.is_null() {
            return SerializedValue::Array(Vec::new());
        }
        let len = (*arr).length as usize;
        let capacity = (*arr).capacity as usize;
        let mut elements = Vec::with_capacity(len);
        for i in 0..len {
            if i < capacity {
                let slots = crate::array::array_elements_ptr(arr) as *const u64;
                if *slots.add(i) == TAG_HOLE {
                    elements.push(SerializedValue::Hole);
                    continue;
                }
            }
            let bits = crate::array::js_array_get_f64(arr, i as u32).to_bits();
            elements.push(self.value(bits));
        }
        SerializedValue::Array(elements)
    }

    unsafe fn object_value(&mut self, bits: u64, addr: usize) -> SerializedValue {
        let value = f64::from_bits(bits);
        if fs_thread_codec().is_some_and(|codec| (codec.is_filehandle)(value)) {
            return match self.mode {
                CloneMode::Thread => SerializedValue::DetachedFileHandle,
                CloneMode::Message => SerializedValue::Unsupported("FileHandle"),
            };
        }
        // #340/#341: native-backed builtins (TextEncoder, Timeout, the
        // `perry/tui` handles, …) are ordinary objects whose state lives
        // outside the heap; a copy would be an empty `{}` (#6185).
        let obj = addr as *const crate::object::ObjectHeader;
        let class_id = (*obj).class_id;
        if let Some(name) = crate::event_target::native_class_name(class_id).or_else(|| {
            crate::event_target::state::transfer_family(addr as *mut crate::object::ObjectHeader)
        }) {
            return SerializedValue::Unsupported(name);
        }
        if crate::native_class_ids::is_native_backed_class_id(class_id) {
            return SerializedValue::Unsupported("native handle");
        }
        let meta = (*obj).meta;
        if !meta.is_null() && crate::native_payload::is_payload_state_word((*meta).native_state) {
            return SerializedValue::Unsupported("native handle");
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
    unsafe fn plain_object(&mut self, obj: *const crate::object::ObjectHeader) -> SerializedValue {
        let keys_view = crate::object::object_keys(obj);
        let mut names = Vec::new();
        let mut fields = Vec::new();
        if !keys_view.is_null() {
            let keys = keys_view.arr();
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..keys_view.count() {
                let slots = crate::array::array_elements_ptr(keys) as *const u64;
                let key_bits = *slots.add(i as usize);
                if key_bits == TAG_HOLE {
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
                    crate::object::js_object_get_field(obj, i).bits()
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

    /// A class instance for `perry/thread`: every slot in order plus its
    /// class id, so the other thread gets an instance of the same class.
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
        let mut fields = Vec::with_capacity(field_count);
        for i in 0..field_count {
            if i < key_count && key_bits(i) == TAG_HOLE {
                continue;
            }
            let bits = if crate::object::key_attrs::key_is_accessor_at(keys_arr, i as u32) {
                TAG_UNDEFINED
            } else {
                *fields_ptr.add(i)
            };
            fields.push(self.value(bits));
        }
        let keys = (!keys_view.is_null()).then(|| {
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            (0..key_count)
                .filter(|&i| key_bits(i) != TAG_HOLE)
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
    unsafe fn buffer(&mut self, addr: usize) -> SerializedValue {
        let header = addr as *const crate::buffer::BufferHeader;
        if crate::buffer::is_detached_buffer(addr) {
            return SerializedValue::Unsupported("detached ArrayBuffer");
        }
        if crate::buffer::is_array_buffer(addr) {
            // An ArrayBuffer made for `buf.buffer` is a view over buf's bytes.
            return self.backing(crate::buffer::view::backing_of(addr));
        }
        let kind = if crate::buffer::is_data_view(addr) {
            VIEW_KIND_DATA_VIEW
        } else {
            crate::typedarray::KIND_UINT8
        };
        let length = (*header).length;
        if self.mode == CloneMode::Thread {
            if kind == crate::typedarray::KIND_UINT8 {
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

    unsafe fn typed_array(&mut self, addr: usize, kind: u8) -> SerializedValue {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        let length = (*ta).length;
        let meta = crate::typedarray_view::view_meta_of(addr);
        if meta.is_some_and(|meta| crate::buffer::is_detached_buffer(meta.backing)) {
            return SerializedValue::Unsupported("detached ArrayBuffer");
        }
        if self.mode == CloneMode::Thread {
            let bytes = length * crate::typedarray::elem_size_for_kind(kind) as u32;
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
            crate::typedarray::elem_size_for_kind(kind) as u32
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
    unsafe fn backing(&mut self, backing: usize) -> SerializedValue {
        if let Some(addr) = crate::shared_sab::shared_store_owner(backing) {
            return SerializedValue::SharedArrayBuffer { addr };
        }
        if crate::buffer::is_detached_buffer(backing) {
            return SerializedValue::Unsupported("detached ArrayBuffer");
        }
        if let Err(seen) = self.begin(Some(backing | 1)) {
            return seen;
        }
        if self.transfer.contains(&backing) {
            let length = crate::buffer::store::length(backing) as u32;
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
    unsafe fn map(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let map = addr as *const crate::map::MapHeader;
        let used = (*map).used as usize;
        let mut entries = Vec::with_capacity((*map).size as usize);
        for i in 0..used {
            let slots = (*map).entries as *const u64;
            let key = *slots.add(2 * i);
            if key == crate::map::MAP_HOLE_KEY_BITS {
                continue;
            }
            let value = *slots.add(2 * i + 1);
            let key = self.value(key);
            let value = self.value(value);
            entries.push((key, value));
        }
        SerializedValue::Map(entries)
    }

    unsafe fn set(&mut self, addr: usize) -> SerializedValue {
        if let Err(seen) = self.begin(Some(addr)) {
            return seen;
        }
        let set = addr as *const crate::set::SetHeader;
        let used = (*set).used as usize;
        let mut values = Vec::with_capacity((*set).size as usize);
        for i in 0..used {
            let bits = *((*set).elements as *const u64).add(i);
            if bits == crate::set::SET_HOLE_VALUE_BITS {
                continue;
            }
            values.push(self.value(bits));
        }
        SerializedValue::Set(values)
    }

    unsafe fn error(&mut self, bits: u64, addr: usize) -> SerializedValue {
        let err = addr as *const crate::error::ErrorHeader;
        if crate::event_target::is_dom_exception_error(err) {
            return SerializedValue::Unsupported("DOMException");
        }
        // The stack text is formatted on first read, which allocates.
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

unsafe fn string_bytes(ptr: *const crate::string::StringHeader) -> Vec<u8> {
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::slice::from_raw_parts(data, len).to_vec()
}
