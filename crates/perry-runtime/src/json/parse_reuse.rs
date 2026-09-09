use super::*;

crate::perry_thread_local! {
    /// One reusable immutable string token from the most recently parsed
    /// source. Both pointers are ordinary GC roots and are rewritten on a
    /// moving collection. The source length guards the only in-place string
    /// mutation Perry permits; equal-length string contents are immutable.
    static PARSE_STRING_CACHE: RefCell<Option<ParseStringCacheEntry>> = const { RefCell::new(None) };

    /// A bounded construction template for repeated parses of the same small
    /// object source. It owns only immutable strings, a canonical key shape,
    /// and inline scalar bits; every mutable object/array is born afresh.
    static PARSE_OBJECT_TEMPLATE: RefCell<Option<ParseObjectTemplate>> = const { RefCell::new(None) };
}

struct ParseStringCacheEntry {
    source: *const StringHeader,
    source_len: u32,
    token_start: u32,
    token_end: u32,
    value: *const StringHeader,
    direct_depth_validated: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ParseStringReuse {
    pub(crate) token_start: usize,
    pub(crate) token_end: usize,
    pub(crate) value: *const StringHeader,
}

const PARSE_STRING_REUSE_MIN_BYTES: usize = 256;
const PARSE_STRING_CACHE_MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const PARSE_OBJECT_TEMPLATE_MIN_SOURCE_BYTES: usize = 65;
const PARSE_OBJECT_TEMPLATE_MAX_SOURCE_BYTES: usize = 2048;
const PARSE_OBJECT_TEMPLATE_MAX_FIELDS: usize = 8;
const PARSE_OBJECT_TEMPLATE_MAX_ARRAY: usize = 8;

#[derive(Clone, Copy)]
enum ParseTemplateValue {
    Inline(JSValue),
    Array {
        values: [JSValue; PARSE_OBJECT_TEMPLATE_MAX_ARRAY],
        len: u8,
    },
}

#[derive(Clone, Copy)]
struct ParseObjectTemplate {
    source: *const StringHeader,
    source_len: u16,
    keys_array: *mut crate::array::ArrayHeader,
    shape_id: u32,
    values: [ParseTemplateValue; PARSE_OBJECT_TEMPLATE_MAX_FIELDS],
    len: u8,
}

const EMPTY_PARSE_TEMPLATE_VALUE: ParseTemplateValue =
    ParseTemplateValue::Inline(JSValue::undefined());

#[inline]
pub(crate) fn cached_parse_string(
    source: *const StringHeader,
    source_len: usize,
) -> Option<ParseStringReuse> {
    if source.is_null()
        || source_len > PARSE_STRING_CACHE_MAX_SOURCE_BYTES
        || source_len > u32::MAX as usize
    {
        return None;
    }
    PARSE_STRING_CACHE.with(|cache| {
        let cache = cache.borrow();
        let entry = cache.as_ref()?;
        (entry.source == source
            && entry.source_len == source_len as u32
            && entry.token_end <= entry.source_len)
            .then_some(ParseStringReuse {
                token_start: entry.token_start as usize,
                token_end: entry.token_end as usize,
                value: entry.value,
            })
    })
}

#[inline]
pub(crate) fn remember_parse_string(
    source: *const StringHeader,
    source_len: usize,
    token_start: usize,
    token_end: usize,
    value: *const StringHeader,
    value_len: usize,
) {
    if source.is_null()
        || value.is_null()
        || value_len < PARSE_STRING_REUSE_MIN_BYTES
        // This cache is a strong root. Bound both its absolute footprint and
        // its usefulness: retaining a multi-megabyte records document for one
        // medium-sized field would trade a little copying for excessive RSS.
        || source_len > PARSE_STRING_CACHE_MAX_SOURCE_BYTES
        || value_len < source_len.div_ceil(2)
        || source_len > u32::MAX as usize
        || token_start > u32::MAX as usize
        || token_end > u32::MAX as usize
    {
        return;
    }
    PARSE_STRING_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache
            .as_ref()
            .is_some_and(|entry| entry.source == source && entry.source_len == source_len as u32)
        {
            return;
        }
        *cache = Some(ParseStringCacheEntry {
            source,
            source_len: source_len as u32,
            token_start: token_start as u32,
            token_end: token_end as u32,
            value,
            direct_depth_validated: false,
        });
        crate::gc::runtime_write_barrier_root_nanbox(
            crate::JSValue::string_ptr(source.cast_mut()).bits(),
        );
        crate::gc::runtime_write_barrier_root_nanbox(
            crate::JSValue::string_ptr(value.cast_mut()).bits(),
        );
    });
}

