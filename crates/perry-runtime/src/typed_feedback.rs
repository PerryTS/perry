//! Runtime typed-feedback sites.
//!
//! The optimizer-facing inline caches stay where they are. This module records
//! a separate, source-attributed view of what each generated dynamic boundary
//! has actually seen at runtime.

use std::collections::{BTreeMap, HashMap};
#[cfg(any(feature = "diagnostics", test))]
use std::sync::atomic::AtomicBool;
#[cfg(any(feature = "diagnostics", test))]
use std::sync::atomic::Ordering;
use std::sync::{LazyLock, Mutex};

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::value::{
    BIGINT_TAG, INT32_TAG, JS_HANDLE_TAG, POINTER_MASK, POINTER_TAG, SHORT_STRING_TAG, STRING_TAG,
    TAG_FALSE, TAG_HOLE, TAG_MASK, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
};

const POLYMORPHIC_CAP: usize = 4;

static REGISTRY: LazyLock<Mutex<TypedFeedbackRegistry>> =
    LazyLock::new(|| Mutex::new(TypedFeedbackRegistry::default()));
#[cfg(any(feature = "diagnostics", test))]
static TRACE_DUMPED: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
static TYPED_FEEDBACK_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    std::env::var_os("PERRY_TYPED_FEEDBACK_TRACE").is_some()
        || std::env::var_os("PERRY_TYPED_FEEDBACK").is_some()
});

#[inline]
fn typed_feedback_enabled() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        *TYPED_FEEDBACK_ENABLED
    }
}

/// #5093: whether typed-feedback tracing is active. Read once at `js_gc_init`
/// to disable the codegen-inlined class-field fast path (which would skip the
/// observation recording the guard does in this mode).
pub(crate) fn typed_feedback_active() -> bool {
    typed_feedback_enabled()
}

#[cfg(test)]
pub(crate) static TYPED_FEEDBACK_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum TypedFeedbackSiteKind {
    PropertyGet = 0,
    PropertySet = 1,
    MethodCall = 2,
    ClosureCall = 3,
    ArrayElement = 4,
    NumericFieldWrite = 5,
    HelperReturn = 6,
}

