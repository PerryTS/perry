//! RSTREAM R4-R6: the members of `ReadableStream.prototype`,
//! `ReadableStreamDefaultController.prototype` and
//! `ReadableStreamDefaultReader.prototype`.
//!
//! All three families are `lazy` (`global_this_tables.rs`): realm setup binds
//! the constructors only, and each prototype is born on its first observation
//! by `birth_lazy_family`, so startup does not grow. The member functions are
//! the stdlib's (they probe `this` through the stream registry), installed
//! through [`INSTALL`] by the program's streams init: a program that never
//! links streams installs nothing, and its prototypes hold `constructor` and
//! the tag only.
//!
//! The stdlib reaches this module through the stable C ABI only (the
//! `js_stream_prototype_*` exports below), like `js_install_readable_stream_from`:
//! the stdlib archive carries its own copy of this crate's Rust items, so a
//! direct Rust call from it would fill that copy's hook and allocate in that
//! copy, never the program's runtime.

use crate::closure::JsFunctionInfo;
use crate::object::ObjectHeader;

/// Which family prototype the stdlib's installer is filling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum StreamFamily {
    ReadableStream = 1,
    DefaultController = 2,
    DefaultReader = 3,
}

impl StreamFamily {
    fn of_u8(family: u8) -> Option<Self> {
        match family {
            1 => Some(Self::ReadableStream),
            2 => Some(Self::DefaultController),
            3 => Some(Self::DefaultReader),
            _ => None,
        }
    }

    fn of_name(name: &str) -> Option<Self> {
        match name {
            "ReadableStream" => Some(Self::ReadableStream),
            "ReadableStreamDefaultController" => Some(Self::DefaultController),
            "ReadableStreamDefaultReader" => Some(Self::DefaultReader),
            _ => None,
        }
    }
}

/// The families whose members only the stdlib installs.
pub fn members_from_stdlib(name: &str) -> bool {
    StreamFamily::of_name(name).is_some()
}

/// The program's streams install: fills one family prototype (the family is
/// a [`StreamFamily`] discriminant).
pub type InstallFn = extern "C" fn(u8, *mut ObjectHeader);
/// `new ReadableStreamDefaultReader(stream)`: acquires a reader (stdlib).
pub type ReaderConstructFn = extern "C" fn(f64) -> f64;

static INSTALL: crate::feature_hooks::Hook<InstallFn> = crate::feature_hooks::Hook::empty();
static READER_CONSTRUCT: crate::feature_hooks::Hook<ReaderConstructFn> =
    crate::feature_hooks::Hook::empty();

/// The provider's `ReadableStream.from` (a `JsFunctionInfo` address, 0 until
/// the stdlib's streams init registers it).
static FROM_INFO: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Registers the provider's `ReadableStream.from`. Init writes nothing to the
/// lazy constructor: the family's birth installs `from` beside the prototype
/// members (`populate_prototype`), so no init order matters.
/// # Safety
/// `info` must have static lifetime and describe a one-argument JS method.
#[no_mangle]
pub unsafe extern "C" fn js_install_readable_stream_from(info: *const JsFunctionInfo) {
    FROM_INFO.store(info as usize, std::sync::atomic::Ordering::Release);
}

/// `ReadableStream.from` on the family's constructor, after its `prototype`
/// (Node's own key order: `length`, `name`, `prototype`, `from`).
unsafe fn install_from(info: *const JsFunctionInfo) {
    use crate::closure::ClosureHeader;
    let ctor = crate::array::intrinsic_constructor_addr_if_bound(
        crate::object::BuiltinConstructorIdentity::ReadableStream,
    );
    if ctor == 0 {
        return;
    }
    let method = crate::closure::js_closure_alloc(&*info, 0);
    crate::object::set_bound_native_closure_name(method, "from");
    crate::object::set_builtin_closure_length(method as usize, 1);
    crate::object::set_builtin_closure_non_constructable(method as usize);
    crate::closure::closure_set_dynamic_prop(
        ctor,
        "from",
        crate::value::js_nanbox_pointer(method as *const ClosureHeader as i64),
    );
    crate::object::set_builtin_property_attrs(
        ctor,
        "from".to_string(),
        crate::object::PropertyAttrs::new(true, true, true),
    );
}

/// Called by the stdlib's streams init (beside `ReadableStream.from`).
#[no_mangle]
pub extern "C" fn js_stream_prototypes_install(
    install: InstallFn,
    reader_construct: ReaderConstructFn,
) {
    INSTALL.set(install);
    READER_CONSTRUCT.set(reader_construct);
}

