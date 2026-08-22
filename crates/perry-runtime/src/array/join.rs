//! `Array.prototype.join`: GC-safe construction directly in StringHeader storage.

use super::*;
use crate::gc::RuntimeHandle;
use crate::string::{StringHeader, STRING_FLAG_HAS_LONE_SURROGATES};
use crate::value::{
    JSValue, POINTER_MASK, SHORT_STRING_DATA_MASK, STRING_TAG, TAG_HOLE, TAG_UNDEFINED,
};
use std::ptr;

/// Matches Node's `buffer.constants.MAX_STRING_LENGTH`.
const MAX_STRING_LENGTH: u64 = 536_870_888;
/// Avoid a huge speculative allocation for very large or sparse arrays. The
/// builder grows geometrically if the sampled estimate is too small.
const MAX_INITIAL_CAPACITY: u64 = 16 * 1024 * 1024;
const CAPACITY_SAMPLE_SIZE: usize = 32;

#[inline(always)]
unsafe fn array_elements_ptr(arr: *const ArrayHeader) -> *const f64 {
    unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 }
}

#[cold]
fn throw_invalid_string_length() -> ! {
    let message = b"Invalid string length";
    let string = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_rangeerror_new(string);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64))
}

/// Read one exotic element while keeping the receiver current across a getter
/// or prototype lookup that can allocate and move it.
#[cold]
#[inline(never)]
fn exotic_element(arr: *const ArrayHeader, index: u32) -> Option<u64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let present = arr_handle.with_const_ptr(|arr| crate::array::array_spec_has_index(arr, index));
    if !present {
        return None;
    }
    Some(
        arr_handle
            .with_const_ptr(|arr| crate::array::array_spec_get(arr, index))
            .to_bits(),
    )
}

/// Run an allocating element conversion with the receiver and value rooted.
#[cold]
#[inline(never)]
fn element_to_string(arr: *const ArrayHeader, element_bits: u64) -> *const StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_const_ptr(arr);
    let element_handle = scope.root_nanbox_u64(element_bits);
    crate::value::js_jsvalue_to_string(element_handle.get_nanbox_f64())
}

/// Accept the legacy cross-module representation where a real StringHeader is
/// boxed with POINTER_TAG instead of STRING_TAG. Native handle ids and
/// headerless buffer/symbol pointers must be excluded before the GC-header
/// read.
#[inline]
unsafe fn pointer_tagged_string(element_bits: u64) -> Option<*const StringHeader> {
    let value = JSValue::from_bits(element_bits);
    if !value.is_pointer() {
        return None;
    }
    let address = (element_bits & POINTER_MASK) as usize;
    if crate::buffer::is_registered_buffer(address) || crate::symbol::is_registered_symbol(address)
    {
        return None;
    }
    unsafe { crate::value::addr_class::try_read_gc_header(address) }
        .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_STRING)
        .then_some(address as *const StringHeader)
}

#[inline]
unsafe fn direct_string(element_bits: u64) -> Option<*const StringHeader> {
    let value = JSValue::from_bits(element_bits);
    if value.is_string() {
        Some(value.as_string_ptr())
    } else {
        unsafe { pointer_tagged_string(element_bits) }
    }
}

#[derive(Clone, Copy)]
struct WellFormedJoinSize {
    byte_len: u64,
    utf16_len: u64,
}

/// Prove the common dense path contains only directly writable, well-formed
/// values while computing the exact result size. This loop deliberately does
/// no byte decoding and accounts for all separators in one multiplication.
unsafe fn well_formed_join_size(
    arr: *const ArrayHeader,
    separator: *const StringHeader,
    length: u32,
) -> Option<WellFormedJoinSize> {
    let (separator_bytes, separator_utf16) = if separator.is_null() {
        (1u64, 1u64)
    } else {
        if unsafe { (*separator).flags } & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
            return None;
        }
        unsafe { ((*separator).byte_len as u64, (*separator).utf16_len as u64) }
    };
    let repeats = length.saturating_sub(1) as u64;
    let mut size = WellFormedJoinSize {
        byte_len: separator_bytes.saturating_mul(repeats),
        utf16_len: separator_utf16.saturating_mul(repeats),
    };
    let elements = unsafe { array_elements_ptr(arr) };

    for index in 0..length as usize {
        let bits = unsafe { (*elements.add(index)).to_bits() };
        if bits == TAG_HOLE {
            continue;
        }
        let value = JSValue::from_bits(bits);
        let (byte_len, utf16_len) = if value.is_short_string() {
            let len = value.short_string_len() as u64;
            (len, len)
        } else if let Some(string) = unsafe { direct_string(bits) } {
            if unsafe { (*string).flags } & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
                return None;
            }
            unsafe { ((*string).byte_len as u64, (*string).utf16_len as u64) }
        } else if value.is_bool() {
            let len = if value.as_bool() { 4 } else { 5 };
            (len, len)
        } else if value.is_null() || value.is_undefined() {
            continue;
        } else {
            return None;
        };
        size.byte_len = size.byte_len.saturating_add(byte_len);
        size.utf16_len = size.utf16_len.saturating_add(utf16_len);
    }
    Some(size)
}