impl TypedFeedbackSiteKind {
    fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::PropertySet,
            2 => Self::MethodCall,
            3 => Self::ClosureCall,
            4 => Self::ArrayElement,
            5 => Self::NumericFieldWrite,
            6 => Self::HelperReturn,
            _ => Self::PropertyGet,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PropertyGet => "property_get",
            Self::PropertySet => "property_set",
            Self::MethodCall => "method_call",
            Self::ClosureCall => "closure_call",
            Self::ArrayElement => "array_element",
            Self::NumericFieldWrite => "numeric_field_write",
            Self::HelperReturn => "helper_return",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum TypedFeedbackState {
    Uninitialized,
    Monomorphic,
    Polymorphic,
    Megamorphic,
}

impl TypedFeedbackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Monomorphic => "monomorphic",
            Self::Polymorphic => "polymorphic",
            Self::Megamorphic => "megamorphic",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservationSource {
    Property,
    Method,
    Closure,
    Array,
    NumericWrite,
    HelperReturn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Observation {
    source: ObservationSource,
    object_addr: usize,
    shape_addr: usize,
    key_hash: u64,
    class_id: u32,
    heap_type: u16,
    aux: u64,
    value_tag: u16,
}

impl Observation {
    fn same_feedback_key(&self, other: &Self) -> bool {
        if self.source != other.source {
            return false;
        }
        match self.source {
            ObservationSource::Property | ObservationSource::Method => {
                self.shape_addr == other.shape_addr
                    && self.key_hash == other.key_hash
                    && self.class_id == other.class_id
                    && self.heap_type == other.heap_type
                    && self.value_tag == other.value_tag
            }
            ObservationSource::NumericWrite => {
                self.shape_addr == other.shape_addr
                    && self.class_id == other.class_id
                    && self.heap_type == other.heap_type
                    && self.aux == other.aux
                    && self.value_tag == other.value_tag
            }
            ObservationSource::Closure => {
                self.aux == other.aux
                    && self.heap_type == other.heap_type
                    && self.value_tag == other.value_tag
            }
            ObservationSource::Array | ObservationSource::HelperReturn => {
                self.shape_addr == other.shape_addr
                    && self.class_id == other.class_id
                    && self.heap_type == other.heap_type
                    && self.aux == other.aux
                    && self.value_tag == other.value_tag
            }
        }
    }

    fn is_shape_keyed(&self) -> bool {
        matches!(
            self.source,
            ObservationSource::Property
                | ObservationSource::Method
                | ObservationSource::NumericWrite
        ) || (self.source == ObservationSource::HelperReturn
            && self.heap_type == crate::gc::GC_TYPE_OBJECT as u16)
    }

    fn roots_object_addr(&self) -> bool {
        false
    }

    fn roots_shape_addr(&self) -> bool {
        // #6804: an id token is NOT a heap address — visiting it would let
        // the GC mark garbage or, worse, rewrite the token if a moved
        // object's from-space address numerically collided with the id
        // range. Address tokens (class instances' keys arrays) keep the
        // visit; a skipped low-address token can only go stale, which the
        // equality-only usage reads as a benign mismatch.
        self.is_shape_keyed()
            && self.shape_addr != 0
            && !crate::object::shapes::is_shape_id_token(self.shape_addr)
    }

    // #854: in-progress typed-feedback shape-change tracking
    #[allow(dead_code)]
    fn affected_by_shape_change(&self, old_shape: usize, new_shape: usize, class_id: u32) -> bool {
        if !self.is_shape_keyed() {
            return false;
        }
        (old_shape != 0 && self.shape_addr == old_shape)
            || (new_shape != 0 && self.shape_addr == new_shape)
            || (old_shape == 0
                && self.shape_addr == 0
                && (class_id == 0 || self.class_id == 0 || self.class_id == class_id))
    }

    fn affected_by_representation_change(
        &self,
        obj_addr: usize,
        shape_addr: usize,
        class_id: u32,
        heap_type: u16,
    ) -> bool {
        if self.object_addr == obj_addr {
            return true;
        }
        if self.source == ObservationSource::Array {
            return heap_type != 0
                && self.heap_type == heap_type
                && (class_id == 0 || self.class_id == 0 || self.class_id == class_id);
        }
        if !self.is_shape_keyed() {
            return false;
        }
        if shape_addr != 0 {
            return self.shape_addr == shape_addr;
        }
        self.shape_addr == 0
            && (class_id == 0 || self.class_id == 0 || self.class_id == class_id)
            && (heap_type == 0 || self.heap_type == 0 || self.heap_type == heap_type)
    }
}

#[derive(Clone, Debug)]
struct SiteMetadata {
    kind: TypedFeedbackSiteKind,
    module: String,
    function: String,
    source_label: String,
    operation: String,
    guard_name: String,
    fallback_name: String,
}

#[derive(Clone, Debug)]
struct TypedFeedbackSite {
    site_id: u64,
    metadata: SiteMetadata,
    observations: Vec<Observation>,
    megamorphic: bool,
    observed_count: u64,
    guard_passes: u64,
    guard_failures: u64,
    fallback_calls: u64,
    shape_invalidations: u64,
    method_invalidations: u64,
    representation_invalidations: u64,
}

impl TypedFeedbackSite {
    fn new(site_id: u64, metadata: SiteMetadata) -> Self {
        Self {
            site_id,
            metadata,
            observations: Vec::new(),
            megamorphic: false,
            observed_count: 0,
            guard_passes: 0,
            guard_failures: 0,
            fallback_calls: 0,
            shape_invalidations: 0,
            method_invalidations: 0,
            representation_invalidations: 0,
        }
    }

    fn state(&self) -> TypedFeedbackState {
        if self.megamorphic {
            TypedFeedbackState::Megamorphic
        } else {
            match self.observations.len() {
                0 => TypedFeedbackState::Uninitialized,
                1 => TypedFeedbackState::Monomorphic,
                _ => TypedFeedbackState::Polymorphic,
            }
        }
    }

    fn observe(&mut self, observation: Observation) {
        self.observed_count = self.observed_count.saturating_add(1);
        if self.megamorphic
            || self
                .observations
                .iter()
                .any(|seen| seen.same_feedback_key(&observation))
        {
            return;
        }
        if self.observations.len() < POLYMORPHIC_CAP {
            self.observations.push(observation);
        } else {
            self.megamorphic = true;
        }
    }
}

#[derive(Default)]
struct TypedFeedbackRegistry {
    sites: HashMap<u64, TypedFeedbackSite>,
    shape_invalidations: u64,
    method_invalidations: u64,
    representation_invalidations: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TypedFeedbackSnapshot {
    pub total_sites: usize,
    pub by_kind: BTreeMap<String, u64>,
    pub by_state: BTreeMap<String, u64>,
    pub shape_invalidations: u64,
    pub method_invalidations: u64,
    pub representation_invalidations: u64,
    pub guard_passes: u64,
    pub guard_failures: u64,
    pub fallback_calls: u64,
    pub guards_by_name: BTreeMap<String, GuardCounterSnapshot>,
    pub sites: Vec<TypedFeedbackSiteSnapshot>,
}

#[derive(Debug, Clone)]
pub struct GuardCounterSnapshot {
    pub passes: u64,
    pub failures: u64,
    pub fallback_calls: u64,
}

impl GuardCounterSnapshot {
    fn add_site(&mut self, site: &TypedFeedbackSite) {
        self.passes = self.passes.saturating_add(site.guard_passes);
        self.failures = self.failures.saturating_add(site.guard_failures);
        self.fallback_calls = self.fallback_calls.saturating_add(site.fallback_calls);
    }
}

#[derive(Debug, Clone)]
pub struct TypedFeedbackSiteSnapshot {
    pub site_id: u64,
    pub kind: &'static str,
    pub state: &'static str,
    pub module: String,
    pub function: String,
    pub source_label: String,
    pub operation: String,
    pub guard_name: String,
    pub fallback_name: String,
    pub observed_count: u64,
    pub observation_count: usize,
    pub guard_passes: u64,
    pub guard_failures: u64,
    pub fallback_calls: u64,
    pub shape_invalidations: u64,
    pub method_invalidations: u64,
    pub representation_invalidations: u64,
    pub observed_kinds: Vec<serde_json::Value>,
}

fn read_static_str(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 || len > 16 * 1024 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

fn registry() -> crate::gc::GcRootRegistryGuard<'static, TypedFeedbackRegistry> {
    crate::gc::lock_gc_root_registry(&REGISTRY)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_register_site(
    site_id: u64,
    kind: u32,
    module_ptr: *const u8,
    module_len: usize,
    function_ptr: *const u8,
    function_len: usize,
    source_ptr: *const u8,
    source_len: usize,
    operation_ptr: *const u8,
    operation_len: usize,
    guard_ptr: *const u8,
    guard_len: usize,
    fallback_ptr: *const u8,
    fallback_len: usize,
) {
    if site_id == 0 || !typed_feedback_enabled() {
        return;
    }
    let metadata = SiteMetadata {
        kind: TypedFeedbackSiteKind::from_raw(kind),
        module: read_static_str(module_ptr, module_len),
        function: read_static_str(function_ptr, function_len),
        source_label: read_static_str(source_ptr, source_len),
        operation: read_static_str(operation_ptr, operation_len),
        guard_name: read_static_str(guard_ptr, guard_len),
        fallback_name: read_static_str(fallback_ptr, fallback_len),
    };
    let mut reg = registry();
    reg.sites
        .entry(site_id)
        .and_modify(|site| site.metadata = metadata.clone())
        .or_insert_with(|| TypedFeedbackSite::new(site_id, metadata));
}

fn value_tag(bits: u64) -> u16 {
    (bits >> 48) as u16
}

fn value_pointer(bits: u64) -> usize {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        (bits & POINTER_MASK) as usize
    } else {
        0
    }
}

const STABLE_VALUE_NUMBER: u16 = 1;
const STABLE_VALUE_BOOLEAN: u16 = 2;
const STABLE_VALUE_NULL: u16 = 3;
const STABLE_VALUE_UNDEFINED: u16 = 4;
const STABLE_VALUE_HOLE: u16 = 5;
const STABLE_VALUE_SHORT_STRING: u16 = 6;
const STABLE_VALUE_STRING: u16 = 7;
const STABLE_VALUE_BIGINT: u16 = 8;
const STABLE_VALUE_POINTER: u16 = 9;
const STABLE_VALUE_INT32: u16 = 10;
const STABLE_VALUE_JS_HANDLE: u16 = 11;

const ARRAY_ACCESS_UNKNOWN: u8 = 0;
const ARRAY_ACCESS_INDEXED_IN_BOUNDS: u8 = 1;
const ARRAY_ACCESS_INDEXED_OUT_OF_BOUNDS: u8 = 2;
const ARRAY_ACCESS_STRING_KEY: u8 = 3;

const ARRAY_LAYOUT_INVALID: u8 = 0;
const ARRAY_LAYOUT_EMPTY: u8 = 1;
const ARRAY_LAYOUT_POINTER_FREE: u8 = 2;
const ARRAY_LAYOUT_POINTER_ONLY: u8 = 3;
const ARRAY_LAYOUT_MIXED: u8 = 4;
const ARRAY_LAYOUT_UNKNOWN: u8 = 5;
const ARRAY_LAYOUT_BUFFER: u8 = 6;
const ARRAY_LAYOUT_TYPED_ARRAY: u8 = 7;
const ARRAY_LAYOUT_LAZY: u8 = 8;

fn stable_value_kind(bits: u64) -> u16 {
    match bits {
        TAG_TRUE | TAG_FALSE => return STABLE_VALUE_BOOLEAN,
        TAG_NULL => return STABLE_VALUE_NULL,
        TAG_UNDEFINED => return STABLE_VALUE_UNDEFINED,
        TAG_HOLE => return STABLE_VALUE_HOLE,
        _ => {}
    }

    match bits & TAG_MASK {
        POINTER_TAG => STABLE_VALUE_POINTER,
        STRING_TAG => STABLE_VALUE_STRING,
        BIGINT_TAG => STABLE_VALUE_BIGINT,
        JS_HANDLE_TAG => STABLE_VALUE_JS_HANDLE,
        SHORT_STRING_TAG => STABLE_VALUE_SHORT_STRING,
        INT32_TAG => STABLE_VALUE_INT32,
        _ => STABLE_VALUE_NUMBER,
    }
}

fn raw_heap_type(addr: usize) -> u16 {
    if addr == 0 {
        return 0;
    }
    if crate::buffer::is_registered_buffer(addr) {
        return crate::gc::GC_TYPE_BUFFER as u16;
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return crate::gc::GC_TYPE_TYPED_ARRAY as u16;
    }
    if !crate::object::is_valid_obj_ptr(addr as *const u8) {
        return 0;
    }
    unsafe {
        let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc).obj_type;
        if crate::gc::gc_type_info(gc_type).is_some() {
            gc_type as u16
        } else {
            0
        }
    }
}

fn pack_array_aux(access_kind: u8, layout_kind: u8, element_kind: u16, typed_kind: u8) -> u64 {
    (access_kind as u64)
        | ((layout_kind as u64) << 8)
        | ((element_kind as u64) << 16)
        | ((typed_kind as u64) << 32)
}

fn array_layout_kind(addr: usize, len: u64) -> u8 {
    if len == 0 {
        return ARRAY_LAYOUT_EMPTY;
    }

    let mut pointer_slots = 0usize;
    if crate::gc::layout_visit_pointer_slots_for_user(addr, len as usize, |_| {
        pointer_slots = pointer_slots.saturating_add(1);
    }) {
        if pointer_slots == 0 {
            ARRAY_LAYOUT_POINTER_FREE
        } else if pointer_slots as u64 == len {
            ARRAY_LAYOUT_POINTER_ONLY
        } else {
            ARRAY_LAYOUT_MIXED
        }
    } else {
        ARRAY_LAYOUT_UNKNOWN
    }
}

fn array_element_kind(addr: usize, index: Option<u32>, len: u64, layout_kind: u8) -> u16 {
    let Some(index) = index else {
        return layout_kind as u16;
    };
    if index as u64 >= len || len > 16_000_000 {
        return STABLE_VALUE_UNDEFINED;
    }
    unsafe {
        let elements = (addr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
        stable_value_kind(*elements.add(index as usize))
    }
}

fn classify_array(addr: usize, index: Option<u32>) -> (u32, u16, u64, u16) {
    if addr == 0 {
        return (
            0,
            0,
            pack_array_aux(
                ARRAY_ACCESS_UNKNOWN,
                ARRAY_LAYOUT_INVALID,
                STABLE_VALUE_UNDEFINED,
                0,
            ),
            STABLE_VALUE_UNDEFINED,
        );
    }

    let access_kind = match index {
        None => ARRAY_ACCESS_UNKNOWN,
        Some(u32::MAX) => ARRAY_ACCESS_STRING_KEY,
        Some(_) => ARRAY_ACCESS_INDEXED_IN_BOUNDS,
    };

    if crate::buffer::is_registered_buffer(addr) {
        let len = unsafe { (*(addr as *const crate::buffer::BufferHeader)).length as u64 };
        let access_kind = match index {
            Some(i) if i != u32::MAX && i as u64 >= len => ARRAY_ACCESS_INDEXED_OUT_OF_BOUNDS,
            _ => access_kind,
        };
        let element_kind = STABLE_VALUE_NUMBER;
        return (
            crate::buffer::BUFFER_TYPE_ID,
            crate::gc::GC_TYPE_BUFFER as u16,
            pack_array_aux(access_kind, ARRAY_LAYOUT_BUFFER, element_kind, 0),
            element_kind,
        );
    }

    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        let len = unsafe { (*(addr as *const crate::typedarray::TypedArrayHeader)).length as u64 };
        let access_kind = match index {
            Some(i) if i != u32::MAX && i as u64 >= len => ARRAY_ACCESS_INDEXED_OUT_OF_BOUNDS,
            _ => access_kind,
        };
        let element_kind = STABLE_VALUE_NUMBER;
        return (
            crate::typedarray::class_id_for_kind(kind),
            crate::gc::GC_TYPE_TYPED_ARRAY as u16,
            pack_array_aux(access_kind, ARRAY_LAYOUT_TYPED_ARRAY, element_kind, kind),
            element_kind,
        );
    }

    if !crate::object::is_valid_obj_ptr(addr as *const u8) {
        return (
            0,
            0,
            pack_array_aux(
                ARRAY_ACCESS_UNKNOWN,
                ARRAY_LAYOUT_INVALID,
                STABLE_VALUE_UNDEFINED,
                0,
            ),
            STABLE_VALUE_UNDEFINED,
        );
    }

    unsafe {
        let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc).obj_type;
        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            return (
                0,
                gc_type as u16,
                pack_array_aux(access_kind, ARRAY_LAYOUT_LAZY, STABLE_VALUE_POINTER, 0),
                STABLE_VALUE_POINTER,
            );
        }
        if (*gc).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return (
                0,
                gc_type as u16,
                pack_array_aux(access_kind, ARRAY_LAYOUT_INVALID, STABLE_VALUE_POINTER, 0),
                STABLE_VALUE_POINTER,
            );
        }
        if gc_type != crate::gc::GC_TYPE_ARRAY {
            return (
                0,
                gc_type as u16,
                pack_array_aux(
                    ARRAY_ACCESS_UNKNOWN,
                    ARRAY_LAYOUT_INVALID,
                    STABLE_VALUE_POINTER,
                    0,
                ),
                STABLE_VALUE_POINTER,
            );
        }

