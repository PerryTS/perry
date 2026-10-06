//! Reader half of the thread clone: a [`SerializedValue`] becomes JS values
//! in the current thread's heap.
//!
//! Every object the reader creates is rooted in one handle scope for the
//! whole read, in creation order, because a later `Ref` may name it and any
//! allocation may move it. Only one scope is used: a nested scope would drop
//! its handles before the outer read is done.

use std::ptr;

use super::{fs_thread_codec, store_thread_array_slot, store_thread_object_field};
use super::{SerializedValue, VIEW_KIND_DATA_VIEW};
use super::{BIGINT_TAG, POINTER_MASK, TAG_UNDEFINED};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::JSValue;

/// Deserialize a SerializedValue into a NaN-boxed JSValue.
///
/// Allocates any needed objects in the **current thread's** arena: the
/// caller picks the arena by calling this on the receiving thread.
///
/// # Safety
/// `sv` must come from the writer in `clone_write.rs` (or keep its rules).
pub unsafe fn deserialize_nanbox_on_current_thread(sv: &SerializedValue) -> u64 {
    let scope = RuntimeHandleScope::new();
    let mut reader = Reader {
        scope: &scope,
        made: Vec::new(),
    };
    reader.value(sv)
}

/// Whether the writer gave this value an index (`Writer::begin`). The reader
/// must make a slot for exactly these, in the same order.
fn remembers(sv: &SerializedValue) -> bool {
    matches!(
        sv,
        SerializedValue::Array(_)
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

struct Reader<'s> {
    scope: &'s RuntimeHandleScope,
    made: Vec<(RuntimeHandle<'s>, Made)>,
}

impl<'s> Reader<'s> {
    fn reserve(&mut self) -> usize {
        let handle = self.scope.root_nanbox_u64(TAG_UNDEFINED);
        self.made.push((handle, Made::Value));
        self.made.len() - 1
    }

    fn fill(&mut self, slot: usize, bits: u64, made: Made) {
        self.made[slot].0.set_nanbox_u64(bits);
        self.made[slot].1 = made;
    }

    fn get(&self, slot: usize) -> u64 {
        self.made[slot].0.get_nanbox_u64()
    }

    fn ptr<T>(&self, slot: usize) -> *mut T {
        (self.get(slot) & POINTER_MASK) as *mut T
    }

    /// The value a `Ref` names. For a view that owns its bytes, make its
    /// ArrayBuffer now and remember it.
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

    unsafe fn value(&mut self, sv: &SerializedValue) -> u64 {
        let slot = if remembers(sv) {
            Some(self.reserve())
        } else {
            None
        };
        let bits = match sv {
            SerializedValue::Inline(bits) => *bits,
            SerializedValue::Hole => crate::value::TAG_HOLE,
            SerializedValue::Ref(index) => self.resolve(*index as usize),
            SerializedValue::String(bytes) => string_bits(bytes),
            SerializedValue::BigInt(limbs) => {
                let ptr = crate::bigint::bigint_alloc_with_limbs(*limbs);
                BIGINT_TAG | (ptr as u64 & POINTER_MASK)
            }
            // #2089: a fresh DateCell in this thread's arena.
            SerializedValue::Date(ts) => crate::date::alloc_date_cell(*ts).to_bits(),
            SerializedValue::Array(elements) => return self.array(slot.unwrap(), elements),
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
                return crate::r#box::js_box_alloc_bits(value as i64) as u64;
            }
            SerializedValue::ScopeCapture(slots) => return self.scope_capture(slots),
            SerializedValue::Uint8Array(bytes) => owned_uint8array(bytes),
            SerializedValue::ArrayBuffer(bytes) => array_buffer(bytes),
            SerializedValue::TransferredArrayBuffer(store) => {
                let buffer = crate::buffer::buffer_adopt_backing(store.take(), store.length);
                crate::buffer::mark_as_array_buffer(buffer as usize);
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
                JSValue::pointer(*addr as *const u8).bits()
            }
            // The boundary rejects these before anything is read.
            SerializedValue::Unsupported(_) => TAG_UNDEFINED,
        };
        if let Some(slot) = slot {
            self.fill(slot, bits, Made::Value);
        }
        bits
    }

    unsafe fn array(&mut self, slot: usize, elements: &[SerializedValue]) -> u64 {
        let arr = crate::array::js_array_alloc(elements.len() as u32);
        self.fill(slot, JSValue::pointer(arr as *const u8).bits(), Made::Value);
        if elements.iter().any(|e| matches!(e, SerializedValue::Hole)) {
            // Pushing keeps the array's hole bookkeeping right.
            for element in elements {
                let bits = self.value(element);
                let grown = crate::array::js_array_push_f64(self.ptr(slot), f64::from_bits(bits));
                self.fill(
                    slot,
                    JSValue::pointer(grown as *const u8).bits(),
                    Made::Value,
                );
            }
            return self.get(slot);
        }
        for (i, element) in elements.iter().enumerate() {
            let bits = self.value(element);
            // GC_STORE_AUDIT(BARRIERED): deserialized thread array slot uses the shared array slot-store helper.
            store_thread_array_slot(self.ptr(slot), i, bits);
        }
        let arr = self.ptr::<crate::array::ArrayHeader>(slot);
        (*arr).length = elements.len() as u32;
        self.get(slot)
    }

    unsafe fn object(
        &mut self,
        slot: usize,
        final_constfn: Option<&super::constfn_transfer::ConstFnTransferFacts>,
        class_id: u32,
        parent_class_id: u32,
        fields: &[SerializedValue],
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
            let keys_arr = crate::array::js_array_alloc(names.len() as u32);
            let keys_handle = self.scope.root_raw_mut_ptr(keys_arr);
            for (i, name) in names.iter().enumerate() {
                let key = string_bits(name);
                // GC_STORE_AUDIT(BARRIERED): deserialized key array slot uses the shared array slot-store helper.
                keys_handle.with_mut_ptr(|keys_arr| store_thread_array_slot(keys_arr, i, key));
            }
            keys_handle.with_mut_ptr::<crate::array::ArrayHeader, _>(|keys_arr| {
                (*keys_arr).length = names.len() as u32;
                crate::object::js_object_set_keys(self.ptr(slot), keys_arr);
            });
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

    unsafe fn closure(
        &mut self,
        slot: usize,
        info: usize,
        capture_count: u32,
        captures: &[SerializedValue],
    ) -> u64 {
        let closure = crate::closure::js_closure_alloc(
            info as *const crate::closure::JsFunctionInfo,
            capture_count,
        );
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

    unsafe fn scope_capture(&mut self, slots: &[SerializedValue]) -> u64 {
        let base = crate::r#box::scope::js_scope_alloc(slots.len() as i32, TAG_UNDEFINED as i64)
            as usize as *mut u8;
        let rooted = self.scope.root_raw_mut_ptr(base);
        for (i, word) in slots.iter().enumerate() {
            let bits = self.value(word);
            rooted.with_mut_ptr(|base: *mut u8| {
                crate::r#box::scope::js_scope_set(base as i64, i as i32, bits as i64)
            });
        }
        rooted.with_mut_ptr(|base: *mut u8| base as u64)
    }

    unsafe fn view(
        &mut self,
        slot: usize,
        kind: u8,
        buffer: &SerializedValue,
        byte_offset: u32,
        length: u32,
    ) -> u64 {
        let elem = if kind == VIEW_KIND_DATA_VIEW {
            1
        } else {
            crate::typedarray::elem_size_for_kind(kind)
        };
        // A view over all of a buffer seen for the first time owns its
        // bytes; nothing can tell it from a view over a fresh ArrayBuffer.
        if let SerializedValue::ArrayBuffer(bytes) = buffer {
            if kind != VIEW_KIND_DATA_VIEW
                && byte_offset == 0
                && length as usize * elem == bytes.len()
            {
                let buffer_slot = self.reserve();
                let (bits, made) = if kind == crate::typedarray::KIND_UINT8 {
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
        } else if kind == crate::typedarray::KIND_UINT8 {
            let view = crate::buffer::js_uint8array_view(buffer, offset, length);
            JSValue::pointer(view as *const u8).bits()
        } else {
            let view =
                crate::typedarray_view::js_typed_array_view(kind as i32, buffer, offset, length);
            JSValue::pointer(view as *const u8).bits()
        };
        self.fill(slot, bits, Made::Value);
        bits
    }

    #[cfg(feature = "regex-engine")]
    unsafe fn regexp(&mut self, source: &[u8], flags: &[u8]) -> u64 {
        let source = self.scope.root_nanbox_u64(string_bits(source));
        let flags = string_bits(flags);
        let re = crate::regex::js_regexp_new(
            (source.get_nanbox_u64() & POINTER_MASK) as *const _,
            (flags & POINTER_MASK) as *const _,
        );
        JSValue::pointer(re as *const u8).bits()
    }

    /// Without the regex engine no RegExp can exist to be sent.
    #[cfg(not(feature = "regex-engine"))]
    unsafe fn regexp(&mut self, _source: &[u8], _flags: &[u8]) -> u64 {
        TAG_UNDEFINED
    }

    unsafe fn map(&mut self, slot: usize, entries: &[(SerializedValue, SerializedValue)]) -> u64 {
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

    unsafe fn set(&mut self, slot: usize, values: &[SerializedValue]) -> u64 {
        let set = crate::set::js_set_alloc((values.len() as u32).max(8));
        self.fill(slot, JSValue::pointer(set as *const u8).bits(), Made::Value);
        for value in values {
            let value = self.value(value);
            let set = crate::set::js_set_add(self.ptr(slot), f64::from_bits(value));
            self.fill(slot, JSValue::pointer(set as *const u8).bits(), Made::Value);
        }
        self.get(slot)
    }

    unsafe fn error(
        &mut self,
        slot: usize,
        name: &[u8],
        message: Option<&[u8]>,
        stack: Option<&[u8]>,
        cause: Option<&SerializedValue>,
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

unsafe fn string_bits(bytes: &[u8]) -> u64 {
    JSValue::string_ptr(heap_string(bytes)).bits()
}

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
unsafe fn owned_uint8array(bytes: &[u8]) -> u64 {
    let buffer = new_buffer(bytes);
    crate::buffer::mark_as_uint8array(buffer as usize);
    JSValue::pointer(buffer as *const u8).bits()
}

unsafe fn array_buffer(bytes: &[u8]) -> u64 {
    let buffer = new_buffer(bytes);
    crate::buffer::mark_as_array_buffer(buffer as usize);
    JSValue::pointer(buffer as *const u8).bits()
}

unsafe fn new_buffer(bytes: &[u8]) -> *mut crate::buffer::BufferHeader {
    JSValue::from_bits(
        crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::Buffer, bytes).to_bits(),
    )
    .as_pointer::<crate::buffer::BufferHeader>()
    .cast_mut()
}

unsafe fn owned_typed_array(kind: u8, length: u32, bytes: &[u8]) -> u64 {
    let ta = crate::typedarray::typed_array_alloc(kind, length);
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