/// Exact-size direct writer for the hot well-formed path. After the sole
/// destination allocation, both receiver and separator are re-read from their
/// roots before any payload pointer is used.
fn try_join_well_formed(
    arr: *const ArrayHeader,
    separator: *const StringHeader,
    length: u32,
) -> Option<*mut StringHeader> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let separator_handle = (!separator.is_null()).then(|| scope.root_string_ptr(separator));
    let size = unsafe { well_formed_join_size(arr, separator, length) }?;
    if size.utf16_len > MAX_STRING_LENGTH || size.byte_len > u32::MAX as u64 {
        throw_invalid_string_length();
    }

    let (result, mut cursor) = crate::string::string_storage_alloc(size.byte_len as u32);
    let mut write_result = |arr: *const ArrayHeader, separator: *const StringHeader| unsafe {
        let elements = array_elements_ptr(arr);
        let (separator_data, separator_len) = if separator.is_null() {
            (b",".as_ptr(), 1usize)
        } else {
            (
                crate::string::string_data(separator),
                (*separator).byte_len as usize,
            )
        };

        crate::string::init_string_header(
            result,
            size.utf16_len as u32,
            size.byte_len as u32,
            size.byte_len as u32,
            0,
            0,
        );
        for index in 0..length as usize {
            if index > 0 {
                if separator_len == 1 {
                    *cursor = *separator_data;
                } else if separator_len != 0 {
                    ptr::copy_nonoverlapping(separator_data, cursor, separator_len);
                }
                cursor = cursor.add(separator_len);
            }

            let bits = (*elements.add(index)).to_bits();
            if bits == TAG_HOLE {
                continue;
            }
            let value = JSValue::from_bits(bits);
            if value.is_short_string() {
                let len = value.short_string_len();
                let bytes = (bits & SHORT_STRING_DATA_MASK).to_le_bytes();
                ptr::copy_nonoverlapping(bytes.as_ptr(), cursor, len);
                cursor = cursor.add(len);
            } else if let Some(string) = direct_string(bits) {
                let len = (*string).byte_len as usize;
                if len != 0 {
                    ptr::copy_nonoverlapping(crate::string::string_data(string), cursor, len);
                    cursor = cursor.add(len);
                }
            } else if value.is_bool() {
                let bytes = if value.as_bool() {
                    b"true".as_slice()
                } else {
                    b"false".as_slice()
                };
                ptr::copy_nonoverlapping(bytes.as_ptr(), cursor, bytes.len());
                cursor = cursor.add(bytes.len());
            }
        }
        debug_assert_eq!(
            cursor,
            crate::string::string_data(result).add(size.byte_len as usize) as *mut u8
        );
    };
    if let Some(separator_handle) = separator_handle {
        separator_handle.with_const_ptr(|separator| {
            arr_handle.with_const_ptr(|arr| write_result(arr, separator))
        });
    } else {
        arr_handle.with_const_ptr(|arr| write_result(arr, ptr::null()));
    }
    Some(result)
}

/// A rooted, growable StringHeader. Capacity is estimated from a small sample,
/// so ordinary joins allocate once and write each element once. If growth is
/// needed, sources are re-read from their roots after the allocation.
struct DirectJoinBuilder<'scope> {
    result: RuntimeHandle<'scope>,
    current: *mut StringHeader,
    byte_len: u32,
    utf16_len: u64,
    surrogate_count: u64,
    surrogate_pair_count: u64,
    pending_high_surrogate: bool,
}