        let len = (*(addr as *const ArrayHeader)).length as u64;
        let access_kind = match index {
            Some(i) if i != u32::MAX && i as u64 >= len => ARRAY_ACCESS_INDEXED_OUT_OF_BOUNDS,
            _ => access_kind,
        };
        let layout_kind = array_layout_kind(addr, len);
        let element_kind =
            array_element_kind(addr, index.filter(|i| *i != u32::MAX), len, layout_kind);
        (
            0,
            gc_type as u16,
            pack_array_aux(access_kind, layout_kind, element_kind, 0),
            element_kind,
        )
    }
}

fn helper_return_facts(bits: u64) -> (usize, u32, u16, u64, u16) {
    let value_kind = stable_value_kind(bits);
    let addr = value_pointer(bits);
    if addr == 0 {
        return (0, 0, 0, 0, value_kind);
    }

    if crate::buffer::is_registered_buffer(addr)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some()
    {
        let (class_id, heap_type, aux, element_kind) = classify_array(addr, None);
        return (0, class_id, heap_type, aux, element_kind);
    }

    match raw_heap_type(addr) as u8 {
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => {
            let (class_id, heap_type, aux, element_kind) = classify_array(addr, None);
            (0, class_id, heap_type, aux, element_kind)
        }
        crate::gc::GC_TYPE_OBJECT => {
            let (shape_addr, class_id, heap_type) = object_shape(addr);
            (shape_addr, class_id, heap_type, 0, value_kind)
        }
        heap_type => (0, 0, heap_type as u16, 0, value_kind),
    }
}

fn normalize_raw_object_addr(bits: u64) -> usize {
    let top = bits >> 48;
    let addr = if top >= 0x7FF8 {
        // NaN-boxed: only POINTER/STRING/BIGINT tags carry a real, dereferenceable
        // heap address. SHORT_STRING/INT32/primitive/handle pack INLINE data in
        // the low 48 bits — masking that to an "address" and probing
        // `addr - header` as a GC object faults. A short (SSO) string receiver
        // such as `email = "jo"` (inline bytes `0x..6f6a`) reaching
        // `email.includes("@")` through typed-feedback method dispatch otherwise
        // dereferences those bytes as a pointer (EXC_BAD_ACCESS in
        // `object_shape`). Mirrors the heap-pointer check at `value_tag` /
        // `gc::barrier::decode_heap_addr`. (Heap-allocated receivers hide this;
        // small-string-optimized values such as `JSON.parse(...).value` expose
        // it.)
        let tag = bits & TAG_MASK;
        if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
            return 0;
        }
        bits & POINTER_MASK
    } else {
        bits
    } as usize;
    // Native module registry handles are carried as small raw values in several
    // dispatch paths. They are not GC objects, and probing `addr - header_size`
    // for them can fault before the generic native-handle dispatcher runs.
    if crate::value::addr_class::is_handle_band(addr) || (addr as u64) >> 48 != 0 {
        0
    } else {
        addr
    }
}

fn object_shape(addr: usize) -> (usize, u32, u16) {
    if addr == 0 {
        return (0, 0, 0);
    }
    let ptr = addr as *const ObjectHeader;
    if !crate::object::is_valid_obj_ptr(ptr as *const u8) {
        return (0, 0, 0);
    }
    unsafe {
        let gc = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc).obj_type as u16;
        if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
            return (0, 0, gc_type);
        }
        let class_id = (*ptr).class_id;
        // #6804: plain objects canonicalize the token on the stable
        // ShapeId. Shape-cached literals are stamped at birth; anything
        // else is stamped HERE on first observation (self-healing), so one
        // logical shape can never split into a pre-stamp address token and
        // a post-stamp id token within a site. Class instances keep the
        // keys-address token (their `parent_class_id` is inheritance data).
        let shape = if class_id == 0 {
            let stamp = (*ptr).parent_class_id;
            if crate::object::shapes::is_shape_id(stamp) {
                stamp as usize
            } else if crate::regex::regex_header_has_magic(
                addr as *const crate::regex::RegExpHeader,
            ) {
                // RegExpHeader aliases GC_TYPE_OBJECT with a different
                // layout — never write through the ObjectHeader view; keep
                // the legacy (equality-only) address token.
                (*ptr).keys_array as usize
            } else {
                let keys = (*ptr).keys_array;
                if let Some(keys_header) =
                    crate::value::addr_class::try_read_gc_header(keys as usize)
                {
                    if keys_header.obj_type == crate::gc::GC_TYPE_ARRAY
                        || keys_header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                    {
                        let id =
                            crate::object::shapes::shape_id_for_keys_ensure(keys, (*keys).length);
                        if id != 0 {
                            (*(ptr as *mut ObjectHeader)).parent_class_id = id;
                            id as usize
                        } else {
                            keys as usize
                        }
                    } else {
                        keys as usize
                    }
                } else {
                    keys as usize
                }
            }
        } else {
            (*ptr).keys_array as usize
        };
        (shape, class_id, gc_type)
    }
}

#[cfg(test)]
pub(crate) fn test_object_shape_token(addr: usize) -> usize {
    object_shape(addr).0
}

fn observe(site_id: u64, fallback_kind: TypedFeedbackSiteKind, observation: Observation) {
    if site_id == 0 {
        return;
    }
    let mut reg = registry();
    let site = reg.sites.entry(site_id).or_insert_with(|| {
        TypedFeedbackSite::new(
            site_id,
            SiteMetadata {
                kind: fallback_kind,
                module: String::new(),
                function: String::new(),
                source_label: String::new(),
                operation: String::new(),
                guard_name: String::new(),
                fallback_name: String::new(),
            },
        )
    });
    site.observe(observation);
}

fn site_entry(
    reg: &mut TypedFeedbackRegistry,
    site_id: u64,
    fallback_kind: TypedFeedbackSiteKind,
) -> &mut TypedFeedbackSite {
    reg.sites.entry(site_id).or_insert_with(|| {
        TypedFeedbackSite::new(
            site_id,
            SiteMetadata {
                kind: fallback_kind,
                module: String::new(),
                function: String::new(),
                source_label: String::new(),
                operation: String::new(),
                guard_name: String::new(),
                fallback_name: String::new(),
            },
        )
    })
}

fn guard_observe(
    site_id: u64,
    fallback_kind: TypedFeedbackSiteKind,
    observation: Observation,
    contract_valid: bool,
) -> bool {
    if site_id == 0 || !typed_feedback_enabled() {
        return contract_valid;
    }
    let mut reg = registry();
    let site = site_entry(&mut reg, site_id, fallback_kind);
    let guard_passed = contract_valid
        && !site.megamorphic
        && (site.observations.is_empty()
            || site
                .observations
                .iter()
                .any(|seen| seen.same_feedback_key(&observation)));
    if guard_passed {
        site.guard_passes = site.guard_passes.saturating_add(1);
    } else {
        site.guard_failures = site.guard_failures.saturating_add(1);
    }
    site.observe(observation);
    guard_passed
}

