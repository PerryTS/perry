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

/// The program's streams install: fills one family prototype.
pub type InstallFn = fn(StreamFamily, *mut ObjectHeader);
/// `new ReadableStreamDefaultReader(stream)`: acquires a reader (stdlib).
pub type ReaderConstructFn = fn(f64) -> f64;

static INSTALL: crate::feature_hooks::Hook<InstallFn> = crate::feature_hooks::Hook::empty();
static READER_CONSTRUCT: crate::feature_hooks::Hook<ReaderConstructFn> =
    crate::feature_hooks::Hook::empty();

/// Called by the stdlib's streams init (beside `ReadableStream.from`).
pub fn set_stream_prototype_install(install: InstallFn, reader_construct: ReaderConstructFn) {
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
        install(family, proto);
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

/// A prototype method: `{ writable, enumerable, configurable }` (WebIDL
/// operations are enumerable). Returns the installed function.
pub fn method(
    proto: *mut ObjectHeader,
    name: &str,
    info: *const JsFunctionInfo,
    arity: u32,
) -> f64 {
    let value = crate::object::install_proto_method(proto, name, info, arity);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        crate::object::PropertyAttrs::new(true, true, true),
    );
    value
}

/// A prototype getter: `{ get, enumerable, configurable }`.
pub fn getter(proto: *mut ObjectHeader, name: &str, info: *const JsFunctionInfo) {
    crate::event_target::prototype::accessor(proto, name, info, None, true);
}

/// `[Symbol.asyncIterator]`: the same function object as `value`
/// (`{ writable, configurable }`).
pub fn async_iterator(proto: *mut ObjectHeader, value: f64) {
    crate::object::install_builtin_well_known_symbol_method(proto, "asyncIterator", value);
}

/// `name` read off `family`'s prototype (births it), when it holds one.
pub fn family_member(family: StreamFamily, name: &str) -> Option<f64> {
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