impl<'scope> DirectJoinBuilder<'scope> {
    fn new(scope: &'scope crate::gc::RuntimeHandleScope, capacity: u32) -> Self {
        let (result, data) = crate::string::string_storage_alloc(capacity);
        unsafe {
            crate::string::init_string_header(result, 0, 0, capacity, 0, 0);
            // Capacity beyond the final byte length is still inside the GC
            // object. Initialize it before a coercion can run the conservative
            // from-space verifier over this private, partially built string.
            ptr::write_bytes(data, 0, capacity as usize);
        }
        Self {
            result: scope.root_nanbox_u64(STRING_TAG | (result as u64 & POINTER_MASK)),
            current: result,
            byte_len: 0,
            utf16_len: 0,
            surrogate_count: 0,
            surrogate_pair_count: 0,
            pending_high_surrogate: false,
        }
    }

    #[inline]
    fn result_ptr(&self) -> *mut StringHeader {
        self.current
    }

    #[inline]
    fn refresh_after_gc(&mut self) {
        self.current = (self.result.get_nanbox_u64() & POINTER_MASK) as *mut StringHeader;
    }

    #[inline]
    fn flags(&self) -> u32 {
        if self.surrogate_count > self.surrogate_pair_count.saturating_mul(2) {
            STRING_FLAG_HAS_LONE_SURROGATES
        } else {
            0
        }
    }

    #[inline]
    fn check_utf16_growth(&self, additional: u32) {
        if self.utf16_len.saturating_add(additional as u64) > MAX_STRING_LENGTH {
            throw_invalid_string_length();
        }
    }

    /// Reserve enough raw bytes for the next piece. Raw WTF-8 length is an
    /// upper bound because a high+low surrogate pair shrinks from six bytes to
    /// four when canonicalized.
    fn reserve(&mut self, additional: u32) {
        let required = self.byte_len as u64 + additional as u64;
        if required > u32::MAX as u64 {
            throw_invalid_string_length();
        }
        let current = self.result_ptr();
        let capacity = unsafe { (*current).capacity };
        if required <= capacity as u64 {
            return;
        }

        let new_capacity = required
            .max((capacity as u64).saturating_mul(2))
            .max(64)
            .min(u32::MAX as u64) as u32;
        let (new_result, new_data) = crate::string::string_storage_alloc(new_capacity);

        // Allocation may move the old builder. Re-read it from the root before
        // copying, then publish the replacement in that same root.
        let old_result = (self.result.get_nanbox_u64() & POINTER_MASK) as *mut StringHeader;
        unsafe {
            crate::string::init_string_header(
                new_result,
                self.utf16_len as u32,
                self.byte_len,
                new_capacity,
                0,
                self.flags(),
            );
            ptr::write_bytes(new_data, 0, new_capacity as usize);
            if self.byte_len != 0 {
                ptr::copy_nonoverlapping(
                    crate::string::string_data(old_result),
                    new_data,
                    self.byte_len as usize,
                );
            }
        }
        self.result
            .set_nanbox_u64(STRING_TAG | (new_result as u64 & POINTER_MASK));
        self.current = new_result;
    }

    #[inline]
    unsafe fn publish_header(&self) {
        let result = self.result_ptr();
        unsafe {
            (*result).utf16_len = self.utf16_len as u32;
            (*result).byte_len = self.byte_len;
            (*result).flags = self.flags();
        }
    }

