//! RegExp construction/reinitialization from original rooted strings. Perex
//! alone validates and compiles; publication follows successful emission.
use super::flags::CanonicalFlags;
use super::perex_api::{self as api, PROGRAM_BYTES, SCRATCH_BYTES, WORK};
use super::perex_memory::MemoryBudget;
use super::perex_owner::{GcProgram, HeapSubject};
use super::perex_runtime::{self as host, EngineError};
use super::{RegExpHeader, RegexMetadata, REGEXP_MAGIC, REGEX_EVER_REGISTERED, REGEX_SOURCE_TABLE};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, JSValue};
use perex::binding::BoundSubject;
use perex::Budget;

fn string<'s>(scope: &'s RuntimeHandleScope, value: &RuntimeHandle<'_>) -> RuntimeHandle<'s> {
    let text = if JSValue::from_bits(value.get_nanbox_f64().to_bits()).is_undefined() {
        super::js_string_from_str("")
    } else {
        api::finish(super::perex_dispatch::to_string(value))
    };
    crate::string::js_string_addref(text);
    scope.root_string_ptr(text)
}

fn canonical<'s>(
    scope: &'s RuntimeHandleScope,
    flags: RuntimeHandle<'s>,
) -> Result<(CanonicalFlags, RuntimeHandle<'s>), EngineError> {
    let canonical = unsafe { flags.with_string_bytes(CanonicalFlags::parse) }
        .ok_or(EngineError::InvalidFlags)?;
    let same = unsafe { flags.with_string_bytes(|bytes| bytes == canonical.as_str().as_bytes()) };
    let flags = if same {
        flags
    } else {
        scope.root_string_ptr(super::js_string_from_str(canonical.as_str()))
    };
    crate::string::js_string_addref(flags.get_raw_mut_ptr::<StringHeader>());
    Ok((canonical, flags))
}

fn compile<'s>(
    scope: &'s RuntimeHandleScope,
    source: RuntimeHandle<'s>,
    flags: CanonicalFlags,
) -> Result<GcProgram<'s>, EngineError> {
    let source = BoundSubject::new(
        unsafe { HeapSubject::new(source) }
            .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?,
    )
    .map_err(|e| EngineError::Subject(e.error))?;
    let memory = MemoryBudget::new(SCRATCH_BYTES);
    host::compile(
        scope,
        &source,
        flags,
        &mut Budget::new(WORK),
        &memory,
        PROGRAM_BYTES,
        &mut host::poll,
    )
}

unsafe fn publish(
    receiver: &RuntimeHandle<'_>,
    source: &RuntimeHandle<'_>,
    flags: &RuntimeHandle<'_>,
    canonical: CanonicalFlags,
    program: &GcProgram<'_>,
) {
    let re = receiver.get_raw_mut_ptr::<RegExpHeader>();
    (*re).pattern_ptr = source.get_raw_const_ptr::<StringHeader>();
    (*re).flags_ptr = flags.get_raw_const_ptr::<StringHeader>();
    let flags = canonical.as_str();
    (*re).case_insensitive = flags.contains('i');
    (*re).global = flags.contains('g');
    (*re).multiline = flags.contains('m');
    (*re).sticky = flags.contains('y');
    (*re).dot_all = flags.contains('s');
    (*re).unicode = flags.contains('u') || flags.contains('v');
    (*re).has_indices = flags.contains('d');
    for (slot, value) in [
        (
            std::ptr::addr_of!((*re).pattern_ptr) as usize,
            js_nanbox_string((*re).pattern_ptr as i64),
        ),
        (
            std::ptr::addr_of!((*re).flags_ptr) as usize,
            js_nanbox_string((*re).flags_ptr as i64),
        ),
    ] {
        crate::gc::runtime_write_barrier_gc_slot(re as usize, slot, value.to_bits());
    }
    program.install(receiver);
}

