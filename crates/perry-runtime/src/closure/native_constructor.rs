//! Sealed native constructor bags, and the fixed chains that prove
//! Function.prototype's call/apply/bind are inherited. No closure ShapeId is
//! repurposed: its recorded [[Prototype]] still requires FunctionDictionary.
//! The bag's static layout word proves its own keys and its parent's link;
//! every fixed ancestor is tested against its own sealed layout word.
use super::shape;
use crate::object::{layout_desc::PerryLayoutDesc, runtime_layouts as layouts};

#[inline(always)]
fn definition(module: &str, name: &str) -> Option<(*const PerryLayoutDesc, u64)> {
    Some(match (module, name) {
        ("events", "EventEmitter") => (
            layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
        ),
        ("stream", "Stream") => (
            layouts::NATIVE_CTOR_STREAM::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
        ),
        ("stream", "Readable") => (
            layouts::NATIVE_CTOR_READABLE::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE,
        ),
        ("stream", "Writable") => (
            layouts::NATIVE_CTOR_WRITABLE::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_WRITABLE,
        ),
        ("stream", "Duplex") => (
            layouts::NATIVE_CTOR_DUPLEX::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_DUPLEX,
        ),
        ("stream", "Transform") => (
            layouts::NATIVE_CTOR_TRANSFORM::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_TRANSFORM,
        ),
        ("stream", "PassThrough") => (
            layouts::NATIVE_CTOR_PASSTHROUGH::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_PASSTHROUGH,
        ),
        ("http", "OutgoingMessage") => (
            layouts::NATIVE_CTOR_OUTGOING_MESSAGE::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_OUTGOING_MESSAGE,
        ),
        ("http", "ServerResponse") => (
            layouts::NATIVE_CTOR_SERVER_RESPONSE::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_SERVER_RESPONSE,
        ),
        ("http", "IncomingMessage") => (
            layouts::NATIVE_CTOR_INCOMING_MESSAGE::desc(),
            shape::INTRINSIC_SERIAL_NATIVE_CTOR_INCOMING_MESSAGE,
        ),
        _ => return None,
    })
}

/// Dynamic constructor readers share the definition resolver.
pub(crate) fn seal(module: &str, name: &str, value: f64) {
    seal_selected(module, name, value)
}

/// A selected constructor specializes only its own static layout declaration.
#[inline(always)]
pub(crate) fn seal_selected(module: &str, name: &str, value: f64) {
    let Some((desc, serial)) = definition(module, name) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(value);
    // EventEmitter's own prototype is normally lazy. A sealed definition
    // includes it, so its first read must not become a write to the bag.
    if module == "events" {
        let _ = crate::object::js_function_prototype_value_for_read(ctor.get_nanbox_f64());
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let ptr = crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
        let old = super::props::bag_of(ptr);
        let built = crate::value::js_nanbox_pointer(old as i64);
        let Some(image) = crate::object::layout_desc::prototype_image(&*desc, built, serial, &[])
        else {
            eprintln!("Perry internal error: native constructor bag {module}.{name} differs from its static layout");
            std::process::abort();
        };
        let bag = crate::value::js_nanbox_get_pointer(image) as *mut crate::object::ObjectHeader;
        // The recorded constructor parent (and other private state) remains
        // an owned edge. It is never a JS property of this null-prototype bag.
        if !crate::meta_read!(crate::meta_obj_runtime_born!(old as *mut _)).is_null() {
            let state = (*crate::meta_read!(crate::meta_obj_runtime_born!(old as *mut _))).expando;
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
            // GC_STORE_AUDIT(BARRIERED): metadata-record child edge.
            (*meta).expando = state;
            crate::gc::runtime_write_barrier_slot(
                meta as usize,
                &(*meta).expando as *const _ as usize,
                state,
            );
        }
        crate::object::builtin_proto::seal_runtime_prototype(bag, serial);
        super::props::install_bag(ptr, bag);
    }
}

/// The existing identity word names the bag itself, not a cached verdict.
/// Its live layout word is the fact, including its irreversible shadow bit.
#[inline(always)]
unsafe fn ancestor(serial: u64, desc: *const PerryLayoutDesc) -> bool {
    crate::object::shapes::identity_word_slot(serial).is_some_and(|slot| {
        let bits = *slot;
        bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
            && (*((bits & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader))
                .parent_class_id
                == (*desc).layout_id
    })
}

/// A fixed set of complete static chains, with one word compare per level.
/// These are constant expressions of the runtime's definitions: no runtime
/// chain walk, key lookup, memo, validity flag or mutation side table.
#[inline(never)]
pub(crate) unsafe fn inherits_function(closure: *const super::ClosureHeader, name: &[u8]) -> bool {
    if !matches!(name, b"call" | b"apply" | b"bind") {
        return false;
    }
    let bag = (*closure).props;
    if bag.is_null() {
        return false;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(bag as *mut _));
    if meta.is_null() {
        return false;
    }
    let word = (*bag).parent_class_id;
    // The existing intrinsic identity selects a static definition; only
    // its layout word proves the contents and link. Ordinary function bags
    // have no such identity and leave before loading constructor layouts.
    match (*meta).proto_serial {
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER => {
            word == (*(layouts::NATIVE_CTOR_EVENT_EMITTER::desc())).layout_id
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM => {
            word == (*(layouts::NATIVE_CTOR_STREAM::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE => {
            word == (*(layouts::NATIVE_CTOR_READABLE::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_WRITABLE => {
            word == (*(layouts::NATIVE_CTOR_WRITABLE::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_DUPLEX => {
            word == (*(layouts::NATIVE_CTOR_DUPLEX::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE,
                    layouts::NATIVE_CTOR_READABLE::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_TRANSFORM => {
            word == (*(layouts::NATIVE_CTOR_TRANSFORM::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_DUPLEX,
                    layouts::NATIVE_CTOR_DUPLEX::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE,
                    layouts::NATIVE_CTOR_READABLE::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_PASSTHROUGH => {
            word == (*(layouts::NATIVE_CTOR_PASSTHROUGH::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_TRANSFORM,
                    layouts::NATIVE_CTOR_TRANSFORM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_DUPLEX,
                    layouts::NATIVE_CTOR_DUPLEX::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE,
                    layouts::NATIVE_CTOR_READABLE::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_OUTGOING_MESSAGE => {
            word == (*(layouts::NATIVE_CTOR_OUTGOING_MESSAGE::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_SERVER_RESPONSE => {
            word == (*(layouts::NATIVE_CTOR_SERVER_RESPONSE::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_OUTGOING_MESSAGE,
                    layouts::NATIVE_CTOR_OUTGOING_MESSAGE::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        shape::INTRINSIC_SERIAL_NATIVE_CTOR_INCOMING_MESSAGE => {
            word == (*(layouts::NATIVE_CTOR_INCOMING_MESSAGE::desc())).layout_id
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_READABLE,
                    layouts::NATIVE_CTOR_READABLE::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_STREAM,
                    layouts::NATIVE_CTOR_STREAM::desc(),
                )
                && ancestor(
                    shape::INTRINSIC_SERIAL_NATIVE_CTOR_EVENT_EMITTER,
                    layouts::NATIVE_CTOR_EVENT_EMITTER::desc(),
                )
        }
        _ => false,
    }
}