/// Realm population's step for the three families (`proto_methods.rs`):
/// the stdlib's members when streams are linked, then the fixed tag.
/// `false` for any other family.
pub(crate) fn populate_prototype(builtin_name: &str, proto: *mut ObjectHeader) -> bool {
    let Some(family) = StreamFamily::of_name(builtin_name) else {
        return false;
    };
    let _gc = crate::gc::GcSuppressScope::new();
    if let Some(install) = INSTALL.peek() {
        install(family as u8, proto);
    }
    let from = FROM_INFO.load(std::sync::atomic::Ordering::Acquire);
    if family == StreamFamily::ReadableStream && from != 0 {
        // SAFETY: registered by `js_install_readable_stream_from` (static).
        unsafe { install_from(from as *const JsFunctionInfo) };
    }
    true
}

/// `new ReadableStreamDefaultReader(x)`: a reader of `x`, or Node's
/// `ERR_INVALID_ARG_TYPE` when streams are not linked (nothing can be a
/// ReadableStream then).
pub(crate) fn construct_reader(args: &[f64]) -> f64 {
    let stream = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
    match READER_CONSTRUCT.peek() {
        Some(construct) => construct(stream),
        None => crate::fs::validate::throw_type_error_with_code(
            "The \"stream\" argument must be an instance of ReadableStream.",
            "ERR_INVALID_ARG_TYPE",
        ),
    }
}

/// `name_ptr`/`name_len` as the UTF-8 member name.
///
/// # Safety
/// `name_ptr` points at `name_len` bytes of UTF-8.
unsafe fn member_name<'a>(name_ptr: *const u8, name_len: usize) -> &'a str {
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len))
}

/// A prototype method: `{ writable, enumerable, configurable }` (WebIDL
/// operations are enumerable). Returns the installed function.
///
/// # Safety
/// `proto` is the family prototype being born; `name_ptr`/`name_len` UTF-8;
/// `info` static.
#[no_mangle]
pub unsafe extern "C" fn js_stream_prototype_method(
    proto: *mut ObjectHeader,
    name_ptr: *const u8,
    name_len: usize,
    info: *const JsFunctionInfo,
    arity: u32,
) -> f64 {
    let name = member_name(name_ptr, name_len);
    let value = crate::object::install_proto_method(proto, name, info, arity);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        crate::object::PropertyAttrs::new(true, true, true),
    );
    value
}

/// A prototype getter: `{ get, enumerable, configurable }`.
///
/// # Safety
/// As [`js_stream_prototype_method`].
#[no_mangle]
pub unsafe extern "C" fn js_stream_prototype_getter(
    proto: *mut ObjectHeader,
    name_ptr: *const u8,
    name_len: usize,
    info: *const JsFunctionInfo,
) {
    let name = member_name(name_ptr, name_len);
    crate::event_target::prototype::accessor(proto, name, info, None, true);
}

/// `[Symbol.asyncIterator]`: the same function object as `value`
/// (`{ writable, configurable }`).
///
/// # Safety
/// `proto` is the family prototype being born.
#[no_mangle]
pub unsafe extern "C" fn js_stream_prototype_async_iterator(proto: *mut ObjectHeader, value: f64) {
    crate::object::install_builtin_well_known_symbol_method(proto, "asyncIterator", value);
}

/// `name` read off `family`'s prototype (births it); `undefined` when it
/// holds none or `family` is not a [`StreamFamily`].
///
/// # Safety
/// `name_ptr`/`name_len` UTF-8.
#[no_mangle]
pub unsafe extern "C" fn js_stream_family_member(
    family: u8,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    match StreamFamily::of_u8(family) {
        Some(family) => family_member(family, member_name(name_ptr, name_len)).unwrap_or(undefined),
        None => undefined,
    }
}

fn family_member(family: StreamFamily, name: &str) -> Option<f64> {
    let interface = match family {
        StreamFamily::ReadableStream => "ReadableStream",
        StreamFamily::DefaultController => "ReadableStreamDefaultController",
        StreamFamily::DefaultReader => "ReadableStreamDefaultReader",
    };
    let proto = crate::object::builtin_prototype_value(interface);
    if !crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
        return None;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name(
        crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader,
        key,
    );
    let value = f64::from_bits(value.bits());
    (value.to_bits() != crate::value::TAG_UNDEFINED).then_some(value)
}