    /// Write a piece after `reserve(byte_len)` and the UTF-16 limit check.
    /// WTF-8 surrogate halves are canonicalized across empty element and
    /// separator boundaries in place.
    unsafe fn write_prepared_piece(
        &mut self,
        data: *const u8,
        byte_len: u32,
        utf16_len: u32,
        flags: u32,
    ) {
        self.utf16_len += utf16_len as u64;
        if byte_len == 0 {
            return;
        }

        let result = self.result_ptr();
        let output = crate::string::string_data(result) as *mut u8;
        if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    data,
                    output.add(self.byte_len as usize),
                    byte_len as usize,
                );
            }
            self.byte_len += byte_len;
            self.pending_high_surrogate = false;
            return;
        }

        let bytes = unsafe { std::slice::from_raw_parts(data, byte_len as usize) };
        let mut index = 0;
        while index < bytes.len() {
            let remaining = &bytes[index..];
            if remaining.len() >= 3 && remaining[0] == 0xED && (0xA0..=0xAF).contains(&remaining[1])
            {
                unsafe {
                    ptr::copy_nonoverlapping(
                        remaining.as_ptr(),
                        output.add(self.byte_len as usize),
                        3,
                    );
                }
                self.byte_len += 3;
                self.surrogate_count += 1;
                self.pending_high_surrogate = true;
                index += 3;
            } else if remaining.len() >= 3
                && remaining[0] == 0xED
                && (0xB0..=0xBF).contains(&remaining[1])
            {
                self.surrogate_count += 1;
                if self.pending_high_surrogate {
                    let high_pos = self.byte_len as usize - 3;
                    let high = unsafe { std::slice::from_raw_parts(output.add(high_pos), 3) };
                    let high_code_unit = ((high[0] as u32 & 0x0F) << 12)
                        | ((high[1] as u32 & 0x3F) << 6)
                        | (high[2] as u32 & 0x3F);
                    let low_code_unit = ((remaining[0] as u32 & 0x0F) << 12)
                        | ((remaining[1] as u32 & 0x3F) << 6)
                        | (remaining[2] as u32 & 0x3F);
                    let code_point =
                        0x10000 + ((high_code_unit - 0xD800) << 10) + (low_code_unit - 0xDC00);
                    let mut encoded = [0u8; 4];
                    let encoded = unsafe { char::from_u32_unchecked(code_point) }
                        .encode_utf8(&mut encoded)
                        .as_bytes();
                    unsafe {
                        ptr::copy_nonoverlapping(encoded.as_ptr(), output.add(high_pos), 4);
                    }
                    self.byte_len += 1;
                    self.surrogate_pair_count += 1;
                } else {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            remaining.as_ptr(),
                            output.add(self.byte_len as usize),
                            3,
                        );
                    }
                    self.byte_len += 3;
                }
                self.pending_high_surrogate = false;
                index += 3;
            } else {
                unsafe {
                    *output.add(self.byte_len as usize) = remaining[0];
                }
                self.byte_len += 1;
                self.pending_high_surrogate = false;
                index += 1;
            }
        }
    }

    fn append_static(&mut self, bytes: &'static [u8]) {
        let len = bytes.len() as u32;
        self.check_utf16_growth(len);
        self.reserve(len);
        unsafe { self.write_prepared_piece(bytes.as_ptr(), len, len, 0) };
    }

    fn append_sso(&mut self, bits: u64) {
        let value = JSValue::from_bits(bits);
        let len = value.short_string_len() as u32;
        self.check_utf16_growth(len);
        self.reserve(len);
        let bytes = (bits & SHORT_STRING_DATA_MASK).to_le_bytes();
        unsafe { self.write_prepared_piece(bytes.as_ptr(), len, len, 0) };
    }

    fn append_separator(&mut self, separator: RuntimeHandle<'scope>) {
        let (byte_len, utf16_len, flags) =
            separator.with_const_ptr::<StringHeader, _>(|source| unsafe {
                ((*source).byte_len, (*source).utf16_len, (*source).flags)
            });
        self.check_utf16_growth(utf16_len);
        self.reserve(byte_len);
        separator.with_const_ptr::<StringHeader, _>(|source| unsafe {
            self.write_prepared_piece(
                crate::string::string_data(source),
                byte_len,
                utf16_len,
                flags,
            )
        });
    }

    fn append_rooted_string(&mut self, source: *const StringHeader) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = scope.root_string_ptr(source);
        let (byte_len, utf16_len, flags) =
            source.with_const_ptr::<StringHeader, _>(|current| unsafe {
                ((*current).byte_len, (*current).utf16_len, (*current).flags)
            });
        self.check_utf16_growth(utf16_len);
        self.reserve(byte_len);
        source.with_const_ptr::<StringHeader, _>(|current| unsafe {
            self.write_prepared_piece(
                crate::string::string_data(current),
                byte_len,
                utf16_len,
                flags,
            )
        });
    }

    fn append_array_string(
        &mut self,
        array: RuntimeHandle<'scope>,
        index: usize,
        source: *const StringHeader,
    ) {
        let (byte_len, utf16_len, flags) =
            unsafe { ((*source).byte_len, (*source).utf16_len, (*source).flags) };
        self.check_utf16_growth(utf16_len);
        self.reserve(byte_len);

        // Growth may move both the array and the string in its slot. Re-read
        // the slot after reserving instead of retaining the old payload ptr.
        array.with_const_ptr(|array| unsafe {
            let bits = (*array_elements_ptr(array).add(index)).to_bits();
            let source = direct_string(bits)
                .expect("direct join string slot changed during a GC-only window");
            debug_assert_eq!((*source).byte_len, byte_len);
            self.write_prepared_piece(
                crate::string::string_data(source),
                byte_len,
                utf16_len,
                flags,
            )
        });
    }

    fn finish(self) -> *mut StringHeader {
        unsafe { self.publish_header() };
        self.result_ptr()
    }
}