fn record_guard_pass(site_id: u64) {
    if site_id == 0 || !typed_feedback_enabled() {
        return;
    }
    let mut reg = registry();
    if let Some(site) = reg.sites.get_mut(&site_id) {
        site.guard_passes = site.guard_passes.saturating_add(1);
    }
}

fn record_guard_fail(site_id: u64) {
    if site_id == 0 || !typed_feedback_enabled() {
        return;
    }
    let mut reg = registry();
    if let Some(site) = reg.sites.get_mut(&site_id) {
        site.guard_failures = site.guard_failures.saturating_add(1);
    }
}

fn record_fallback_call(site_id: u64) {
    if site_id == 0 || !typed_feedback_enabled() {
        return;
    }
    let mut reg = registry();
    if let Some(site) = reg.sites.get_mut(&site_id) {
        site.fallback_calls = site.fallback_calls.saturating_add(1);
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_record_guard_pass(site_id: u64) {
    record_guard_pass(site_id);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_record_guard_fail(site_id: u64) {
    record_guard_fail(site_id);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_record_fallback_call(site_id: u64) {
    record_fallback_call(site_id);
}

fn observe_property(
    site_id: u64,
    kind: TypedFeedbackSiteKind,
    obj_bits: u64,
    key: *const crate::StringHeader,
) {
    if site_id == 0 || !typed_feedback_enabled() {
        return;
    }
    let object_addr = normalize_raw_object_addr(obj_bits);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    observe(
        site_id,
        kind,
        Observation {
            source: ObservationSource::Property,
            object_addr: shape_keyed_object_addr(ObservationSource::Property, object_addr),
            shape_addr,
            key_hash: key_hash(key),
            class_id,
            heap_type: gc_type,
            aux: 0,
            value_tag: value_tag(obj_bits),
        },
    );
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_observe_property_get(
    site_id: u64,
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) {
    observe_property(site_id, TypedFeedbackSiteKind::PropertyGet, obj as u64, key);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_observe_property_set(
    site_id: u64,
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    observe_property(site_id, TypedFeedbackSiteKind::PropertySet, obj as u64, key);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_object_get_field_by_name_f64(
    site_id: u64,
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let observation = Observation {
        source: ObservationSource::Property,
        object_addr: shape_keyed_object_addr(ObservationSource::Property, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: 0,
        value_tag: value_tag(obj as u64),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::PropertyGet,
        observation,
        valid_string_key(key) && gc_type == crate::gc::GC_TYPE_OBJECT as u16,
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::object::js_object_get_field_by_name_f64(obj, key)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_object_set_field_by_name(
    site_id: u64,
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let observation = Observation {
        source: ObservationSource::Property,
        object_addr: shape_keyed_object_addr(ObservationSource::Property, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: 0,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::PropertySet,
        observation,
        valid_string_key(key) && gc_type == crate::gc::GC_TYPE_OBJECT as u16,
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::object::js_object_set_field_by_name(obj, key, value);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_object_set_field_by_name_fast(
    site_id: u64,
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if !typed_feedback_enabled() {
        if crate::object::js_object_set_field_by_name_transition_fast(obj, key, value) == 0 {
            crate::object::js_object_set_field_by_name(obj, key, value);
        }
        return;
    }
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let handled = crate::object::js_object_set_field_by_name_transition_fast(obj, key, value) != 0;
    let observation = Observation {
        source: ObservationSource::Property,
        object_addr: shape_keyed_object_addr(ObservationSource::Property, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: 0,
        value_tag: stable_value_kind(value.to_bits()),
    };
    guard_observe(
        site_id,
        TypedFeedbackSiteKind::PropertySet,
        observation,
        handled,
    );
    if !handled {
        record_fallback_call(site_id);
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
}

#[path = "typed_feedback/guards.rs"]
mod guards;
pub use guards::{
    js_typed_feedback_class_field_get_guard, js_typed_feedback_class_field_set_guard,
    js_typed_feedback_closure_direct_call_guard, js_typed_feedback_method_direct_call_guard,
    js_typed_feedback_native_call_method, js_typed_feedback_native_call_method_apply,
};

#[path = "typed_feedback/trace.rs"]
mod trace;
pub use trace::typed_feedback_snapshot;
#[cfg(feature = "diagnostics")]
pub use trace::{js_typed_feedback_maybe_dump_trace, typed_feedback_trace_json};

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn key_hash(key: *const crate::StringHeader) -> u64 {
    if key.is_null() || (key as usize) < 0x1000 {
        return 0;
    }
    unsafe {
        let len = (*key).byte_len as usize;
        if len > 4096 {
            return 0;
        }
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        hash_bytes(std::slice::from_raw_parts(data, len))
    }
}

fn valid_string_key(key: *const crate::StringHeader) -> bool {
    if key.is_null() || (key as usize) < 0x1000 {
        return false;
    }
    unsafe {
        let len = (*key).byte_len as usize;
        len <= 4096
    }
}

fn valid_method_name(method_name_ptr: *const i8, method_name_len: usize) -> bool {
    !method_name_ptr.is_null() && method_name_len > 0 && method_name_len <= 4096
}

fn method_name_bytes<'a>(method_name_ptr: *const i8, method_name_len: usize) -> Option<&'a [u8]> {
    if !valid_method_name(method_name_ptr, method_name_len) {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len) })
}

fn method_name_str<'a>(method_name_ptr: *const i8, method_name_len: usize) -> Option<&'a str> {
    std::str::from_utf8(method_name_bytes(method_name_ptr, method_name_len)?).ok()
}

fn is_plain_number_bits(bits: u64) -> bool {
    stable_value_kind(bits) == STABLE_VALUE_NUMBER
}

fn is_numeric_value_bits(bits: u64) -> bool {
    crate::array::value_bits_to_number(bits).is_some()
}

fn finite_nonnegative_u32_index(index: f64) -> Option<u32> {
    let bits = index.to_bits();
    if (bits & TAG_MASK) == INT32_TAG {
        let index = crate::value::JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index as u32);
    }
    if index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index < u32::MAX as f64 {
        Some(index as u32)
    } else {
        None
    }
}

fn finite_nonnegative_i32_index(index: f64) -> Option<i32> {
    let bits = index.to_bits();
    if (bits & TAG_MASK) == INT32_TAG {
        let index = crate::value::JSValue::from_bits(bits).as_int32();
        return (index >= 0).then_some(index);
    }
    if index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index <= i32::MAX as f64 {
        Some(index as i32)
    } else {
        None
    }
}

/// INVARIANT (load-bearing): every array-like receiver reaching this must carry
/// a real GcHeader. The address checks below are coarse (band floor, canonical
/// form, heap range) — none of them proves a header is actually there, and the
/// `addr - GC_HEADER_SIZE` back-read is taken on faith. Reintroducing an
/// off-GC-heap allocation tier for small typed arrays or buffers (the pre-#6190
/// raw-`alloc` tier under the 16 KB threshold) therefore does not fault: it
/// silently reads whatever heap bytes precede the block, and when they happen to
/// look like `GC_TYPE_ARRAY` the caller admits a typed array to the inline
/// plain-`ArrayHeader` raw-slot path, which reinterprets its elements as f64
/// denormals and sums them as zero (#6136; #6190 made every typed array/buffer a
/// GC-heap object). Keep that invariant or replace this with an exact registry
/// lookup — do not weaken it by adding address heuristics here.
fn gc_header_for_user_addr(addr: usize) -> Option<*const crate::gc::GcHeader> {
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000
        || (addr as u64) >> 48 != 0
        || !crate::object::is_valid_obj_ptr(addr as *const u8)
    {
        return None;
    }
    Some(unsafe {
        (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
    })
}

fn plain_array_index_guard(arr: *const ArrayHeader, index: u32, require_in_bounds: bool) -> bool {
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        // #6518 audit note: the GC_FLAG_FORWARDED arm below is load-bearing
        // for the stale-grown-pointer family (#6486). A caller-held pre-grow
        // pointer left by `js_array_grow` (#233) is rejected HERE, before
        // the raw length/capacity read below — the `len > cap` sanity check
        // alone is not a reliable defense against forwarding-pointer bit
        // patterns. Same applies to the sibling check in
        // `numeric_array_push_guard`. Do not remove either arm without
        // routing the guard through `clean_arr_ptr`.
        if (*header).obj_type != crate::gc::GC_TYPE_ARRAY
            || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        // Index accessors / custom attribute descriptors divert element
        // reads and writes through the descriptor tables; the inline
        // raw-slot fast path the guard admits would bypass them (test262
        // sort/precise-* read accessor indices after defineProperty).
        if (*header)._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
            return false;
        }
        // A polluted `Array.prototype[i]` (or custom array prototype) makes
        // holes read through the chain — the raw slot load would return
        // undefined instead (test262 concat/S15.4.4.4_A3_T2,
        // copyWithin/coerced-values-start-change-*). Rare global flags;
        // two relaxed atomic loads.
        if crate::array::array_prototype_has_index_flag()
            || crate::array::object_prototype_has_index_flag()
            || crate::object::prototype_chain::array_static_proto_recorded()
        {
            return false;
        }
        let arr = raw_addr as *const ArrayHeader;
        let len = (*arr).length;
        let cap = (*arr).capacity;
        if len > 16_000_000 || cap > 16_000_000 || len > cap {
            return false;
        }
        !require_in_bounds || index < len
    }
}

fn numeric_array_index_guard(arr: *const ArrayHeader, index: u32, require_in_bounds: bool) -> bool {
    if !plain_array_index_guard(arr, index, require_in_bounds) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    // Numeric arrays produced by literals/push already carry the raw-layout
    // bit. Read it from the header that the plain-array guard just validated
    // instead of entering `js_array_is_numeric_f64_layout`, which cleans the
    // pointer and re-reads the header on every element access. Preserve the
    // slower verify/rewrite path for arrays not marked yet.
    unsafe {
        if (*header)._reserved & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0 {
            true
        } else {
            crate::array::js_array_is_numeric_f64_layout(raw_addr as *const ArrayHeader) != 0
        }
    }
}

fn plain_array_index_set_guard(
    arr: *const ArrayHeader,
    index: u32,
    require_in_bounds: bool,
) -> bool {
    if !plain_array_index_guard(arr, index, require_in_bounds) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        let flags = (*header)._reserved;
        if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return false;
        }
        let arr = raw_addr as *const ArrayHeader;
        if index >= (*arr).length
            && flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0
        {
            return false;
        }
    }
    true
}

fn numeric_array_index_set_guard(
    arr: *const ArrayHeader,
    index: u32,
    require_in_bounds: bool,
) -> bool {
    if !plain_array_index_set_guard(arr, index, require_in_bounds) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        if (*header)._reserved & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0 {
            true
        } else {
            crate::array::js_array_is_numeric_f64_layout(raw_addr as *const ArrayHeader) != 0
        }
    }
}

fn packed_f64_array_loop_guard(arr: *const ArrayHeader) -> bool {
    if !plain_array_index_guard(arr, 0, false) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        let flags = (*header)._reserved;
        if flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND)
            != 0
        {
            return false;
        }
        if (*header).obj_type == crate::gc::GC_TYPE_ARRAY
            && (*(raw_addr as *const ArrayHeader)).length > i32::MAX as u32
        {
            return false;
        }
    }
    crate::array::js_array_is_numeric_f64_layout(raw_addr as *const ArrayHeader) != 0
}

/// #6011: entry guard for the packed-f64 *range* versioned loop. Validates the
/// same plain-array shape as [`packed_f64_array_loop_guard`], then proves the
/// whole static index range `[min_idx, max_idx_exclusive)` the loop can touch
/// is inside the array, and finally normalizes the slots hole-tolerantly:
/// numeric slots are rewritten to raw f64 while `TAG_HOLE` slots stay in place
/// (the guarded loop's inline loads hole-check and side-exit to the slow loop).
/// Any slot that is neither numeric nor a hole fails the guard.
fn packed_f64_array_loop_range_guard(
    arr: *const ArrayHeader,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> bool {
    if !plain_array_index_guard(arr, 0, false) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        let flags = (*header)._reserved;
        if flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND)
            != 0
        {
            return false;
        }
        let arr = raw_addr as *mut ArrayHeader;
        let len = (*arr).length;
        if len > i32::MAX as u32 {
            return false;
        }
        if min_idx < 0 || i64::from(max_idx_exclusive) > i64::from(len) {
            return false;
        }
        crate::array::rebuild_array_numeric_raw_f64_allow_holes(arr)
    }
}

/// Dense-window variant of [`packed_f64_array_loop_range_guard`] for the
/// read-only masked-index range loop: identical shape/window validation, but
/// the window must additionally be hole-free (the guarded loop's inline loads
/// carry no hole check and no side exit — its multi-statement body cannot be
/// safely re-executed mid-iteration).
fn packed_f64_array_loop_range_guard_dense(
    arr: *const ArrayHeader,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> bool {
    if !plain_array_index_guard(arr, 0, false) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        let flags = (*header)._reserved;
        if flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND)
            != 0
        {
            return false;
        }
        let arr = raw_addr as *mut ArrayHeader;
        let len = (*arr).length;
        if len > i32::MAX as u32 {
            return false;
        }
        if min_idx < 0 || i64::from(max_idx_exclusive) > i64::from(len) {
            return false;
        }
        crate::array::rebuild_array_numeric_raw_f64_dense_window(arr, min_idx, max_idx_exclusive)
    }
}

/// i32 tier of [`packed_f64_array_loop_range_guard_dense`]: the window must
/// additionally hold only i32-representable integers, so the guarded loop's
/// inline loads can use a bare exact `fptosi`.
fn packed_f64_array_loop_range_guard_dense_i32(
    arr: *const ArrayHeader,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> bool {
    if !packed_f64_array_loop_range_guard_dense(arr, min_idx, max_idx_exclusive) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    unsafe {
        crate::array::rebuild_array_numeric_raw_f64_dense_window_i32(
            raw_addr as *mut ArrayHeader,
            min_idx,
            max_idx_exclusive,
        )
    }
}

fn packed_f64_range_dense_guard_impl(
    site_id: u64,
    receiver: f64,
    min_idx: i32,
    max_idx_exclusive: i32,
    guard: fn(*const ArrayHeader, i32, i32) -> bool,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return guard(raw_addr as *const ArrayHeader, min_idx, max_idx_exclusive) as i32;
    }
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        guard(raw_addr as *const ArrayHeader, min_idx, max_idx_exclusive),
    );
    if pass {
        1
    } else {
        0
    }
}

/// FFI wrapper for [`packed_f64_array_loop_range_guard_dense`] — the entry
/// guard of the read-only masked-index packed-f64 range loop (f64 tier).
#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_f64_range_loop_guard_dense(
    site_id: u64,
    receiver: f64,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> i32 {
    packed_f64_range_dense_guard_impl(
        site_id,
        receiver,
        min_idx,
        max_idx_exclusive,
        packed_f64_array_loop_range_guard_dense,
    )
}

/// FFI wrapper for [`packed_f64_array_loop_range_guard_dense_i32`] — the i32
/// tier of the dense range-loop guard.
#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_f64_range_loop_guard_dense_i32(
    site_id: u64,
    receiver: f64,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> i32 {
    packed_f64_range_dense_guard_impl(
        site_id,
        receiver,
        min_idx,
        max_idx_exclusive,
        packed_f64_array_loop_range_guard_dense_i32,
    )
}

/// Kind codes returned by [`js_typed_feedback_masked_window_ta_kind`]. The
/// codegen tier dispatch branches on these exact values — keep in sync with
/// the masked-window TA tiers in `perry-codegen/src/stmt/loops.rs`.
pub const MASKED_WINDOW_TA_KIND_NONE: i32 = 0;
pub const MASKED_WINDOW_TA_KIND_I32: i32 = 1;
pub const MASKED_WINDOW_TA_KIND_U32: i32 = 2;
pub const MASKED_WINDOW_TA_KIND_F64: i32 = 3;

/// Masked-window typed-array probe (#6750 follow-up): classify a receiver
/// whose static type the compiler could not prove (an `any` function
/// parameter — the bcryptjs S-box shape) as a typed array whose whole static
/// index window `[min_idx, max_idx_exclusive)` is in bounds. O(1): a registry
/// lookup plus a length compare — no window scan, so re-entering a short hot
/// loop (one probe per accessed array per entry) stays cheap.
///
/// A view over a detached ArrayBuffer has `length == 0`
/// (`zero_views_of_detached_backing`), so the window check also rejects
/// detached backings. Kinds outside {Int32, Uint32, Float64} return NONE and
/// fall through to the plain-array guard tiers / the slow loop.
fn masked_window_ta_kind(addr: usize, min_idx: i32, max_idx_exclusive: i32) -> i32 {
    if min_idx < 0 {
        return MASKED_WINDOW_TA_KIND_NONE;
    }
    let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) else {
        return MASKED_WINDOW_TA_KIND_NONE;
    };
    let code = match kind {
        crate::typedarray::KIND_INT32 => MASKED_WINDOW_TA_KIND_I32,
        crate::typedarray::KIND_UINT32 => MASKED_WINDOW_TA_KIND_U32,
        crate::typedarray::KIND_FLOAT64 => MASKED_WINDOW_TA_KIND_F64,
        _ => return MASKED_WINDOW_TA_KIND_NONE,
    };
    let len = unsafe { (*(addr as *const crate::typedarray::TypedArrayHeader)).length };
    if i64::from(max_idx_exclusive) > i64::from(len) {
        return MASKED_WINDOW_TA_KIND_NONE;
    }
    code
}

/// FFI wrapper for [`masked_window_ta_kind`] — the typed-array tier probe of
/// the read-only masked-index range loop. Returns the `MASKED_WINDOW_TA_KIND_*`
/// code; the codegen requires every accessed array to probe to the same
/// non-NONE code before entering the matching typed-array fast copy.
#[no_mangle]
pub extern "C" fn js_typed_feedback_masked_window_ta_kind(
    site_id: u64,
    receiver: f64,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    let code = masked_window_ta_kind(raw_addr, min_idx, max_idx_exclusive);
    if typed_feedback_enabled() {
        let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
        let observation = Observation {
            source: ObservationSource::Array,
            object_addr: 0,
            shape_addr: 0,
            key_hash: 0,
            class_id,
            heap_type,
            aux,
            value_tag: element_kind,
        };
        guard_observe(
            site_id,
            TypedFeedbackSiteKind::ArrayElement,
            observation,
            code != MASKED_WINDOW_TA_KIND_NONE,
        );
    }
    code
}

fn packed_i32_array_loop_guard(arr: *const ArrayHeader) -> bool {
    if !packed_f64_array_loop_guard(arr) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    unsafe {
        let arr = raw_addr as *const ArrayHeader;
        let len = (*arr).length as usize;
        if len > 16_000_000 {
            return false;
        }
        let elements =
            (raw_addr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..len {
            let value = *elements.add(i);
            if !value.is_finite()
                || value.fract() != 0.0
                || value < i32::MIN as f64
                || value > i32::MAX as f64
            {
                return false;
            }
        }
    }
    true
}

fn packed_u32_array_loop_guard(arr: *const ArrayHeader) -> bool {
    if !packed_f64_array_loop_guard(arr) {
        return false;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    unsafe {
        let arr = raw_addr as *const ArrayHeader;
        let len = (*arr).length as usize;
        if len > 16_000_000 {
            return false;
        }
        let elements =
            (raw_addr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..len {
            let value = *elements.add(i);
            if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value > u32::MAX as f64
            {
                return false;
            }
        }
    }
    true
}

fn numeric_array_push_guard(arr: *const ArrayHeader, value: f64) -> bool {
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let Some(header) = gc_header_for_user_addr(raw_addr) else {
        return false;
    };
    unsafe {
        if (*header).obj_type != crate::gc::GC_TYPE_ARRAY
            || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        let arr = raw_addr as *const ArrayHeader;
        let len = (*arr).length;
        let cap = (*arr).capacity;
        let flags = (*header)._reserved;
        if flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
            != 0
        {
            return false;
        }
        if crate::object::get_property_attrs(raw_addr, "length")
            .map(|attrs| !attrs.writable())
            .unwrap_or(false)
        {
            return false;
        }
        len <= 16_000_000
            && cap <= 16_000_000
            && len < cap
            && is_numeric_value_bits(value.to_bits())
            && crate::array::js_array_is_numeric_f64_layout(arr) != 0
    }
}

fn object_key_matches_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    field_index: u32,
) -> bool {
    if !valid_string_key(key) {
        return false;
    }
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, _, heap_type) = object_shape(object_addr);
    if heap_type != crate::gc::GC_TYPE_OBJECT as u16 || shape_addr == 0 {
        return false;
    }
    unsafe {
        let obj = object_addr as *mut ObjectHeader;
        let alloc_limit =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32);
        if field_index >= alloc_limit {
            return false;
        }
        let keys = (*obj).keys_array;
        // #6804: `shape_addr` is an opaque TOKEN (a stable ShapeId for
        // stamped plain objects), not necessarily the keys address — the
        // actual contract is carried by the key/slot validation below.
        if keys.is_null() {
            return false;
        }
        if !plain_array_index_guard(keys, field_index, true) {
            return false;
        }
        let stored = crate::array::js_array_get(keys, field_index);
        stored.is_string()
            && !stored.as_string_ptr().is_null()
            && crate::string::js_string_equals(key, stored.as_string_ptr()) != 0
    }
}

fn shape_keyed_object_addr(source: ObservationSource, object_addr: usize) -> usize {
    if matches!(
        source,
        ObservationSource::Property | ObservationSource::Method | ObservationSource::NumericWrite
    ) {
        0
    } else {
        object_addr
    }
}

fn observe_array(site_id: u64, arr: *const ArrayHeader, index: u32) {
    // #5094: recording-only helper. When typed-feedback is off (the default),
    // `observe` records nothing, so the whole body is dead work — and
    // `classify_array` walks the per-object pointer-slot layout, which is O(len)
    // for a downgraded (SIDE_MASK) array. This ran per `arr[i] = v` on a
    // heterogeneous array (via `js_typed_feedback_array_set_index_or_string` and
    // the polymorphic / string-key set wrappers), making the write loop
    // quadratic. Bail before the walk, matching every other recording path.
    if !typed_feedback_enabled() {
        return;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, Some(index));
    observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        Observation {
            source: ObservationSource::Array,
            object_addr: 0,
            shape_addr: 0,
            key_hash: 0,
            class_id,
            heap_type,
            aux,
            value_tag: element_kind,
        },
    );
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_get_f64(
    site_id: u64,
    arr: *const ArrayHeader,
    index: u32,
) -> f64 {
    // #5094: when typed-feedback recording is off (the default — `guard_observe`
    // early-returns with no side effect), the whole classify/observe block is
    // dead work. `classify_array` walks the per-object pointer-slot layout
    // (`array_layout_kind` → `layout_visit_pointer_slots`), which is O(len) for a
    // downgraded (SIDE_MASK) array — so this ran per element and made
    // `arr[i]` on a heterogeneous array quadratic. Mirror the gate the sibling
    // guard wrappers already carry (`plain_array_index_get_guard_impl`) and take
    // the underlying array op directly.
    if !typed_feedback_enabled() {
        return crate::array::js_array_get_f64(arr, index);
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, Some(index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        plain_array_index_guard(arr, index, true),
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::array::js_array_get_f64(arr, index)
}

#[no_mangle]
// i32-index guard (see `js_typed_feedback_numeric_array_index_get_guard`).
pub extern "C" fn js_typed_feedback_plain_array_index_get_guard(
    site_id: u64,
    receiver: f64,
    index: i32,
    require_in_bounds: i32,
) -> i32 {
    plain_array_index_get_guard_impl(site_id, receiver, true, index, require_in_bounds)
}

#[inline]
fn plain_array_index_get_guard_impl(
    site_id: u64,
    receiver: f64,
    index_is_plain: bool,
    index: i32,
    require_in_bounds: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return (index_is_plain
            && index >= 0
            && plain_array_index_guard(
                raw_addr as *const ArrayHeader,
                index as u32,
                require_in_bounds != 0,
            )) as i32;
    }
    let observed_index = if index >= 0 { index as u32 } else { u32::MAX };
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, Some(observed_index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let contract_valid = index_is_plain
        && index >= 0
        && plain_array_index_guard(
            raw_addr as *const ArrayHeader,
            index as u32,
            require_in_bounds != 0,
        );
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        contract_valid,
    );
    if pass {
        1
    } else {
        0
    }
}

#[no_mangle]
// The index is always statically proven to be a non-negative `i32` at every
// emit site (see `lower_guarded_array_index_get`), so the guard takes the i32
// index directly — no `f64` index and no `is_plain_number` check (it would be
// tautological) — keeping the int→fp conversion out of the numeric loop's hot
// region. The boxed fallback materializes the `f64` index lazily in its cold
// block.
pub extern "C" fn js_typed_feedback_numeric_array_index_get_guard(
    site_id: u64,
    receiver: f64,
    index: i32,
    require_in_bounds: i32,
) -> i32 {
    numeric_array_index_get_guard_impl(site_id, receiver, true, index, require_in_bounds)
}

#[inline]
fn numeric_array_index_get_guard_impl(
    site_id: u64,
    receiver: f64,
    index_is_plain: bool,
    index: i32,
    require_in_bounds: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return (index_is_plain
            && index >= 0
            && numeric_array_index_guard(
                raw_addr as *const ArrayHeader,
                index as u32,
                require_in_bounds != 0,
            )) as i32;
    }
    let observed_index = if index >= 0 { index as u32 } else { u32::MAX };
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, Some(observed_index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let contract_valid = index_is_plain
        && index >= 0
        && numeric_array_index_guard(
            raw_addr as *const ArrayHeader,
            index as u32,
            require_in_bounds != 0,
        );
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        contract_valid,
    );
    if pass {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_f64_array_loop_guard(
    site_id: u64,
    receiver: f64,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return packed_f64_array_loop_guard(raw_addr as *const ArrayHeader) as i32;
    }
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        packed_f64_array_loop_guard(raw_addr as *const ArrayHeader),
    );
    if pass {
        1
    } else {
        0
    }
}

/// #6011: FFI wrapper for the packed-f64 range-loop guard. `min_idx` /
/// `max_idx_exclusive` are the smallest and one-past-largest indices the
/// candidate fast loop can touch (loop start + smallest offset, loop bound +
/// largest offset), both precomputed as i32 by codegen.
#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_f64_range_loop_guard(
    site_id: u64,
    receiver: f64,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return packed_f64_array_loop_range_guard(
            raw_addr as *const ArrayHeader,
            min_idx,
            max_idx_exclusive,
        ) as i32;
    }
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        packed_f64_array_loop_range_guard(
            raw_addr as *const ArrayHeader,
            min_idx,
            max_idx_exclusive,
        ),
    );
    if pass {
        1
    } else {
        0
    }
}

