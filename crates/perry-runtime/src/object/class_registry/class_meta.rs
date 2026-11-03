use super::*;
use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap};
use crate::object::class_image::ImageTable;
use std::borrow::Cow;
use std::sync::RwLock;

/// Register a class id so `js_value_typeof` can distinguish class refs
/// (INT32-tagged with class_id payload) from real int32 numeric values.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = REGISTERED_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(new_ptr_hash_set());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// Maps `class_id → user-visible class name`. Populated by codegen via
/// `js_register_class_name`. Read back by V8-bridge code when surfacing a
/// Perry class to JS — NestJS's `ModuleTokenFactory.create()` reads
/// `metatype.name` to build the module token, so the empty default name
/// from `v8::Function::builder(...)` would collide every module under the
/// same token. (#1021.)
pub static CLASS_NAMES: ImageTable<RwLock<Option<PtrHashMap<u32, String>>>> =
    ImageTable::new(|image| &image.names);
/// Maps `class_id → ECMAScript constructor length` (formal parameters before
/// the first default/rest parameter). Class refs are integer immediates rather
/// than heap Function objects, so their own `length` property is reified from
/// this table alongside `CLASS_NAMES`.
pub static CLASS_LENGTHS: ImageTable<RwLock<Option<PtrHashMap<u32, u32>>>> =
    ImageTable::new(|image| &image.lengths);

/// Register the user-visible name of a class so the V8 bridge can label
/// the V8-side wrapper for nice `metatype.name` reads. Idempotent.
///
/// A zero-length name is a legitimate registration, not a no-op: an anonymous
/// class expression's `.name` IS the empty string per spec (issue #5952 —
/// `const M = Mixin(Base)` over `function Mixin(B) { return class extends B {} }`
/// binds a class whose `name` is `""`). Membership in `CLASS_NAMES` means "this
/// id is a real class", which stays true for an anonymous one; only the null
/// pointer and the reserved id 0 are rejected.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_name(class_id: u32, name_ptr: *const u8, name_len: u32) {
    if class_id == 0 || name_ptr.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let name = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    if let Err(error) = super::declarations::register_declared_identity(class_id) {
        eprintln!("{error}");
        std::process::abort();
    }
    {
        let mut guard = CLASS_NAMES.write().unwrap();
        if guard.is_none() {
            *guard = Some(new_ptr_hash_map());
        }
        guard.as_mut().unwrap().insert(class_id, name);
    }

    // A declaration has a prototype surface even when its body is empty.
    // Publish that fact at registration, including anonymous declarations;
    // literal-only ids never register a declaration name.
    super::registration::publish_unbuilt_holder(class_id, true);
    crate::object::class_value::note_intrinsic_registration(class_id, "name");
}

/// Look up the user-visible name of a registered class. Returns `None`
/// when the class id was never registered with `js_register_class_name`.
pub fn class_name_for_id(class_id: u32) -> Option<String> {
    let guard = CLASS_NAMES.read().ok()?;
    guard.as_ref()?.get(&class_id).cloned()
}

/// Was `class_id` registered with a user-visible name? [`class_name_for_id`]
/// without cloning the name, for the identity predicates on read misses.
pub(crate) fn class_has_name(class_id: u32) -> bool {
    CLASS_NAMES.read().ok().is_some_and(|guard| {
        guard
            .as_ref()
            .is_some_and(|names| names.contains_key(&class_id))
    })
}

/// #9413: `class_id → the class's original source text`. Populated by codegen
/// via `js_register_class_source_static` (or the copying
/// `js_register_class_source` for an image that can be unloaded), exactly as
/// `js_register_function_source{,_static}` does for functions (#4101, #9188).
/// A class ref is an INT32 immediate rather than a heap Function object, so
/// `Function.prototype.toString` cannot recover its source from a
/// `ClosureHeader` — this side table is the only record. Kept out of the heap
/// image (unlike `CLASS_NAMES`) because nothing but
/// `Function.prototype.toString` reads it.
///
/// Borrowed and owned text share ONE map, unlike the function registries: they
/// split the two because an enum costs 8 bytes on each of ~60,000 borrowed
/// entries, and a bundle carries orders of magnitude fewer classes than
/// functions, so a second table would buy nothing here.
fn class_source_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<u32, RegisteredClassSource>> {
    static REGISTRY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<u32, RegisteredClassSource>>,
    > = std::sync::OnceLock::new();
    crate::once_init::get_or_init(&REGISTRY, || {
        std::sync::Mutex::new(std::collections::HashMap::new())
    })
}

