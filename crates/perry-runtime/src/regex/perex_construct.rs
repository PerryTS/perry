//! RegExp construction/reinitialization from original rooted strings. Perex
//! alone validates and compiles; publication follows successful emission.
use super::flags::CanonicalFlags;
use super::perex_api::{self as api, PROGRAM_BYTES, SCRATCH_BYTES, WORK};
use super::perex_memory::MemoryBudget;
use super::perex_owner::{GcProgram, HeapSubject};
use super::perex_runtime::{self as host, EngineError};
use super::{RegExpData, RegExpHeader};
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
    flags.with_mut_ptr::<StringHeader, _>(|flags| crate::string::js_string_addref(flags));
    Ok((canonical, flags))
}

fn compile<'s>(
    scope: &'s RuntimeHandleScope,
    source: RuntimeHandle<'s>,
    flags: CanonicalFlags,
) -> Result<GcProgram<'s>, EngineError> {
    super::perex_cache::get_or_compile(scope, &source, flags, || {
        compile_uncached(scope, source, flags)
    })
}

fn compile_uncached<'s>(
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

/// A program for `re`'s own source and canonical flags with `y` removed,
/// compiled from its internal slots, so no property of `re` is observed.
/// Split's forward search uses it in place of the sticky splitter (#10165).
pub(crate) fn nonsticky_program<'s>(
    scope: &'s RuntimeHandleScope,
    re: &RuntimeHandle<'_>,
) -> Result<GcProgram<'s>, EngineError> {
    let (source, flags) = re.with_const_ptr::<RegExpHeader, _>(|re| unsafe {
        let data = &*crate::regex::regexp_data_ptr(re);
        (data.pattern_ptr, data.flags_ptr)
    });
    if source.is_null() || flags.is_null() {
        return Err(EngineError::InvalidFlags);
    }
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(flags);
    let canonical = unsafe {
        flags.with_string_bytes(|bytes| {
            let without: Vec<u8> = bytes.iter().copied().filter(|&b| b != b'y').collect();
            CanonicalFlags::parse(&without)
        })
    }
    .ok_or(EngineError::InvalidFlags)?;
    compile(scope, source, canonical)
}

unsafe fn publish(
    receiver: &RuntimeHandle<'_>,
    source: &RuntimeHandle<'_>,
    flags: &RuntimeHandle<'_>,
    canonical: CanonicalFlags,
    program: &GcProgram<'_>,
) {
    // Field writes and write barriers only: nothing here allocates, so the
    // three addresses stay current until `install` re-reads the receiver.
    receiver.with_mut_ptr::<RegExpData, _>(|re| {
        source.with_const_ptr::<StringHeader, _>(|source| (*re).pattern_ptr = source);
        flags.with_const_ptr::<StringHeader, _>(|flags| (*re).flags_ptr = flags);
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
    });
    program.install(receiver);
}

pub(super) fn new_data<'s>(
    scope: &'s RuntimeHandleScope,
    source: *const StringHeader,
    flags: *const StringHeader,
) -> Result<RuntimeHandle<'s>, EngineError> {
    // Root both arguments before either default/canonical string allocates.
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(flags);
    if !source.with_const_ptr::<StringHeader, _>(|p| super::is_valid_ptr(p)) {
        source.set_raw_const_ptr(super::js_string_from_str(""));
    }
    if !flags.with_const_ptr::<StringHeader, _>(|p| super::is_valid_ptr(p)) {
        flags.set_raw_const_ptr(super::js_string_from_str(""));
    }
    let (canonical, flags) = canonical(&scope, flags)?;
    if let Some(data) = super::perex_cache::data(&scope, &source, canonical) {
        return Ok(data);
    }
    new_data_miss(scope, source, flags, canonical)
}

#[cold]
#[inline(never)]
fn new_data_miss<'s>(
    scope: &'s RuntimeHandleScope,
    source: RuntimeHandle<'s>,
    flags: RuntimeHandle<'s>,
    canonical: CanonicalFlags,
) -> Result<RuntimeHandle<'s>, EngineError> {
    let program = compile(&scope, source, canonical)?;
    let re = crate::arena::arena_alloc_gc(
        std::mem::size_of::<RegExpData>(),
        std::mem::align_of::<RegExpData>(),
        crate::gc::GC_TYPE_REGEXP,
    ) as *mut RegExpData;
    if re.is_null() {
        return Err(EngineError::Storage(
            super::perex_memory::StorageError::Allocation,
        ));
    }
    unsafe {
        // No collecting action until the entire header and all edges are valid.
        // GC_STORE_AUDIT(INIT): fresh header with every edge null; `publish` stores the pattern, flags and program edges through the barrier.
        re.write(RegExpData {
            pattern_ptr: std::ptr::null(),
            flags_ptr: std::ptr::null(),
            case_insensitive: false,
            global: false,
            multiline: false,
            sticky: false,
            dot_all: false,
            unicode: false,
            has_indices: false,
            perex_program: std::ptr::null(),
        });
    }
    let receiver = scope.root_raw_mut_ptr(re);
    unsafe {
        publish(&receiver, &source, &flags, canonical, &program);
    }
    super::perex_cache::install_data(&source, canonical, &receiver);
    Ok(receiver)
}