#[used]
static KEEP_JS_TYPED_FEEDBACK_PACKED_F64_RANGE_LOOP_GUARD: extern "C" fn(
    u64,
    f64,
    i32,
    i32,
) -> i32 = js_typed_feedback_packed_f64_range_loop_guard;

#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_i32_array_loop_guard(
    site_id: u64,
    receiver: f64,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return packed_i32_array_loop_guard(raw_addr as *const ArrayHeader) as i32;
    }
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        packed_i32_array_loop_guard(raw_addr as *const ArrayHeader),
    );
    if pass {
        1
    } else {
        0
    }
}

#[used]
static KEEP_JS_TYPED_FEEDBACK_PACKED_I32_ARRAY_LOOP_GUARD: extern "C" fn(u64, f64) -> i32 =
    js_typed_feedback_packed_i32_array_loop_guard;

#[no_mangle]
pub extern "C" fn js_typed_feedback_packed_u32_array_loop_guard(
    site_id: u64,
    receiver: f64,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return packed_u32_array_loop_guard(raw_addr as *const ArrayHeader) as i32;
    }
    let (class_id, heap_type, aux, element_kind) = classify_array(raw_addr, None);
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: element_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        packed_u32_array_loop_guard(raw_addr as *const ArrayHeader),
    );
    if pass {
        1
    } else {
        0
    }
}