/// One class's retained source text, by who owns the bytes.
enum RegisteredClassSource {
    /// Borrowed from the program image by `js_register_class_source_static`.
    /// Stored unvalidated and decoded on read, as function source is (#9187),
    /// so module init does not fault in every class body just to check it.
    Image(&'static [u8]),
    /// Copied by `js_register_class_source`; validated at registration.
    Owned(std::sync::Arc<str>),
}

/// Register the original source text of a class. The registry COPIES the
/// bytes. Idempotent — last write wins, matching `js_register_class_name`.
///
/// Codegen calls this only for an image that can be unloaded (`dylib` /
/// `staticlib`); executables use [`js_register_class_source_static`].
///
/// # Safety
///
/// `src_ptr..src_ptr + src_len` must point at a valid UTF-8 byte slice that
/// outlives the call (we copy it). `class_id` is used only as a map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_source(class_id: u32, src_ptr: *const u8, src_len: u32) {
    if class_id == 0 || src_ptr.is_null() || src_len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(src_ptr, src_len as usize);
    let Ok(text) = std::str::from_utf8(slice) else {
        return;
    };
    if let Ok(mut map) = class_source_registry().lock() {
        map.insert(
            class_id,
            RegisteredClassSource::Owned(std::sync::Arc::from(text)),
        );
    }
}

/// Codegen-facing entry point: like [`js_register_class_source`], but the
/// registry BORROWS the bytes rather than copying them. Codegen registers the
/// source of every class a bundle contains, to serve a
/// `Function.prototype.toString()` most programs never call, so the copy was
/// dirty heap duplicating text the image already held read-only.
///
/// # Safety
///
/// `src_ptr..src_ptr + src_len` must point at bytes valid for the REST OF THE
/// PROCESS, not merely for the duration of the call — the registry keeps the
/// pointer. Codegen satisfies this with a `private unnamed_addr constant`
/// global in an image nothing unloads (`codegen/string_pool.rs`); anything
/// else must use [`js_register_class_source`]. `class_id` is used only as a
/// map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_source_static(
    class_id: u32,
    src_ptr: *const u8,
    src_len: u32,
) {
    if class_id == 0 || src_ptr.is_null() || src_len == 0 {
        return;
    }
    // SAFETY: the caller's contract is process lifetime.
    let image: &'static [u8] = std::slice::from_raw_parts(src_ptr, src_len as usize);
    if let Ok(mut map) = class_source_registry().lock() {
        map.insert(class_id, RegisteredClassSource::Image(image));
    }
}

/// The retained source text of a registered class, or `None` when codegen
/// registered none (a builtin, or a class synthesized at runtime) or the
/// borrowed bytes are not UTF-8. Image text comes back BORROWED — no copy.
pub fn class_source_for_id(class_id: u32) -> Option<Cow<'static, str>> {
    let map = class_source_registry().lock().ok()?;
    match map.get(&class_id)? {
        RegisteredClassSource::Image(bytes) => {
            crate::source_read_log::note("class", bytes);
            std::str::from_utf8(bytes).ok().map(Cow::Borrowed)
        }
        RegisteredClassSource::Owned(text) => {
            crate::source_read_log::note("class-owned", text.as_bytes());
            Some(Cow::Owned(text.to_string()))
        }
    }
}