/// Test-only program publication. Copy into a fresh unpublished data cell so
/// ownership witnesses never mutate data shared by the compile cache.
#[cfg(test)]
pub(crate) unsafe fn test_install_program(receiver: &RuntimeHandle<'_>, program: &GcProgram<'_>) {
    let scope = RuntimeHandleScope::new();
    let old = scope.root_raw_const_ptr(
        receiver.with_const_ptr::<RegExpHeader, _>(|r| super::regexp_data_ptr(r)),
    );
    let data = crate::arena::arena_alloc_gc(
        std::mem::size_of::<RegExpData>(),
        std::mem::align_of::<RegExpData>(),
        crate::gc::GC_TYPE_REGEXP,
    ) as *mut RegExpData;
    // GC_STORE_AUDIT(INIT): copy traced strings into an unpublished cell,
    // then barrier both edges before any collecting action.
    data.write(old.with_const_ptr::<RegExpData, _>(|old| std::ptr::read(old)));
    for (slot, value) in [
        (
            std::ptr::addr_of!((*data).pattern_ptr) as usize,
            js_nanbox_string((*data).pattern_ptr as i64),
        ),
        (
            std::ptr::addr_of!((*data).flags_ptr) as usize,
            js_nanbox_string((*data).flags_ptr as i64),
        ),
    ] {
        crate::gc::runtime_write_barrier_gc_slot(data as usize, slot, value.to_bits());
    }
    let data = scope.root_raw_mut_ptr(data);
    program.install(&data);
    assert!(crate::object::intrinsic_private_set(
        receiver.with_const_ptr::<RegExpHeader, _>(|r| js_nanbox_pointer(r as i64)),
        super::REGEXP_MATCHER,
        data.with_const_ptr::<RegExpData, _>(|d| js_nanbox_pointer(d as i64)),
    ));
}

pub(super) fn new(
    source: *const StringHeader,
    flags: *const StringHeader,
) -> Result<*mut RegExpHeader, EngineError> {
    let scope = RuntimeHandleScope::new();
    let data = new_data(&scope, source, flags)?;
    Ok(super::instance::new(&scope, &data))
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
    if let Some(data) = super::regexp_data_of(pattern.get_nanbox_f64()) {
        unsafe {
            pattern.set_nanbox_f64(js_nanbox_string((*data).pattern_ptr as i64));
            if flags.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
                flags.set_nanbox_f64(js_nanbox_string((*data).flags_ptr as i64));
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
    // `new` roots both strings before it allocates anything.
    api::finish(source.with_const_ptr::<StringHeader, _>(|source| {
        flags.with_const_ptr::<StringHeader, _>(|flags| new(source, flags))
    }))
}

pub(super) fn recompile(re: *mut RegExpHeader, pattern: f64, flags: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(re);
    let pattern = scope.root_nanbox_f64(pattern);
    let flags = scope.root_nanbox_f64(flags);
    if let Some(data) = super::regexp_data_of(pattern.get_nanbox_f64()) {
        if flags.get_nanbox_f64().to_bits() != crate::value::TAG_UNDEFINED {
            crate::collection_iter::throw_type_error(
                "Cannot supply flags when constructing one RegExp from another",
            );
        }
        unsafe {
            flags.set_nanbox_f64(js_nanbox_string((*data).flags_ptr as i64));
            pattern.set_nanbox_f64(js_nanbox_string((*data).pattern_ptr as i64));
        }
    }
    let source = string(&scope, &pattern);
    let flags = string(&scope, &flags);
    let data = api::finish(source.with_const_ptr::<StringHeader, _>(|source| {
        flags.with_const_ptr::<StringHeader, _>(|flags| new_data(&scope, source, flags))
    }));
    let installed = crate::object::intrinsic_private_set(
        receiver.with_mut_ptr::<RegExpHeader, _>(|re| js_nanbox_pointer(re as i64)),
        super::REGEXP_MATCHER,
        data.with_const_ptr::<RegExpData, _>(|data| js_nanbox_pointer(data as i64)),
    );
    assert!(
        installed,
        "recompile requires the intrinsic private matcher"
    );
    // RegExpInitialize publishes the new source/flags/program before the
    // throwing lastIndex write. A failed compile above publishes nothing.
    // Allocates only on its throwing path, which does not return.
    receiver.with_mut_ptr::<RegExpHeader, _>(|re| super::set_last_index_throwing(re, 0));
    receiver.with_mut_ptr::<RegExpHeader, _>(|re| js_nanbox_pointer(re as i64))
}