#[used]
static KEEP_JS_TYPED_FEEDBACK_PACKED_U32_ARRAY_LOOP_GUARD: extern "C" fn(u64, f64) -> i32 =
    js_typed_feedback_packed_u32_array_loop_guard;

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_index_get_fallback_boxed(
    site_id: u64,
    receiver: f64,
    index: f64,
) -> f64 {
    record_fallback_call(site_id);

    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    if receiver_value.is_string() || receiver_value.is_short_string() {
        return crate::value::js_dyn_index_get(receiver, index);
    }

    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if raw_addr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    if crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
        return crate::typedarray::js_typed_array_index_get_dynamic(
            raw_addr as *const crate::typedarray::TypedArrayHeader,
            index,
        );
    }

    if crate::buffer::is_registered_buffer(raw_addr) {
        let Some(index) = finite_nonnegative_i32_index(index) else {
            return f64::from_bits(TAG_UNDEFINED);
        };
        let buf = raw_addr as *const crate::buffer::BufferHeader;
        let len = unsafe { (*buf).length };
        if (index as u32) >= len {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return crate::buffer::js_buffer_get(buf, index) as f64;
    }

    if crate::set::is_registered_set(raw_addr) || crate::map::is_registered_map(raw_addr) {
        let Some(index) = finite_nonnegative_u32_index(index) else {
            return f64::from_bits(TAG_UNDEFINED);
        };
        return crate::array::js_array_get_f64(raw_addr as *const ArrayHeader, index);
    }

    if !crate::object::is_valid_obj_ptr(raw_addr as *const u8) {
        return f64::from_bits(TAG_UNDEFINED);
    }

    unsafe {
        let gc_header =
            (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        match (*gc_header).obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => {
                // Fast path: a plain non-negative integer double indexes
                // element storage directly.
                if is_plain_number_bits(index.to_bits())
                    && index >= 0.0
                    && index.fract() == 0.0
                    && index < u32::MAX as f64
                {
                    return crate::array::js_array_get_f64(
                        raw_addr as *const ArrayHeader,
                        index as u32,
                    );
                }
                // Everything else — a string key ("1" canonical index, or a
                // "foo" / "-1" expando), or a negative / fractional / non-finite
                // number — coerces to a property key and dispatches through the
                // full array getter, which routes canonical indices to element
                // storage and otherwise consults named props, `length`, and the
                // Array prototype. Previously every such key returned
                // `undefined`, so `a["1"]` / `a[k]` (k a numeric string) and the
                // `a[-1]` expando read silently missed.
                let key_ptr = index_value_to_property_key(index);
                crate::object::js_object_get_field_by_name_f64(
                    raw_addr as *const ObjectHeader,
                    key_ptr,
                )
            }
            crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_CLOSURE => {
                let key_ptr = index_value_to_property_key(index);
                crate::object::js_object_get_field_by_name_f64(
                    raw_addr as *const ObjectHeader,
                    key_ptr,
                )
            }
            _ => f64::from_bits(TAG_UNDEFINED),
        }
    }
}