/// `Function.prototype.toString` for a class REF: the retained class source
/// when codegen registered it, otherwise the NativeFunction placeholder Node
/// uses for callables with no recoverable source. Mirrors
/// `builtins::function_source_for_func_ptr` for the INT32 class-ref encoding.
pub fn class_ref_to_string(class_id: u32) -> Cow<'static, str> {
    crate::source_read_count::note(crate::source_read_count::ReadKind::Class);
    if let Some(src) = class_source_for_id(class_id) {
        return src;
    }
    let name = class_name_for_id(class_id).unwrap_or_default();
    Cow::Owned(format!("function {name}() {{ [native code] }}"))
}

/// #9413: Node's `util.inspect` / `console.log` rendering of a class
/// constructor — `[class Name]`, `[class Name extends Parent]`,
/// `[class (anonymous)]` when its `.name` is the empty string.
///
/// A class ref is an INT32-tagged NaN box carrying the class id, the same
/// encoding a tagged small integer uses, and the console formatter's
/// `is_int32()` arm printed that payload — so `console.log(Klass)` and
/// `util.inspect(Klass)` both answered `6`, handing the program the
/// compiler's internal class identity. Callers must gate on
/// `is_class_id_registered` first, exactly as `js_jsvalue_to_string` does,
/// so a plain small integer whose value collides with a live class id still
/// prints as a number.
pub fn class_ref_inspect_label(class_id: u32) -> String {
    let name = class_name_for_id(class_id).unwrap_or_default();
    let label = if name.is_empty() {
        "(anonymous)"
    } else {
        name.as_str()
    };
    match crate::object::get_parent_class_id(class_id)
        .filter(|parent| *parent != 0)
        .and_then(class_name_for_id)
        .filter(|parent| !parent.is_empty())
    {
        Some(parent) => format!("[class {label} extends {parent}]"),
        None => format!("[class {label}]"),
    }
}

#[no_mangle]
pub extern "C" fn js_register_class_length(class_id: u32, length: u32) {
    if class_id == 0 {
        return;
    }
    {
        let mut guard = CLASS_LENGTHS.write().unwrap();
        if guard.is_none() {
            *guard = Some(new_ptr_hash_map());
        }
        guard.as_mut().unwrap().insert(class_id, length);
    }
    crate::object::class_value::note_intrinsic_registration(class_id, "length");
}

pub fn class_length_for_id(class_id: u32) -> Option<u32> {
    CLASS_LENGTHS.read().ok()?.as_ref()?.get(&class_id).copied()
}

/// Whether dynamic-dispatch miss diagnostics are enabled (`PERRY_DISPATCH_DIAG`,
/// any non-empty/non-falsey value). Cached on first read.
///
/// When a dynamic dispatch falls through every resolution tower (vtable,
/// static-method, static-field, prototype, field-scan, namespace, symbol), the
/// runtime returns a *silent placeholder* — the receiver class ref, an empty
/// object, `undefined`, etc. — rather than throwing, because some of those
/// placeholders are load-bearing (effect's `.pipe()` chains yield the class ref
/// during module init, #687). The upside is no spurious crashes; the downside
/// is a typo'd / unsupported member surfaces far downstream as a stray
/// `{}`/`1`/`[]`/function, turning each one into a multi-hour localization.
///
/// This flag doesn't change behavior — it just prints a located, typed report
/// at the moment of the miss, so the bug surfaces at its true call site.
pub(crate) fn dispatch_diag_enabled() -> bool {
    use std::sync::OnceLock;
    static EN: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&EN, || {
        std::env::var("PERRY_DISPATCH_DIAG")
            .map(|v| !v.is_empty() && v != "0" && v != "off" && v != "false")
            .unwrap_or(false)
    })
}

/// Best-effort one-line description of a dispatch receiver for diagnostics:
/// class refs resolve to their registered name, pointers/primitives to a tag.
fn describe_dispatch_receiver(recv: f64) -> String {
    let bits = recv.to_bits();
    let top16 = bits >> 48;
    let _ = top16;
    if let Some(cid) = crate::object::class_value::legacy_class_value_word(bits) {
        return match class_name_for_id(cid) {
            Some(n) => format!("class-ref `{}` (id {})", n, cid),
            None => format!("class-ref (id {})", cid),
        };
    }
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        return "string".to_string();
    }
    if top16 == 0x7FFD {
        return "object/pointer".to_string();
    }
    match bits {
        x if x == crate::value::TAG_UNDEFINED => "undefined".to_string(),
        0x7FFC_0000_0000_0002 => "null".to_string(),
        0x7FFC_0000_0000_0003 => "false".to_string(),
        0x7FFC_0000_0000_0004 => "true".to_string(),
        _ if !recv.is_nan() => format!("number {}", recv),
        _ => "value".to_string(),
    }
}