#[inline]
unsafe fn estimated_element_bytes(bits: u64) -> u32 {
    if bits == TAG_HOLE {
        return 0;
    }
    let value = JSValue::from_bits(bits);
    if let Some(string) = unsafe { direct_string(bits) } {
        unsafe { (*string).byte_len }
    } else if value.is_short_string() {
        value.short_string_len() as u32
    } else if value.is_bool() {
        if value.as_bool() {
            4
        } else {
            5
        }
    } else if value.is_null() || value.is_undefined() {
        0
    } else {
        // Numbers and ordinary object stringifications are commonly short.
        8
    }
}

/// Estimate once from a bounded prefix. The 12.5% slack absorbs modest
/// variance while keeping peak memory below the old Rust String + copied GC
/// String pair. No getter or user coercion is invoked during sampling.
unsafe fn estimate_initial_capacity(
    arr: *const ArrayHeader,
    separator: *const StringHeader,
    length: u32,
    exotic: bool,
) -> u32 {
    let separator_len = if separator.is_null() {
        1
    } else {
        unsafe { (*separator).byte_len as u64 }
    };
    let separator_bytes = separator_len.saturating_mul(length.saturating_sub(1) as u64);

    let element_bytes = if exotic {
        (length as u64).saturating_mul(8)
    } else {
        let sample_len = (length as usize).min(CAPACITY_SAMPLE_SIZE);
        if sample_len == 0 {
            0
        } else {
            let elements = unsafe { array_elements_ptr(arr) };
            let mut sample_bytes = 0u64;
            for index in 0..sample_len {
                let bits = unsafe { (*elements.add(index)).to_bits() };
                sample_bytes =
                    sample_bytes.saturating_add(unsafe { estimated_element_bytes(bits) as u64 });
            }
            sample_bytes
                .saturating_add(sample_len as u64 - 1)
                .checked_div(sample_len as u64)
                .unwrap_or(0)
                .saturating_mul(length as u64)
        }
    };

    let estimate = separator_bytes.saturating_add(element_bytes);
    if estimate == 0 {
        return 0;
    }
    estimate
        .saturating_add(estimate / 8)
        .saturating_add(32)
        .min(MAX_INITIAL_CAPACITY)
        .min(u32::MAX as u64) as u32
}

fn join_values(
    arr: *const ArrayHeader,
    separator: *const StringHeader,
    length: u32,
    exotic: bool,
) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let separator_handle = (!separator.is_null()).then(|| scope.root_string_ptr(separator));
    let initial_capacity = unsafe { estimate_initial_capacity(arr, separator, length, exotic) };
    let mut result = DirectJoinBuilder::new(&scope, initial_capacity);

    for index in 0..length as usize {
        if index > 0 {
            if let Some(separator) = separator_handle {
                result.append_separator(separator);
            } else {
                result.append_static(b",");
            }
        }

        let element_bits = if exotic {
            let bits = arr_handle
                .with_const_ptr(|arr| exotic_element(arr, index as u32))
                .unwrap_or(TAG_UNDEFINED);
            result.refresh_after_gc();
            bits
        } else {
            arr_handle
                .with_const_ptr(|arr| unsafe { (*array_elements_ptr(arr).add(index)).to_bits() })
        };
        if element_bits == TAG_HOLE {
            continue;
        }

        let value = JSValue::from_bits(element_bits);
        if value.is_null() || value.is_undefined() {
            continue;
        }
        if value.is_short_string() {
            result.append_sso(element_bits);
        } else if let Some(string) = unsafe { direct_string(element_bits) } {
            if exotic {
                result.append_rooted_string(string);
            } else {
                result.append_array_string(arr_handle, index, string);
            }
        } else if value.is_bool() {
            result.append_static(if value.as_bool() { b"true" } else { b"false" });
        } else {
            crate::builtins::reject_symbol_to_string(f64::from_bits(element_bits));
            let string = arr_handle.with_const_ptr(|arr| element_to_string(arr, element_bits));
            result.refresh_after_gc();
            if crate::string::is_valid_string_ptr(string) {
                result.append_rooted_string(string);
            }
        }
    }

    result.finish()
}