fn index_value_to_property_key(index: f64) -> *const crate::StringHeader {
    let bits = index.to_bits();
    let tag = bits & TAG_MASK;
    if tag == STRING_TAG || tag == SHORT_STRING_TAG {
        return crate::value::js_get_string_pointer_unified(index) as *const crate::StringHeader;
    }

    let key = if index.is_nan() {
        "NaN".to_string()
    } else if index.is_infinite() {
        if index.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        let int_index = index as i32;
        if index == int_index as f64 {
            int_index.to_string()
        } else {
            format!("{}", index)
        }
    };
    crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_set_f64(
    site_id: u64,
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) {
    // #5094: skip the dead classify/observe block when recording is off (see
    // `js_typed_feedback_array_get_f64`). The tail array op is the only
    // observable effect in that case.
    if !typed_feedback_enabled() {
        crate::array::js_array_set_f64(arr, index, value);
        return;
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let (class_id, heap_type, aux, _element_kind) = classify_array(raw_addr, Some(index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        plain_array_index_guard(arr, index, true),
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::array::js_array_set_f64(arr, index, value);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_set_f64_extend(
    site_id: u64,
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    // #5094: skip the dead classify/observe block when recording is off (see
    // `js_typed_feedback_array_get_f64`). Preserve the strict extend semantics
    // documented below by routing straight to the same tail op.
    if !typed_feedback_enabled() {
        return crate::array::js_array_set_f64_extend_strict(arr, index, value);
    }
    let raw_addr = normalize_raw_object_addr(arr as u64);
    let (class_id, heap_type, aux, _element_kind) = classify_array(raw_addr, Some(index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        plain_array_index_guard(arr, index, false),
    );
    if !pass {
        record_fallback_call(site_id);
    }
    // This wrapper is only emitted for the source-level `arr[i] = v` assignment,
    // so it carries strict `Set`-with-`Throw` semantics: a frozen array's element
    // is non-writable and a non-extensible array rejects a new index → TypeError.
    crate::array::js_array_set_f64_extend_strict(arr, index, value)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_plain_array_index_set_guard(
    site_id: u64,
    receiver: f64,
    index: i32,
    value: f64,
    require_in_bounds: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return (index >= 0
            && plain_array_index_set_guard(
                raw_addr as *const ArrayHeader,
                index as u32,
                require_in_bounds != 0,
            )) as i32;
    }
    let observed_index = if index >= 0 { index as u32 } else { u32::MAX };
    let (class_id, heap_type, aux, _element_kind) = classify_array(raw_addr, Some(observed_index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let contract_valid = index >= 0
        && plain_array_index_set_guard(
            raw_addr as *const ArrayHeader,
            index as u32,
            require_in_bounds != 0,
        );
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        contract_valid,
    );
    if pass {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_numeric_array_index_set_guard(
    site_id: u64,
    receiver: f64,
    index: i32,
    value: f64,
    require_in_bounds: i32,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if !typed_feedback_enabled() {
        return (index >= 0
            && is_numeric_value_bits(value.to_bits())
            && numeric_array_index_set_guard(
                raw_addr as *const ArrayHeader,
                index as u32,
                require_in_bounds != 0,
            )) as i32;
    }
    let observed_index = if index >= 0 { index as u32 } else { u32::MAX };
    let (class_id, heap_type, aux, _element_kind) = classify_array(raw_addr, Some(observed_index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let contract_valid = index >= 0
        && is_numeric_value_bits(value.to_bits())
        && numeric_array_index_set_guard(
            raw_addr as *const ArrayHeader,
            index as u32,
            require_in_bounds != 0,
        );
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        contract_valid,
    );
    if pass {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_numeric_array_push_guard(
    site_id: u64,
    receiver: f64,
    value: f64,
) -> i32 {
    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    let push_index = match gc_header_for_user_addr(raw_addr) {
        Some(header) if unsafe { (*header).obj_type == crate::gc::GC_TYPE_ARRAY } => unsafe {
            (*(raw_addr as *const ArrayHeader)).length
        },
        _ => u32::MAX,
    };
    let (class_id, heap_type, aux, _element_kind) = classify_array(raw_addr, Some(push_index));
    let observation = Observation {
        source: ObservationSource::Array,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::ArrayElement,
        observation,
        numeric_array_push_guard(raw_addr as *const ArrayHeader, value),
    );
    if pass {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_index_set_fallback_boxed(
    site_id: u64,
    receiver: f64,
    index: f64,
    value: f64,
) -> f64 {
    record_fallback_call(site_id);

    let raw_addr = normalize_raw_object_addr(receiver.to_bits());
    if raw_addr == 0 {
        return receiver;
    }

    if crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
        crate::typedarray_props::js_typed_array_index_set_dynamic(
            raw_addr as *mut crate::typedarray::TypedArrayHeader,
            index,
            value,
        );
        return receiver;
    }

    if crate::buffer::is_registered_buffer(raw_addr) {
        if let Some(index) = finite_nonnegative_i32_index(index) {
            crate::buffer::js_buffer_set(
                raw_addr as *mut crate::buffer::BufferHeader,
                index,
                value as i32,
            );
        }
        return receiver;
    }

    if !crate::object::is_valid_obj_ptr(raw_addr as *const u8) {
        return receiver;
    }

    unsafe {
        let gc_header =
            (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        match (*gc_header).obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => {
                let new_arr = crate::array::js_array_set_index_or_string(
                    raw_addr as *mut ArrayHeader,
                    index,
                    value,
                );
                crate::value::js_nanbox_pointer(new_arr as i64)
            }
            crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_CLOSURE => {
                let key_ptr = crate::value::js_jsvalue_to_string(index);
                if !key_ptr.is_null() {
                    crate::object::js_object_set_field_by_name(
                        raw_addr as *mut ObjectHeader,
                        key_ptr,
                        value,
                    );
                }
                receiver
            }
            _ => receiver,
        }
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_observe_array_element(
    site_id: u64,
    arr: *const ArrayHeader,
    index: u32,
) {
    observe_array(site_id, arr, index);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_set_string_key(
    site_id: u64,
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    // Class-ref receivers (INT32 tag 0x7FFE) are not arrays; skip the array
    // shape observation (which would probe the GC header of a non-pointer) and
    // route straight to the class-ref-aware string-key setter.
    if (arr as u64) >> 48 == 0x7FFE {
        return crate::array::js_array_set_string_key(arr, key, value);
    }
    observe_array(site_id, arr, u32::MAX);
    record_guard_fail(site_id);
    record_fallback_call(site_id);
    crate::array::js_array_set_string_key(arr, key, value)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_array_set_index_or_string(
    site_id: u64,
    arr: *mut ArrayHeader,
    idx: f64,
    value: f64,
) -> *mut ArrayHeader {
    let index = finite_nonnegative_u32_index(idx).unwrap_or(u32::MAX);
    observe_array(site_id, arr, index);
    if index == u32::MAX {
        record_guard_fail(site_id);
        record_fallback_call(site_id);
    } else {
        record_guard_pass(site_id);
    }
    // Assignment-site wrapper → strict `Set` with `Throw = true` (frozen /
    // non-extensible array element write throws a TypeError).
    crate::array::js_array_set_index_or_string_strict(arr, idx, value)
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_object_set_index_polymorphic(
    site_id: u64,
    obj_handle: i64,
    idx: f64,
    value: f64,
) {
    let index = finite_nonnegative_u32_index(idx).unwrap_or(u32::MAX);
    observe_array(site_id, obj_handle as *const ArrayHeader, index);
    record_guard_fail(site_id);
    record_fallback_call(site_id);
    crate::object::js_object_set_index_polymorphic(obj_handle, idx, value);
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_object_set_unboxed_f64_field(
    site_id: u64,
    obj: *mut ObjectHeader,
    field_index: u32,
    key: *const crate::StringHeader,
    value: f64,
) {
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let observation = Observation {
        source: ObservationSource::NumericWrite,
        object_addr: shape_keyed_object_addr(ObservationSource::NumericWrite, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: field_index as u64,
        value_tag: stable_value_kind(value.to_bits()),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::NumericFieldWrite,
        observation,
        object_key_matches_field(obj, key, field_index) && is_plain_number_bits(value.to_bits()),
    );
    if pass {
        crate::object::js_object_set_unboxed_f64_field(obj, field_index, value);
    } else {
        record_fallback_call(site_id);
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_observe_helper_return(site_id: u64, value: f64) -> f64 {
    let bits = value.to_bits();
    let (shape_addr, class_id, heap_type, aux, value_kind) = helper_return_facts(bits);
    let observation = Observation {
        source: ObservationSource::HelperReturn,
        object_addr: 0,
        shape_addr,
        key_hash: 0,
        class_id,
        heap_type,
        aux,
        value_tag: value_kind,
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::HelperReturn,
        observation,
        true,
    );
    if !pass {
        record_fallback_call(site_id);
    }
    value
}

// #854: in-progress typed-feedback shape-change tracking
#[allow(dead_code)]
pub(crate) fn invalidate_shape_change(
    obj: *mut ObjectHeader,
    old_shape: *mut ArrayHeader,
    new_shape: *mut ArrayHeader,
) {
    if old_shape == new_shape {
        return;
    }
    let obj_addr = obj as usize;
    let (_, class_id, _) = object_shape(obj_addr);
    let old_addr = old_shape as usize;
    let new_addr = new_shape as usize;
    let mut reg = registry();
    reg.shape_invalidations = reg.shape_invalidations.saturating_add(1);
    for site in reg.sites.values_mut() {
        let affected = site
            .observations
            .iter()
            .any(|obs| obs.affected_by_shape_change(old_addr, new_addr, class_id));
        if affected {
            site.shape_invalidations = site.shape_invalidations.saturating_add(1);
        }
    }
}

pub(crate) fn invalidate_method_change(class_id: u32) {
    let mut reg = registry();
    reg.method_invalidations = reg.method_invalidations.saturating_add(1);
    for site in reg.sites.values_mut() {
        if site.metadata.kind == TypedFeedbackSiteKind::MethodCall
            && site
                .observations
                .iter()
                .any(|obs| class_id == 0 || obs.class_id == class_id)
        {
            site.method_invalidations = site.method_invalidations.saturating_add(1);
        }
    }
}

/// Upper bound on the cumulative `O(sites)` scan work spent across all
/// [`invalidate_representation_change`] calls. A large object/array built
/// incrementally (a startup spread that sets thousands of properties) triggers
/// a representation-invalidation scan on *every* layout transition; with a big
/// feedback registry that becomes `O(N²)` and effectively hangs the process.
/// The scan only updates per-site deopt counters, and every speculative site
/// re-guards its representation at runtime (`js_typed_feedback_record_guard_*`),
/// so once the estimated total scan work exceeds this budget the scan can be
/// skipped without affecting correctness — churny sites simply deopt less
/// eagerly while their runtime guards still catch any representation mismatch.
const REPRESENTATION_INVALIDATION_SCAN_BUDGET: u64 = 50_000_000;

pub(crate) fn invalidate_representation_change(obj_addr: usize) {
    if obj_addr == 0 {
        return;
    }
    let (shape_addr, class_id, heap_type) = object_shape(obj_addr);
    let mut reg = registry();
    reg.representation_invalidations = reg.representation_invalidations.saturating_add(1);
    // `representation_invalidations * sites` upper-bounds the cumulative scan
    // work; past the budget, skip the scan (see the const docs above).
    if reg
        .representation_invalidations
        .saturating_mul(reg.sites.len() as u64)
        > REPRESENTATION_INVALIDATION_SCAN_BUDGET
    {
        return;
    }
    for site in reg.sites.values_mut() {
        if site.observations.iter().any(|obs| {
            obs.affected_by_representation_change(obj_addr, shape_addr, class_id, heap_type)
        }) {
            site.representation_invalidations = site.representation_invalidations.saturating_add(1);
        }
    }
}

pub fn scan_typed_feedback_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut reg = registry();
    for site in reg.sites.values_mut() {
        for obs in &mut site.observations {
            if obs.roots_object_addr() {
                visitor.visit_usize_slot(&mut obs.object_addr);
            }
            if obs.roots_shape_addr() {
                visitor.visit_usize_slot(&mut obs.shape_addr);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn reset_typed_feedback_for_tests() {
    TRACE_DUMPED.store(false, Ordering::Release);
    let mut reg = registry();
    *reg = TypedFeedbackRegistry::default();
}

#[cfg(test)]
#[path = "typed_feedback/tests.rs"]
mod tests;
