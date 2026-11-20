//! `[[Prototype]]` of native host objects that are plain `GC_TYPE_OBJECT` cells
//! stamped with a reserved class id (fs streams, `Dir`, `Dirent`).
//!
//! These objects have no class declaration and no registered prototype object,
//! so `Object.getPrototypeOf` used to fall through to the self-prototype
//! fallback and answer the receiver itself. A user loop over the chain
//! (`while (p = Object.getPrototypeOf(p))`) then never ended, and every
//! chain-walking `instanceof` (`rs instanceof Readable`) burned its whole hop
//! budget before answering `false`.
//!
//! The prototype is the one user code sees as `<module>.<export>.prototype`.
//! `native_module::constructor_shapes` already chains that prototype to its
//! parent (`fs.ReadStream.prototype` > `Readable.prototype` > `Stream.prototype`
//! > `EventEmitter.prototype` > `Object.prototype`), so no per-object link has
//! to be created or kept alive.

use crate::fs::{
    CLASS_ID_FS_DIR, CLASS_ID_FS_DIRENT, CLASS_ID_FS_READ_STREAM, CLASS_ID_FS_UTF8_STREAM,
    CLASS_ID_FS_WRITE_STREAM,
};

fn host_export(class_id: u32) -> Option<(&'static str, &'static str)> {
    match class_id {
        CLASS_ID_FS_READ_STREAM => Some(("fs", "ReadStream")),
        CLASS_ID_FS_WRITE_STREAM => Some(("fs", "WriteStream")),
        CLASS_ID_FS_UTF8_STREAM => Some(("fs", "Utf8Stream")),
        CLASS_ID_FS_DIR => Some(("fs", "Dir")),
        CLASS_ID_FS_DIRENT => Some(("fs", "Dirent")),
        _ => None,
    }
}

/// `<module>.<export>.prototype` read through an ordinary `[[Get]]`, so the
/// prototype object is created on first access and is identical to what user
/// code reads. `None` when `class_id` is not a known host class or the export
/// does not resolve.
pub(super) fn native_host_prototype(class_id: u32) -> Option<f64> {
    let (module, export) = host_export(class_id)?;
    let ctor = crate::object::bound_native_callable_export_value(module, export);
    if !crate::value::JSValue::from_bits(ctor.to_bits()).is_pointer() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(ctor);
    let key = b"prototype";
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    let proto = crate::proxy::js_reflect_get(
        ctor.get_nanbox_f64(),
        key.get_nanbox_f64(),
        ctor.get_nanbox_f64(),
    );
    crate::value::JSValue::from_bits(proto.to_bits())
        .is_pointer()
        .then_some(proto)
}