/// Join array elements into one GC-managed StringHeader, writing pieces
/// directly into its payload instead of assembling a Rust String first.
#[no_mangle]
pub extern "C" fn js_array_join(
    arr: *const ArrayHeader,
    separator: *const StringHeader,
) -> *mut StringHeader {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(ptr::null(), 0);
    }
    // #3148: TypedArray receiver — join element-typed values (Node formatting).
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_join(
            arr as *const crate::typedarray::TypedArrayHeader,
            separator,
        );
    }

    let length = unsafe { (*arr).length };
    if length == 0 {
        return crate::string::js_string_from_bytes(ptr::null(), 0);
    }
    let exotic = crate::array::array_iteration_is_exotic(arr);
    if !exotic {
        if let Some(result) = try_join_well_formed(arr, separator, length) {
            return result;
        }
    }
    join_values(arr, separator, length, exotic)
}

#[no_mangle]
pub extern "C" fn js_array_join_value(
    arr: *const ArrayHeader,
    separator_value: f64,
) -> *mut StringHeader {
    // Separator ToString can run user code and collect. Keep the receiver
    // current until js_array_join establishes its own roots.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let separator = if separator_value.to_bits() == TAG_UNDEFINED {
        ptr::null()
    } else {
        if unsafe { crate::symbol::js_is_symbol(separator_value) } != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
        }
        crate::value::js_jsvalue_to_string(separator_value) as *const StringHeader
    };
    arr_handle.with_const_ptr(|arr| js_array_join(arr, separator))
}

// Codegen lowers `arr.join(sep)` to this symbol. Keep it alive through the
// auto-optimize whole-program-bitcode link.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_JOIN_VALUE: extern "C" fn(*const ArrayHeader, f64) -> *mut StringHeader =
    js_array_join_value;

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed_string(string: *const StringHeader) -> f64 {
        f64::from_bits(STRING_TAG | (string as u64 & POINTER_MASK))
    }

    unsafe fn result_bytes(result: *const StringHeader) -> &'static [u8] {
        unsafe {
            std::slice::from_raw_parts(
                crate::string::string_data(result),
                (*result).byte_len as usize,
            )
        }
    }

    #[test]
    fn join_canonicalizes_a_surrogate_pair_across_an_empty_field() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = js_array_alloc(3);
        let arr_handle = scope.root_raw_mut_ptr(arr);

        let high = crate::string::js_string_from_wtf8_bytes([0xED, 0xA0, 0xBD].as_ptr(), 3);
        let arr = arr_handle.with_mut_ptr(|arr| js_array_push_f64(arr, boxed_string(high)));
        let arr = js_array_push_hole(arr);
        arr_handle.with_mut_ptr(|current| assert_eq!(arr, current));
        let low = crate::string::js_string_from_wtf8_bytes([0xED, 0xB8, 0x80].as_ptr(), 3);
        let arr = arr_handle.with_mut_ptr(|arr| js_array_push_f64(arr, boxed_string(low)));
        arr_handle.with_mut_ptr(|current| assert_eq!(arr, current));
        let empty = crate::string::js_string_from_bytes(ptr::null(), 0);

        let result = arr_handle.with_const_ptr(|arr| js_array_join(arr, empty));
        unsafe {
            assert_eq!((*result).utf16_len, 2);
            assert_eq!((*result).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
            assert_eq!(result_bytes(result), "😀".as_bytes());
        }
    }

    #[test]
    fn join_preserves_separated_lone_surrogates_and_flags() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr = js_array_alloc(2);
        let arr_handle = scope.root_raw_mut_ptr(arr);

        let high = crate::string::js_string_from_wtf8_bytes([0xED, 0xA0, 0xBD].as_ptr(), 3);
        let arr = arr_handle.with_mut_ptr(|arr| js_array_push_f64(arr, boxed_string(high)));
        arr_handle.with_mut_ptr(|current| assert_eq!(arr, current));
        let low = crate::string::js_string_from_wtf8_bytes([0xED, 0xB8, 0x80].as_ptr(), 3);
        let arr = arr_handle.with_mut_ptr(|arr| js_array_push_f64(arr, boxed_string(low)));
        arr_handle.with_mut_ptr(|current| assert_eq!(arr, current));
        let separator = crate::string::js_string_from_bytes(b"|".as_ptr(), 1);

        let result = arr_handle.with_const_ptr(|arr| js_array_join(arr, separator));
        unsafe {
            assert_eq!((*result).utf16_len, 3);
            assert_ne!((*result).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
            assert_eq!(
                result_bytes(result),
                &[0xED, 0xA0, 0xBD, b'|', 0xED, 0xB8, 0x80]
            );
        }
    }
}