#[inline]
pub(crate) fn cached_parse_source_is_direct(
    source: *const StringHeader,
    source_len: usize,
) -> bool {
    PARSE_STRING_CACHE.with(|cache| {
        cache.borrow().as_ref().is_some_and(|entry| {
            entry.source == source
                && entry.source_len as usize == source_len
                && entry.direct_depth_validated
        })
    })
}

#[inline]
pub(crate) fn validate_cached_parse_source(source: *const StringHeader, source_len: usize) {
    PARSE_STRING_CACHE.with(|cache| {
        if let Some(entry) = cache.borrow_mut().as_mut() {
            if entry.source == source && entry.source_len as usize == source_len {
                entry.direct_depth_validated = true;
            }
        }
    });
}

#[inline]
unsafe fn parse_template_value(value: JSValue) -> Option<ParseTemplateValue> {
    if !value.is_pointer() {
        return Some(ParseTemplateValue::Inline(value));
    }
    let array = value.as_pointer::<crate::array::ArrayHeader>();
    let header = &*(array
        .cast::<u8>()
        .sub(crate::gc::GC_HEADER_SIZE)
        .cast::<crate::gc::GcHeader>());
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || (*array).length as usize > PARSE_OBJECT_TEMPLATE_MAX_ARRAY
    {
        return None;
    }
    let mut values = [JSValue::undefined(); PARSE_OBJECT_TEMPLATE_MAX_ARRAY];
    for index in 0..(*array).length as usize {
        let element = crate::array::js_array_get(array, index as u32);
        if element.is_pointer() {
            return None;
        }
        values[index] = element;
    }
    Some(ParseTemplateValue::Array {
        values,
        len: (*array).length as u8,
    })
}

/// Capture a compact immutable construction plan from a completed small JSON
/// object. The returned object itself is never cached, so later user mutation
/// cannot affect subsequent parses.
pub(crate) unsafe fn remember_parse_object_template(
    source: *const StringHeader,
    source_len: usize,
    result: JSValue,
) {
    if source.is_null()
        || source_len < PARSE_OBJECT_TEMPLATE_MIN_SOURCE_BYTES
        || source_len > PARSE_OBJECT_TEMPLATE_MAX_SOURCE_BYTES
        || !result.is_pointer()
    {
        return;
    }
    let object = result.as_pointer::<crate::object::ObjectHeader>();
    let header = &*(object
        .cast::<u8>()
        .sub(crate::gc::GC_HEADER_SIZE)
        .cast::<crate::gc::GcHeader>());
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return;
    }
    let len = crate::object::object_live_slot_count(object) as usize;
    if len > PARSE_OBJECT_TEMPLATE_MAX_FIELDS {
        return;
    }
    let fields = object
        .cast::<u8>()
        .add(std::mem::size_of::<crate::object::ObjectHeader>())
        .cast::<JSValue>();
    let mut values = [EMPTY_PARSE_TEMPLATE_VALUE; PARSE_OBJECT_TEMPLATE_MAX_FIELDS];
    for (index, slot) in values[..len].iter_mut().enumerate() {
        let Some(value) = parse_template_value(*fields.add(index)) else {
            return;
        };
        *slot = value;
    }
    let entry = ParseObjectTemplate {
        source,
        source_len: source_len as u16,
        keys_array: crate::object::object_keys_array(object),
        shape_id: crate::object::shapes::object_shape_stamp(object),
        values,
        len: len as u8,
    };
    crate::gc::runtime_write_barrier_root_nanbox(JSValue::string_ptr(source.cast_mut()).bits());
    crate::gc::runtime_write_barrier_root_raw_ptr(entry.keys_array);
    for value in &entry.values[..len] {
        match *value {
            ParseTemplateValue::Inline(value) => {
                crate::gc::runtime_write_barrier_root_nanbox(value.bits());
            }
            ParseTemplateValue::Array { values, len } => {
                for value in &values[..len as usize] {
                    crate::gc::runtime_write_barrier_root_nanbox(value.bits());
                }
            }
        }
    }
    PARSE_OBJECT_TEMPLATE.with(|cache| *cache.borrow_mut() = Some(entry));
}

