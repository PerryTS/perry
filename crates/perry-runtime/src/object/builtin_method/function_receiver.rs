use super::*;

/// A function object whose shape inherits `name` from `Function.prototype`
/// (no own key of that name, no recorded `[[Prototype]]`) while
/// `Function.prototype` and `%Object.prototype%` are pristine.
#[inline(always)]
pub(in crate::object) unsafe fn function_ok(v: f64, name: &[u8]) -> bool {
    let Some(h) = heap_cell(v) else {
        return false;
    };
    if (*h).obj_type != crate::gc::GC_TYPE_CLOSURE {
        return false;
    }
    let closure = raw(v) as *const crate::closure::ClosureHeader;
    (function_inherits(closure, name)
        || crate::closure::native_constructor::inherits_function(raw(v).cast(), name))
        && pristine(BuiltinProto::Function)
}

/// Does function `closure` inherit `name` from `Function.prototype`, directly
/// or through its kind's unwritten intrinsic prototype (an async, generator
/// or async-generator function without an own `name`: its kind's prototype
/// defines none of `Function.prototype`'s methods)? The shape is asked once;
/// its answer, the kind, decides.
#[inline(always)]
unsafe fn function_inherits(closure: *const crate::closure::ClosureHeader, name: &[u8]) -> bool {
    use crate::closure::shape::FunctionProtoKind;
    if crate::closure::shape::is_plain_base_shape((*closure).shape_id) {
        return true;
    }
    match crate::closure::shape::function_shape_kind_lacking(closure, name) {
        Some(FunctionProtoKind::Function) => true,
        Some(kind) => super::super::global_this::function_kind_prototype_unwritten(kind),
        None => {
            !matches!(name, b"bind" | b"call" | b"apply")
                && crate::closure::shape::function_shape_inherits_from_function_prototype(
                    closure, name,
                )
        }
    }
}

/// A sealed native constructor's inherited intrinsic is captured before
/// argument evaluation. Arguments can relink the constructor or shadow the
/// method afterwards; the call still uses the value read here.
#[inline(always)]
pub(super) unsafe fn lookup_value(recv: f64, name: &[u8], direct: f64) -> Option<f64> {
    let header = heap_cell(recv)?;
    if (*header).obj_type != crate::gc::GC_TYPE_CLOSURE {
        return None;
    }
    let closure = raw(recv) as *const crate::closure::ClosureHeader;
    // Preserve the ordinary function lookup's existing direct answer. Only
    // the native proof needs to retain a concrete intrinsic across arguments.
    if function_inherits(closure, name) {
        if !pristine(BuiltinProto::Function) {
            return None;
        }
        // `f.call` / `f.apply` is read once, before the arguments, and the
        // value read is called: the answer is the realm's own `call` or
        // `apply` (the pristine prototype's slot), whose body the call half
        // enters directly. An argument that shadows the key afterwards does
        // not change the value read.
        if name != b"call" && name != b"apply" {
            return Some(direct);
        }
    } else if !crate::closure::native_constructor::inherits_function(closure, name)
        || !pristine(BuiltinProto::Function)
    {
        return None;
    }
    // The static FUNCTION_PROTOTYPE layout's apply/bind/call positions.
    let slot = match name {
        b"apply" => 5,
        b"bind" => 6,
        b"call" => 7,
        _ => return None,
    };
    let proto = crate::agent_ptrs::intrinsic_proto(BuiltinProto::Function as usize);
    // Pristine: the prototype holds its static layout, whose apply, bind and
    // call are inline data slots holding the realm's own functions (pointer
    // words, never an I32 lane), so the slot's word is the value.
    debug_assert!(slot < crate::object::object_live_slot_count(proto.cast()));
    let fields = (proto as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const u64;
    Some(f64::from_bits(*fields.add(slot as usize)))
}

/// `recv.call(args…)` on a function `function_ok` admitted (its shape
/// inherits `call` from the pristine `Function.prototype`, so the method is
/// the realm's own): the realm's `hasOwnProperty` target is that body with
/// `this = a0`, a compiled body is entered directly, anything else takes the
/// intrinsic's explicit-`this` call. Nothing is kept.
#[inline(never)]
pub(super) unsafe fn function_call(recv: f64, args: &[f64]) -> f64 {
    if let Some(value) = super::super::global_this::has_own_property_call(recv, args) {
        return value;
    }
    if let Some(value) = super::super::native_call_method::compiled_target_call(
        false,
        recv,
        args.as_ptr(),
        args.len(),
    ) {
        return value;
    }
    let this = args.first().copied().unwrap_or(UNDEFINED);
    super::super::native_call_method::call_function_with_explicit_this(
        recv,
        this,
        args.get(1..).unwrap_or(&[]),
    )
}
