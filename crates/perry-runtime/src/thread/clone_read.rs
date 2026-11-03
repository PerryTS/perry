//! Reader half of the thread clone: a [`SerializedValue`] becomes JS values
//! in the current thread's heap.
//!
//! Every object the reader creates is rooted in one handle scope for the
//! whole read, in creation order, because a later `Ref` may name it and any
//! allocation may move it. Only one scope is used: a nested scope would drop
//! its handles before the outer read is done.

use std::ptr;

use super::{fs_thread_codec, store_thread_array_slot, store_thread_object_field};
use super::{CloneMessage, CloneNode, CloneSlot, SerializedValue, VIEW_KIND_DATA_VIEW};
use super::{BIGINT_TAG, POINTER_MASK, TAG_UNDEFINED};
use crate::codegen_abi::LayoutWord;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{JSValue, POINTER_TAG};

/// Deserialize a SerializedValue into a NaN-boxed JSValue.
///
/// Allocates any needed objects in the **current thread's** arena: the
/// caller picks the arena by calling this on the receiving thread.
///
/// # Safety
/// `sv` must come from the writer in `clone_write.rs` (or keep its rules).
#[optimize(size)]
pub unsafe fn deserialize_nanbox_on_current_thread(sv: &SerializedValue) -> u64 {
    #[cfg(perry_gc_instruments)]
    let _timer = super::clone_timing::CloneTimer::new("deserialize");
    let scope = RuntimeHandleScope::new();
    let mut reader = Reader {
        scope: &scope,
        made: Vec::new(),
        message: None,
        shapes: Vec::new(),
        strings: Vec::new(),
    };
    reader.value(sv)
}

/// Whether the writer gave this value an index (`Writer::begin`). The reader
/// must make a slot for exactly these, in the same order.
#[optimize(size)]
fn remembers(sv: &SerializedValue) -> bool {
    matches!(
        sv,
        SerializedValue::RawArray(_)
            | SerializedValue::SlotArray { .. }
            | SerializedValue::ShapeObject { .. }
            | SerializedValue::Array(_)
            | SerializedValue::Object { .. }
            | SerializedValue::Closure { .. }
            | SerializedValue::Date(_)
            | SerializedValue::Uint8Array(_)
            | SerializedValue::ArrayBuffer(_)
            | SerializedValue::TransferredArrayBuffer(_)
            | SerializedValue::View { .. }
            | SerializedValue::Map(_)
            | SerializedValue::Set(_)
            | SerializedValue::Error { .. }
            | SerializedValue::RegExp { .. }
    )
}

/// What a slot holds. A view that owns its bytes stands in for the
/// ArrayBuffer it was cloned from: that buffer is made only if a later
/// `Ref` asks for it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Made {
    Value,
    BytesOfUint8Array,
    BytesOfTypedArray,
}

enum ArrayElements<'m> {
    Values(&'m [SerializedValue]),
    Slots(&'m [CloneSlot]),
}
impl ArrayElements<'_> {
    #[optimize(size)]
    fn len(&self) -> usize {
        match self {
            Self::Values(v) => v.len(),
            Self::Slots(v) => v.len(),
        }
    }
    #[optimize(size)]
    fn holes(&self) -> bool {
        match self {
            Self::Values(v) => v.iter().any(|e| matches!(e, SerializedValue::Hole)),
            Self::Slots(v) => v.iter().any(|e| e.is_hole()),
        }
    }
}