/// Rebuild only the mutable cells from a cached small-object plan. One pending
/// collection may run before the plan is reloaded; the cache scanner rewrites
/// every managed pointer in the meantime.
pub(crate) unsafe fn try_reuse_parse_object_template(
    source: *const StringHeader,
    source_len: usize,
) -> Option<JSValue> {
    if source.is_null()
        || source_len < PARSE_OBJECT_TEMPLATE_MIN_SOURCE_BYTES
        || source_len > PARSE_OBJECT_TEMPLATE_MAX_SOURCE_BYTES
    {
        return None;
    }
    let matches = PARSE_OBJECT_TEMPLATE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .is_some_and(|entry| entry.source == source && entry.source_len as usize == source_len)
    });
    if !matches {
        return None;
    }

    crate::gc::gc_collect_pending_suppressed_parse();
    let entry = PARSE_OBJECT_TEMPLATE.with(|cache| cache.borrow().as_ref().copied())?;
    let result = {
        let _no_move = crate::gc::GcSuppressScope::new();
        let mut batch = crate::arena::ConstructionBatch::new();
        let mut values = [JSValue::undefined(); PARSE_OBJECT_TEMPLATE_MAX_FIELDS];
        for (index, planned) in entry.values[..entry.len as usize].iter().enumerate() {
            values[index] = match *planned {
                ParseTemplateValue::Inline(value) => value,
                ParseTemplateValue::Array {
                    values: elements,
                    len,
                } => {
                    let mut array =
                        construction_array::ConstructionArray::new(&mut batch, len as u32);
                    for &element in &elements[..len as usize] {
                        array.push(&mut batch, element);
                    }
                    JSValue::object_ptr(array.finish(&batch).cast())
                }
            };
        }
        let object = crate::object::object_from_json_fields_preinstalled(
            &mut batch,
            entry.keys_array,
            entry.shape_id,
            &values[..entry.len as usize],
        );
        JSValue::object_ptr(object.cast())
    };
    parse_scalar::clear_oversized_key_cache();
    crate::gc::gc_schedule_tiny_parse_boundary_collection_if_pressure();
    Some(result)
}

#[cfg(test)]
pub(crate) fn test_parse_object_template_matches(
    source: *const StringHeader,
    source_len: usize,
) -> bool {
    PARSE_OBJECT_TEMPLATE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .is_some_and(|entry| entry.source == source && entry.source_len as usize == source_len)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_root_scanner_slots(
    string_source: *const StringHeader,
    string_value: *const StringHeader,
    template_source: *const StringHeader,
    keys: *mut crate::array::ArrayHeader,
    inline_value: JSValue,
    array_value: JSValue,
) {
    PARSE_STRING_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(ParseStringCacheEntry {
            source: string_source,
            source_len: 1,
            token_start: 0,
            token_end: 1,
            value: string_value,
            direct_depth_validated: false,
        });
    });
    let mut values = [EMPTY_PARSE_TEMPLATE_VALUE; PARSE_OBJECT_TEMPLATE_MAX_FIELDS];
    values[0] = ParseTemplateValue::Inline(inline_value);
    let mut array_values = [JSValue::undefined(); PARSE_OBJECT_TEMPLATE_MAX_ARRAY];
    array_values[0] = array_value;
    values[1] = ParseTemplateValue::Array {
        values: array_values,
        len: 1,
    };
    PARSE_OBJECT_TEMPLATE.with(|cache| {
        *cache.borrow_mut() = Some(ParseObjectTemplate {
            source: template_source,
            source_len: 1,
            keys_array: keys,
            shape_id: 0,
            values,
            len: 2,
        });
    });
}