/// Report a true dynamic-dispatch miss to stderr (only when
/// `PERRY_DISPATCH_DIAG` is set). `tower` names which resolution path fell
/// through; `returning` is the silent placeholder the runtime is about to hand
/// back. No-op (and near-zero cost) when the flag is off.
pub(crate) fn report_dispatch_miss(tower: &str, recv: f64, name: &str, returning: &str) {
    if !dispatch_diag_enabled() {
        return;
    }
    eprintln!(
        "[perry dispatch-miss] {tower}: {}.{:?} did not resolve \u{2192} returning {returning}. \
         A dynamic dispatch fell through every tower; downstream this usually surfaces as a stray \
         {{}}/1/[]/function. Check the call site for {:?}.",
        describe_dispatch_receiver(recv),
        name,
        name
    );
}

/// Legacy name-facing consumers read the immutable header identity too.
/// [[Construct]] dispatches on the identity itself, never on this label.
pub(crate) fn identify_global_builtin_constructor(value: f64) -> Option<&'static str> {
    super::super::global_this_tables::constructor_identity(value).map(|id| id.name())
}

pub(crate) fn text_decoder_bool_option(options: f64, name: &str) -> f64 {
    let jsval = crate::value::JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(obj, key);
    let value_f64 = f64::from_bits(value.bits());
    f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value_f64) != 0).bits())
}