struct Reader<'s, 'm> {
    message: Option<&'m CloneMessage>,
    shapes: Vec<(RuntimeHandle<'s>, u32)>,
    strings: Vec<Option<RuntimeHandle<'s>>>,
    scope: &'s RuntimeHandleScope,
    made: Vec<(RuntimeHandle<'s>, Made)>,
}

impl<'s, 'm> Reader<'s, 'm> {
    #[inline(never)]
    #[optimize(size)]
    fn reserve(&mut self) -> usize {
        let handle = self.scope.root_nanbox_u64(TAG_UNDEFINED);
        self.made.push((handle, Made::Value));
        self.made.len() - 1
    }

    #[inline(never)]
    #[optimize(size)]
    fn fill(&mut self, slot: usize, bits: u64, made: Made) {
        self.made[slot].0.set_nanbox_u64(bits);
        self.made[slot].1 = made;
    }

    #[inline(never)]
    #[optimize(size)]
    fn get(&self, slot: usize) -> u64 {
        self.made[slot].0.get_nanbox_u64()
    }

    #[optimize(size)]
    fn ptr<T>(&self, slot: usize) -> *mut T {
        (self.get(slot) & POINTER_MASK) as *mut T
    }

    /// The value a `Ref` names. For a view that owns its bytes, make its
    /// ArrayBuffer now and remember it.
    #[optimize(size)]
    unsafe fn resolve(&mut self, slot: usize) -> u64 {
        let buffer = match self.made[slot].1 {
            Made::Value => return self.get(slot),
            Made::BytesOfUint8Array => {
                crate::buffer::ensure_buffer_ab_alias(self.ptr::<u8>(slot) as usize)
            }
            Made::BytesOfTypedArray => {
                crate::typedarray_view::js_typed_array_backing_buffer(self.ptr(slot)) as usize
            }
        };
        let bits = JSValue::pointer(buffer as *const u8).bits();
        self.fill(slot, bits, Made::Value);
        bits
    }

    #[optimize(size)]
    unsafe fn value(&mut self, sv: &'m SerializedValue) -> u64 {
        if let SerializedValue::Message { root, data } = sv {
            self.prepare_message(data);
            return self.value(root);
        }
        let slot = if remembers(sv) {
            Some(self.reserve())
        } else {
            None
        };
        let bits = match sv {
            SerializedValue::RawArray(bits) => JSValue::pointer(
                crate::array::js_array_from_f64(bits.as_ptr().cast(), bits.len() as u32).cast(),
            )
            .bits(),
            SerializedValue::Message { .. } => unreachable!(),
            SerializedValue::StringRef(index) => self.string(*index as usize),
            SerializedValue::ShapeObject { shape, start } => {
                return self.shape_object(slot.unwrap(), *shape as usize, *start as usize)
            }
            SerializedValue::SlotArray {
                start,
                len,
                properties,
            } => {
                return self.slot_array(slot.unwrap(), *start as usize, *len as usize, *properties)
            }
            SerializedValue::Inline(bits) => *bits,
            SerializedValue::Hole => crate::value::TAG_HOLE,
            SerializedValue::Ref(index) => self.resolve(*index as usize),
            SerializedValue::String(bytes) => string_bits(bytes),
            SerializedValue::BigInt(limbs) => {
                let ptr = crate::bigint_hooks::bigint_alloc_with_limbs(limbs);
                BIGINT_TAG | (ptr as u64 & POINTER_MASK)
            }
            // #2089: a fresh DateCell in this thread's arena.
            SerializedValue::Date(ts) => crate::date_hook_slots::alloc_date_cell(*ts).to_bits(),
            SerializedValue::Array(elements) => {
                return self.array(slot.unwrap(), ArrayElements::Values(elements))
            }
            SerializedValue::Object {
                final_constfn,
                class_id,
                parent_class_id,
                fields,
                keys,
            } => {
                return self.object(
                    slot.unwrap(),
                    final_constfn.as_ref(),
                    *class_id,
                    *parent_class_id,
                    fields,
                    keys.as_deref(),
                )
            }
            SerializedValue::Closure {
                info,
                capture_count,
                captures,
            } => return self.closure(slot.unwrap(), *info, *capture_count, captures),
            SerializedValue::BoxedCapture(inner) => {
                // The allocator roots its input; the closure's slot owns the box.
                let value = self.value(inner);
                return POINTER_TAG | crate::r#box::js_box_alloc_bits(value as i64) as u64;
            }
            SerializedValue::ScopeCapture(slots) => return self.scope_capture(slots),
            SerializedValue::Uint8Array(bytes) => owned_uint8array(bytes),
            SerializedValue::ArrayBuffer(bytes) => array_buffer(bytes),
            SerializedValue::TransferredArrayBuffer(store) => {
                let buffer = crate::buffer::buffer_adopt_backing(store.take(), store.length);

                JSValue::pointer(buffer as *const u8).bits()
            }
            SerializedValue::View {
                kind,
                buffer,
                byte_offset,
                length,
            } => return self.view(slot.unwrap(), *kind, buffer, *byte_offset, *length),
            SerializedValue::Map(entries) => return self.map(slot.unwrap(), entries),
            SerializedValue::Set(values) => return self.set(slot.unwrap(), values),
            SerializedValue::Error {
                name,
                message,
                stack,
                cause,
            } => {
                return self.error(
                    slot.unwrap(),
                    name,
                    message.as_deref(),
                    stack.as_deref(),
                    cause.as_deref(),
                )
            }
            SerializedValue::RegExp { source, flags } => self.regexp(source, flags),
            SerializedValue::DetachedFileHandle => match fs_thread_codec() {
                Some(codec) => (codec.build_detached)().to_bits(),
                // Only an armed writer makes this variant, and arming is
                // process-wide.
                None => TAG_UNDEFINED,
            },
            SerializedValue::SharedArrayBuffer { addr } => {
                // Same process-global store (#4913), no copy. Its header
                // carries the SharedArrayBuffer brand (#10694), so this
                // thread's checks recognise it without registering it.
                JSValue::pointer(crate::shared_sab::wrap_shared_sab(*addr).cast::<u8>()).bits()
            }
            // The boundary rejects these before anything is read.
            SerializedValue::Unsupported(_) => TAG_UNDEFINED,
        };
        if let Some(slot) = slot {
            self.fill(slot, bits, Made::Value);
        }
        bits
    }

    #[cold]
    #[optimize(size)]
    unsafe fn prepare_message(&mut self, data: &'m CloneMessage) {
        self.message = Some(data);
        self.strings.resize_with(data.strings.len(), || None);
        for shape in &data.shapes {
            let names = &shape.keys;
            let keys = self.keys(names);
            let seed = crate::object::alloc_plain::alloc_plain_record_with_keys(
                names.len() as u32,
                crate::object::ObjectKeys::new(keys.get_raw_mut_ptr(), names.len() as u32),
            );
            let id = crate::object::shapes::object_shape_stamp(seed);
            self.shapes.push((self.scope.root_raw_mut_ptr(seed), id.0));
        }
    }

    #[cold]
    #[optimize(size)]
    unsafe fn keys(&self, names: &[Vec<u8>]) -> RuntimeHandle<'s> {
        let keys = self
            .scope
            .root_raw_mut_ptr(crate::array::js_array_alloc(names.len() as u32));
        for (i, name) in names.iter().enumerate() {
            let bits = string_bits(name);
            store_thread_array_slot(keys.get_raw_mut_ptr(), i, bits);
        }
        keys
    }

    #[optimize(size)]
    unsafe fn string(&mut self, index: usize) -> u64 {
        if let Some(handle) = &self.strings[index] {
            return handle.get_nanbox_u64();
        }
        let bits = string_bits(&self.message.unwrap().strings[index]);
        self.strings[index] = Some(self.scope.root_nanbox_u64(bits));
        bits
    }

    #[optimize(size)]
    unsafe fn read_slot(&mut self, slot: CloneSlot) -> u64 {
        match slot.0 & super::TAG_MASK {
            super::STRING_TAG => self.string((slot.0 & super::POINTER_MASK) as usize),
            super::POINTER_TAG => {
                match &self.message.unwrap().nodes[(slot.0 & super::POINTER_MASK) as usize] {
                    CloneNode::Object { shape, start } => {
                        let slot = self.reserve();
                        self.shape_object(slot, *shape as usize, *start as usize)
                    }
                    CloneNode::Array {
                        start,
                        len,
                        properties,
                    } => {
                        let slot = self.reserve();
                        self.slot_array(slot, *start as usize, *len as usize, *properties)
                    }
                    CloneNode::Ref(index) => self.resolve(*index as usize),
                    CloneNode::Value(value) => self.value(value),
                }
            }
            _ => slot.0,
        }
    }

    #[optimize(size)]
    unsafe fn shape_object(&mut self, slot: usize, shape: usize, start: usize) -> u64 {
        let count = self.message.unwrap().shapes[shape].keys.len();
        let (seed, id) = &self.shapes[shape];
        let keys = crate::object::object_keys(seed.get_raw_const_ptr()).arr();
        let obj = crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(
            count as u32,
            keys,
            LayoutWord(*id),
        );
        self.fill(slot, JSValue::pointer(obj.cast()).bits(), Made::Value);
        for i in 0..count {
            let bits = self.read_slot(self.message.unwrap().slots[start + i]);
            store_thread_object_field(self.ptr(slot), i, bits);
        }
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn slot_array(
        &mut self,
        slot: usize,
        start: usize,
        len: usize,
        properties: Option<(u32, u32)>,
    ) -> u64 {
        let elements = &self.message.unwrap().slots[start..start + len];
        self.array(slot, ArrayElements::Slots(elements));
        if let Some((shape, start)) = properties {
            for i in 0..self.message.unwrap().shapes[shape as usize].keys.len() {
                let bits = self.read_slot(self.message.unwrap().slots[start as usize + i]);
                let value = self.scope.root_nanbox_u64(bits);
                let name = &self.message.unwrap().shapes[shape as usize].keys[i];
                let key = heap_string(name);
                let arr = crate::array::array_named_property_set(
                    self.ptr(slot),
                    key,
                    value.get_nanbox_f64(),
                );
                self.fill(slot, JSValue::pointer(arr.cast()).bits(), Made::Value);
            }
        }
        self.get(slot)
    }

    #[inline(never)]
    #[optimize(size)]
    unsafe fn array(&mut self, slot: usize, elements: ArrayElements<'m>) -> u64 {
        let len = elements.len();
        let arr = crate::array::js_array_alloc(len as u32);
        self.fill(slot, JSValue::pointer(arr.cast()).bits(), Made::Value);
        let holes = elements.holes();
        for i in 0..len {
            let bits = match &elements {
                ArrayElements::Values(values) => self.value(&values[i]),
                ArrayElements::Slots(slots) => self.read_slot(slots[i]),
            };
            if holes {
                let arr = crate::array::js_array_push_f64(self.ptr(slot), f64::from_bits(bits));
                self.fill(slot, JSValue::pointer(arr.cast()).bits(), Made::Value);
            } else {
                store_thread_array_slot(self.ptr(slot), i, bits);
            }
        }
        (*self.ptr::<crate::array::ArrayHeader>(slot)).length = len as u32;
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn object(
        &mut self,
        slot: usize,
        final_constfn: Option<&super::constfn_transfer::ConstFnTransferFacts>,
        class_id: u32,
        parent_class_id: u32,
        fields: &'m [SerializedValue],
        keys: Option<&[Vec<u8>]>,
    ) -> u64 {
        let obj = crate::object::js_object_alloc_with_parent(
            class_id,
            parent_class_id,
            fields.len() as u32,
        );
        if class_id == 0 {
            // A class-less object is data only (keys and values, like
            // `JSON.parse` output). Marked before the first stamp so its
            // layout is minted `Ordinary`, the kind a `{}` literal names.
            // SAFETY: `obj` is the unpublished newborn just allocated.
            crate::object::shapes::store_kind::premark_plain_ordinary(obj);
        }
        self.fill(slot, JSValue::pointer(obj as *const u8).bits(), Made::Value);
        for (i, field) in fields.iter().enumerate() {
            let bits = self.value(field);
            // GC_STORE_AUDIT(BARRIERED): deserialized thread object field uses the shared object slot-store helper.
            store_thread_object_field(self.ptr(slot), i, bits);
        }
        if let Some(names) = keys {
            let keys = self.keys(names);
            crate::object::js_object_set_keys(self.ptr(slot), keys.get_raw_mut_ptr());
        }
        if let (Some(facts), Some(names)) = (final_constfn, keys) {
            let obj = super::constfn_transfer::restore(
                self.ptr(slot),
                class_id,
                fields.len(),
                names,
                facts,
            );
            self.fill(slot, JSValue::pointer(obj as *const u8).bits(), Made::Value);
        }
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn closure(
        &mut self,
        slot: usize,
        info: usize,
        capture_count: u32,
        captures: &'m [SerializedValue],
    ) -> u64 {
        let closure = match crate::object::BuiltinConstructorIdentity::from_header(capture_count) {
            Some(identity) => {
                assert_eq!(info, identity.declaration().info as *const _ as usize);
                assert_eq!(crate::closure::real_capture_count(capture_count), 1);
                crate::object::alloc_builtin_constructor(identity)
            }
            None => crate::closure::js_closure_alloc(
                info as *const crate::closure::JsFunctionInfo,
                capture_count,
            ),
        };
        self.fill(
            slot,
            JSValue::pointer(closure as *const u8).bits(),
            Made::Value,
        );
        for (i, capture) in captures.iter().enumerate() {
            // Reading a capture allocates; the store does not.
            let bits = self.value(capture);
            crate::closure::js_closure_set_capture_f64(
                self.ptr(slot),
                i as u32,
                f64::from_bits(bits),
            );
        }
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn scope_capture(&mut self, slots: &'m [SerializedValue]) -> u64 {
        let base = crate::r#box::scope::js_scope_alloc(slots.len() as i32, TAG_UNDEFINED as i64)
            as usize as *mut u8;
        let rooted = self.scope.root_raw_mut_ptr(base);
        for (i, word) in slots.iter().enumerate() {
            let bits = self.value(word);
            rooted.with_mut_ptr(|base: *mut u8| {
                crate::r#box::scope::js_scope_set(base as i64, i as i32, bits as i64)
            });
        }
        rooted.with_mut_ptr(|base: *mut u8| POINTER_TAG | base as u64)
    }

    #[optimize(size)]
    unsafe fn view(
        &mut self,
        slot: usize,
        kind: u8,
        buffer: &'m SerializedValue,
        byte_offset: u32,
        length: u32,
    ) -> u64 {
        let elem = if kind == VIEW_KIND_DATA_VIEW {
            1
        } else {
            crate::typedarray_hook_slots::elem_size_for_kind(kind)
        };
        // A view over all of a buffer seen for the first time owns its
        // bytes; nothing can tell it from a view over a fresh ArrayBuffer.
        if let SerializedValue::ArrayBuffer(bytes) = buffer {
            if kind != VIEW_KIND_DATA_VIEW
                && byte_offset == 0
                && length as usize * elem == bytes.len()
            {
                let buffer_slot = self.reserve();
                let (bits, made) = if kind == crate::typedarray_hook_slots::KIND_UINT8 {
                    (owned_uint8array(bytes), Made::BytesOfUint8Array)
                } else {
                    (
                        owned_typed_array(kind, length, bytes),
                        Made::BytesOfTypedArray,
                    )
                };
                self.fill(slot, bits, Made::Value);
                self.fill(buffer_slot, bits, made);
                return bits;
            }
        }
        let buffer = f64::from_bits(self.value(buffer));
        let offset = byte_offset as f64;
        let length = length as f64;
        let bits = if kind == VIEW_KIND_DATA_VIEW {
            crate::buffer::js_data_view_new(buffer, offset, length).to_bits()
        } else if kind == crate::typedarray_hook_slots::KIND_UINT8 {
            let view = crate::buffer::js_uint8array_view(buffer, offset, length);
            JSValue::pointer(view as *const u8).bits()
        } else {
            let view = crate::typedarray_hook_slots::js_typed_array_view(
                kind as i32,
                buffer,
                offset,
                length,
            );
            JSValue::pointer(view as *const u8).bits()
        };
        self.fill(slot, bits, Made::Value);
        bits
    }

    #[cfg(feature = "regex-engine")]
    #[optimize(size)]
    unsafe fn regexp(&mut self, source: &[u8], flags: &[u8]) -> u64 {
        let source = self.scope.root_nanbox_u64(string_bits(source));
        let flags = string_bits(flags);
        let re = crate::regex::hooked::new(
            (source.get_nanbox_u64() & POINTER_MASK) as *const _,
            (flags & POINTER_MASK) as *const _,
        );
        JSValue::pointer(re as *const u8).bits()
    }

    /// Without the regex engine no RegExp can exist to be sent.
    #[cfg(not(feature = "regex-engine"))]
    #[optimize(size)]
    unsafe fn regexp(&mut self, _source: &[u8], _flags: &[u8]) -> u64 {
        TAG_UNDEFINED
    }

    #[optimize(size)]
    unsafe fn map(
        &mut self,
        slot: usize,
        entries: &'m [(SerializedValue, SerializedValue)],
    ) -> u64 {
        let map = crate::map::js_map_alloc((entries.len() as u32).max(8));
        self.fill(slot, JSValue::pointer(map as *const u8).bits(), Made::Value);
        for (key, value) in entries {
            // Reading the value allocates, so the key waits in a root.
            let key = self.value(key);
            let key = self.scope.root_nanbox_u64(key);
            let value = self.value(value);
            let map =
                crate::map::js_map_set(self.ptr(slot), key.get_nanbox_f64(), f64::from_bits(value));
            self.fill(slot, JSValue::pointer(map as *const u8).bits(), Made::Value);
        }
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn set(&mut self, slot: usize, values: &'m [SerializedValue]) -> u64 {
        let set = crate::set::js_set_alloc((values.len() as u32).max(8));
        self.fill(slot, JSValue::pointer(set as *const u8).bits(), Made::Value);
        for value in values {
            let value = self.value(value);
            let set = crate::set::js_set_add(self.ptr(slot), f64::from_bits(value));
            self.fill(slot, JSValue::pointer(set as *const u8).bits(), Made::Value);
        }
        self.get(slot)
    }

    #[optimize(size)]
    unsafe fn error(
        &mut self,
        slot: usize,
        name: &[u8],
        message: Option<&[u8]>,
        stack: Option<&[u8]>,
        cause: Option<&'m SerializedValue>,
    ) -> u64 {
        let kind = crate::error::error_kind_for_name(name);
        let message = message.map_or(ptr::null_mut(), |bytes| heap_string(bytes));
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let err = crate::error::js_error_new_kind_with_options(kind, message, undefined);
        self.fill(slot, JSValue::pointer(err as *const u8).bits(), Made::Value);
        if let Some(stack) = stack {
            let text = heap_string(stack);
            crate::error::error_set_stack(self.ptr(slot), text);
        }
        if let Some(cause) = cause {
            let cause = self.value(cause);
            crate::error::error_set_cause(self.ptr(slot), f64::from_bits(cause));
        }
        self.get(slot)
    }
}

#[optimize(size)]
unsafe fn string_bits(bytes: &[u8]) -> u64 {
    JSValue::string_ptr(heap_string(bytes)).bits()
}

#[optimize(size)]
unsafe fn heap_string(bytes: &[u8]) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(
        if bytes.is_empty() {
            ptr::null()
        } else {
            bytes.as_ptr()
        },
        bytes.len() as u32,
    )
}

/// Perry's Uint8Array is a `BufferHeader` plus a brand; restore both (#10103).
#[optimize(size)]
unsafe fn owned_uint8array(bytes: &[u8]) -> u64 {
    let buffer = new_buffer(crate::buffer::bytes::Brand::Uint8Array, bytes);
    JSValue::pointer(buffer as *const u8).bits()
}

#[optimize(size)]
unsafe fn array_buffer(bytes: &[u8]) -> u64 {
    let buffer = new_buffer(crate::buffer::bytes::Brand::ArrayBuffer, bytes);
    JSValue::pointer(buffer as *const u8).bits()
}

#[optimize(size)]
unsafe fn new_buffer(
    brand: crate::buffer::bytes::Brand,
    bytes: &[u8],
) -> *mut crate::buffer::BufferHeader {
    JSValue::from_bits(crate::buffer::bytes::from_slice(brand, bytes).to_bits())
        .as_pointer::<crate::buffer::BufferHeader>()
        .cast_mut()
}

#[optimize(size)]
unsafe fn owned_typed_array(kind: u8, length: u32, bytes: &[u8]) -> u64 {
    let ta = crate::typedarray_hook_slots::typed_array_alloc(kind, length);
    if !bytes.is_empty() {
        crate::buffer::bytes::no_gc(|scope| {
            let value = crate::value::js_nanbox_pointer(ta as i64);
            let destination = crate::buffer::bytes::bytes_mut(value, scope)
                .expect("fresh typed array must expose its bytes");
            destination[..bytes.len()].copy_from_slice(bytes);
        });
    }
    JSValue::pointer(ta as *const u8).bits()
}