#[cfg(test)]
pub(crate) fn test_root_scanner_slot_addresses() -> [usize; 6] {
    let strings = PARSE_STRING_CACHE.with(|cache| {
        let cache = cache.borrow();
        let entry = cache.as_ref().expect("seeded parse string cache");
        [entry.source as usize, entry.value as usize]
    });
    PARSE_OBJECT_TEMPLATE.with(|cache| {
        let cache = cache.borrow();
        let entry = cache.as_ref().expect("seeded parse object template");
        let ParseTemplateValue::Inline(inline) = entry.values[0] else {
            unreachable!("seeded inline template slot")
        };
        let ParseTemplateValue::Array { values, .. } = entry.values[1] else {
            unreachable!("seeded array template slot")
        };
        [
            strings[0],
            strings[1],
            entry.source as usize,
            entry.keys_array as usize,
            inline.as_string_ptr() as usize,
            values[0].as_string_ptr() as usize,
        ]
    })
}

pub(super) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PARSE_STRING_CACHE.with(|cache| {
        if let Some(entry) = cache.borrow_mut().as_mut() {
            visitor.visit_tagged_raw_const_ptr_slot(&mut entry.source, crate::value::STRING_TAG);
            visitor.visit_tagged_raw_const_ptr_slot(&mut entry.value, crate::value::STRING_TAG);
        }
    });
    PARSE_OBJECT_TEMPLATE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let Some(entry) = cache.as_mut() else {
            return;
        };
        visitor.visit_tagged_raw_const_ptr_slot(&mut entry.source, crate::value::STRING_TAG);
        visitor.visit_raw_mut_ptr_slot(&mut entry.keys_array);
        for value in &mut entry.values[..entry.len as usize] {
            match value {
                ParseTemplateValue::Inline(value) => {
                    let mut bits = value.bits();
                    visitor.visit_nanbox_u64_slot(&mut bits);
                    *value = JSValue::from_bits(bits);
                }
                ParseTemplateValue::Array { values, len } => {
                    for value in &mut values[..*len as usize] {
                        let mut bits = value.bits();
                        visitor.visit_nanbox_u64_slot(&mut bits);
                        *value = JSValue::from_bits(bits);
                    }
                }
            }
        }
    });
}

/// Rebuild ShapeId ownership for the object template after a full trace.
///
/// `keys_array` is a traced root, but tracing that array does not visit the
/// separate descriptor table entry named by `shape_id`. The ordinary parse
/// shape cache usually owns the same id; once that bounded cache is full,
/// however, this template can be the only metadata publisher left. Re-note it
/// before uncarried descriptors are pruned so a later template hit cannot
/// stamp a retired id into a fresh object.
pub(super) fn note_shape_carrier() {
    PARSE_OBJECT_TEMPLATE.with(|cache| {
        if let Some(entry) = cache.borrow().as_ref() {
            crate::object::shape_carriers::note_shape_id(entry.shape_id);
        }
    });
}

#[cfg(test)]
pub(super) fn clear_caches() {
    PARSE_STRING_CACHE.with(|cache| *cache.borrow_mut() = None);
    PARSE_OBJECT_TEMPLATE.with(|cache| *cache.borrow_mut() = None);
}