pub(crate) unsafe fn validate_web_compression_stream_format(format: f64) {
    let ptr = crate::builtins::js_string_coerce(format) as *const crate::StringHeader;
    if ptr.is_null() {
        crate::fs::validate::throw_type_error_with_code(
            "The argument 'format' is invalid.",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    if matches!(bytes, b"gzip" | b"deflate" | b"deflate-raw" | b"brotli") {
        return;
    }
    let received = String::from_utf8_lossy(bytes);
    let message = format!("The argument 'format' is invalid. Received '{received}'");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

pub(crate) const CLASS_ID_TEXT_ENCODER_STREAM: u32 = 0x7FFF_FF30;
pub(crate) const CLASS_ID_TEXT_DECODER_STREAM: u32 = 0x7FFF_FF31;
pub(crate) const CLASS_ID_COMPRESSION_STREAM: u32 = 0x7FFF_FF32;
pub(crate) const CLASS_ID_DECOMPRESSION_STREAM: u32 = 0x7FFF_FF33;

pub(crate) unsafe fn text_encoding_stream_new_with_constructor(
    constructor: f64,
    class_id: u32,
) -> f64 {
    let stream = js_object_alloc(class_id, 0);
    if stream.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    for key_bytes in [b"readable".as_slice(), b"writable".as_slice()] {
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let endpoint = js_object_alloc(0, 0);
        let value = if endpoint.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            crate::value::js_nanbox_pointer(endpoint as i64)
        };
        js_object_set_field_by_name(stream, key, value);
    }

    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(stream, ctor_key, constructor);

    crate::value::js_nanbox_pointer(stream as i64)
}

unsafe fn text_encoding_stream_new(constructor_name: &[u8], class_id: u32) -> f64 {
    let ctor = js_get_global_this_builtin_value(constructor_name.as_ptr(), constructor_name.len());
    text_encoding_stream_new_with_constructor(ctor, class_id)
}

#[cfg(test)]
pub(crate) unsafe fn test_text_encoding_stream_new_with_constructor(
    constructor: f64,
    class_id: u32,
) -> f64 {
    text_encoding_stream_new_with_constructor(constructor, class_id)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextEncoderStream", CLASS_ID_TEXT_ENCODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_decoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextDecoderStream", CLASS_ID_TEXT_DECODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_compression_stream_new() -> f64 {
    text_encoding_stream_new(b"CompressionStream", CLASS_ID_COMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_decompression_stream_new() -> f64 {
    text_encoding_stream_new(b"DecompressionStream", CLASS_ID_DECOMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoding_stream_new() -> f64 {
    js_text_encoder_stream_new()
}

#[cfg(test)]
mod class_source_tests {
    use super::*;

    /// Fake class ids, distinct per test so these cases stay independent of
    /// each other and of anything else in the process-global registry.
    const CID_STATIC: u32 = 0x1150_1001;
    const CID_COPIED: u32 = 0x1150_1002;
    const CID_REPLACED: u32 = 0x1150_1003;

    /// The point of #11501: the codegen entry point must hand back the very
    /// bytes it was given, not a heap copy of them. A copy would still
    /// compare equal as text, so compare the ADDRESS.
    #[test]
    fn static_class_source_borrows_the_callers_bytes() {
        static SOURCE: &str = "class Borrowed { m() { return 1; } }";
        unsafe {
            js_register_class_source_static(CID_STATIC, SOURCE.as_ptr(), SOURCE.len() as u32);
        }

        let text = class_source_for_id(CID_STATIC).expect("registered class source");
        assert_eq!(&*text, SOURCE);
        assert!(
            matches!(text, Cow::Borrowed(_)),
            "image text must be returned borrowed, not re-allocated on read"
        );
        assert_eq!(
            text.as_ptr(),
            SOURCE.as_ptr(),
            "js_register_class_source_static must borrow: the registry returned \
             a copy of the caller's bytes"
        );

        let rendered = class_ref_to_string(CID_STATIC);
        assert_eq!(
            rendered.as_ptr(),
            SOURCE.as_ptr(),
            "Function.prototype.toString on a class ref must not copy image text"
        );
    }

    /// The copying entry point stays for callers that can only promise the
    /// bytes outlive the call (a `dylib` plugin that `dlclose` unmaps). Prove
    /// it copies without writing the use-after-free: keep the buffer alive
    /// and overwrite it. A stored borrow would read the new contents.
    #[test]
    fn copying_class_source_owns_its_bytes() {
        let mut buffer = b"class Copied { m() { return 1; } }".to_vec();
        unsafe {
            js_register_class_source(CID_COPIED, buffer.as_ptr(), buffer.len() as u32);
        }
        buffer.copy_from_slice(b"class Clobbr { m() { return 2; } }");

        assert_eq!(
            class_source_for_id(CID_COPIED).as_deref(),
            Some("class Copied { m() { return 1; } }"),
            "js_register_class_source must copy: the registry returned the \
             caller's post-registration buffer contents, i.e. it kept a borrow"
        );
    }

    /// Both spellings write the same map, so the last registration wins
    /// whichever kind it is, and an unregistered class keeps the Node
    /// NativeFunction placeholder.
    #[test]
    fn last_registration_wins_across_ownership_kinds() {
        static IMAGE: &str = "class Replaced { fromImage() {} }";
        let owned = b"class Replaced { fromRuntime() {} }".to_vec();
        let name = "Replaced";
        unsafe {
            js_register_class_name(CID_REPLACED, name.as_ptr(), name.len() as u32);
        }
        assert_eq!(
            class_ref_to_string(CID_REPLACED),
            "function Replaced() { [native code] }"
        );

        unsafe {
            js_register_class_source(CID_REPLACED, owned.as_ptr(), owned.len() as u32);
            js_register_class_source_static(CID_REPLACED, IMAGE.as_ptr(), IMAGE.len() as u32);
        }
        assert_eq!(class_ref_to_string(CID_REPLACED).as_ptr(), IMAGE.as_ptr());

        unsafe {
            js_register_class_source(CID_REPLACED, owned.as_ptr(), owned.len() as u32);
        }
        assert_eq!(
            class_ref_to_string(CID_REPLACED),
            "class Replaced { fromRuntime() {} }"
        );
    }
}