pub(super) fn new(
    source: *const StringHeader,
    flags: *const StringHeader,
) -> Result<*mut RegExpHeader, EngineError> {
    let scope = RuntimeHandleScope::new();
    // Root both arguments before either default/canonical string allocates.
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(flags);
    if !super::is_valid_ptr(source.get_raw_const_ptr::<StringHeader>()) {
        source.set_raw_const_ptr(super::js_string_from_str(""));
    }
    if !super::is_valid_ptr(flags.get_raw_const_ptr::<StringHeader>()) {
        flags.set_raw_const_ptr(super::js_string_from_str(""));
    }
    let (canonical, flags) = canonical(&scope, flags)?;
    let program = compile(&scope, source, canonical)?;
    let re = crate::arena::arena_alloc_gc(
        std::mem::size_of::<RegExpHeader>(),
        std::mem::align_of::<RegExpHeader>(),
        crate::gc::GC_TYPE_REGEXP,
    ) as *mut RegExpHeader;
    if re.is_null() {
        return Err(EngineError::Storage(
            super::perex_memory::StorageError::Allocation,
        ));
    }
    unsafe {
        // No collecting action until the entire header and all edges are valid.
        re.write(RegExpHeader {
            pattern_ptr: std::ptr::null(),
            flags_ptr: std::ptr::null(),
            case_insensitive: false,
            global: false,
            multiline: false,
            sticky: false,
            dot_all: false,
            unicode: false,
            has_indices: false,
            last_index: 0.0f64.to_bits(),
            magic: REGEXP_MAGIC,
            meta: std::ptr::null_mut(),
            perex_program: std::ptr::null(),
        });
        crate::object::exotic_expando::expando_clear_on_alloc(re as usize);
    }
    let receiver = scope.root_raw_mut_ptr(re);
    unsafe {
        publish(&receiver, &source, &flags, canonical, &program);
    }
    REGEX_EVER_REGISTERED.arm();
    REGEX_SOURCE_TABLE.with(|t| {
        t.borrow_mut().insert(
            receiver.get_raw_mut_ptr::<RegExpHeader>() as usize,
            RegexMetadata {
                registered_owner: true,
            },
        );
    });
    Ok(receiver.get_raw_mut_ptr::<RegExpHeader>())
}

fn actual_regex(value: f64) -> Option<*mut RegExpHeader> {
    let v = JSValue::from_bits(value.to_bits());
    (v.is_pointer() && super::is_registered_regex(v.as_pointer::<u8>() as usize))
        .then(|| v.as_pointer::<RegExpHeader>() as *mut RegExpHeader)
}

fn property(owner: &RuntimeHandle<'_>, name: &'static str) -> f64 {
    api::finish(super::perex_dispatch::get(owner, name.as_bytes()))
}

fn is_regexp(value: &RuntimeHandle<'_>) -> bool {
    api::finish(super::perex_dispatch::is_regexp(value))
}

/// Shared constructor operation: IsRegExp is evaluated once; getters and
/// coercions run with both arguments rooted, before native compiler ownership.
pub(super) fn construct(pattern: f64, flags: f64, called: bool) -> *mut RegExpHeader {
    let scope = RuntimeHandleScope::new();
    let pattern = scope.root_nanbox_f64(pattern);
    let flags = scope.root_nanbox_f64(flags);
    let regexp_like = is_regexp(&pattern);
    if called && regexp_like && flags.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
        let ctor = scope.root_nanbox_f64(property(&pattern, "constructor"));
        let intrinsic = crate::object::js_get_global_this_builtin_value(b"RegExp".as_ptr(), 6);
        if ctor.get_nanbox_f64().to_bits() == intrinsic.to_bits() {
            return crate::value::js_nanbox_get_pointer(pattern.get_nanbox_f64())
                as *mut RegExpHeader;
        }
    }
    if let Some(re) = actual_regex(pattern.get_nanbox_f64()) {
        unsafe {
            pattern.set_nanbox_f64(js_nanbox_string((*re).pattern_ptr as i64));
            if flags.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
                flags.set_nanbox_f64(js_nanbox_string((*re).flags_ptr as i64));
            }
        }
    } else if regexp_like {
        let source = scope.root_nanbox_f64(property(&pattern, "source"));
        if flags.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
            flags.set_nanbox_f64(property(&pattern, "flags"));
        }
        pattern.set_nanbox_f64(source.get_nanbox_f64());
    }
    let source = string(&scope, &pattern);
    let flags = string(&scope, &flags);
    api::finish(new(
        source.get_raw_const_ptr::<StringHeader>(),
        flags.get_raw_const_ptr::<StringHeader>(),
    ))
}

pub(super) fn recompile(re: *mut RegExpHeader, pattern: f64, flags: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(re);
    let pattern = scope.root_nanbox_f64(pattern);
    let flags = scope.root_nanbox_f64(flags);
    if let Some(source_re) = actual_regex(pattern.get_nanbox_f64()) {
        if flags.get_nanbox_f64().to_bits() != crate::value::TAG_UNDEFINED {
            crate::collection_iter::throw_type_error(
                "Cannot supply flags when constructing one RegExp from another",
            );
        }
        unsafe {
            flags.set_nanbox_f64(js_nanbox_string((*source_re).flags_ptr as i64));
            pattern.set_nanbox_f64(js_nanbox_string((*source_re).pattern_ptr as i64));
        }
    }
    let source = string(&scope, &pattern);
    let flags = string(&scope, &flags);
    let (canonical, flags) = api::finish(canonical(&scope, flags));
    let program = api::finish(compile(&scope, source, canonical));
    unsafe {
        publish(&receiver, &source, &flags, canonical, &program);
    }
    // RegExpInitialize publishes the new source/flags/program before the
    // throwing lastIndex write. A failed compile above publishes nothing.
    super::set_last_index_throwing(receiver.get_raw_mut_ptr::<RegExpHeader>(), 0);
    js_nanbox_pointer(receiver.get_raw_mut_ptr::<RegExpHeader>() as i64)
}
