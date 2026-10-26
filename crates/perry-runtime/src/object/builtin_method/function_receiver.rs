use super::*;
use crate::codegen_abi::LayoutWord;

/// A function object whose shape inherits `name` from `Function.prototype`
/// (no own key of that name, no recorded `[[Prototype]]`) while
/// `Function.prototype` and `%Object.prototype%` are pristine.
#[inline(always)]
pub(super) unsafe fn function_ok(v: f64, name: &[u8]) -> bool {
    heap_cell(v).is_some_and(|h| {
        (*h).obj_type == crate::gc::GC_TYPE_CLOSURE && {
            let id = (*(raw(v) as *const crate::closure::ClosureHeader)).shape_id;
            crate::closure::shape::function_shape_inherits_from_function_prototype(id, name)
                || kind_function_ok(id, name)
                || crate::closure::native_constructor::inherits_function(raw(v).cast(), name)
        }
    }) && pristine(BuiltinProto::Function)
}

/// An async, generator or async-generator function without an own `name`:
/// its kind's intrinsic prototype, while unwritten, defines none of
/// `Function.prototype`'s methods and inherits them from it.
#[inline(never)]
fn kind_function_ok(id: LayoutWord, name: &[u8]) -> bool {
    crate::closure::shape::function_shape_kind_lacking(id, name)
        .is_some_and(super::super::global_this::function_kind_prototype_unwritten)
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
    let id = (*closure).shape_id;
    // Preserve the ordinary function lookup's existing direct answer. Only
    // the native proof needs to retain a concrete intrinsic across arguments.
    if crate::closure::shape::function_shape_inherits_from_function_prototype(id, name)
        || kind_function_ok(id, name)
    {
        return pristine(BuiltinProto::Function).then_some(direct);
    }
    if !crate::closure::native_constructor::inherits_function(closure, name)
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
    Some(f64::from_bits(
        super::super::js_object_get_field(proto, slot).bits(),
    ))
}

/// `recv.call(args…)` on a function `function_ok` admitted: a compiled body
/// is entered directly, anything else takes the intrinsic's explicit-`this`
/// call.
#[inline(never)]
pub(super) unsafe fn function_call(recv: f64, args: &[f64]) -> f64 {
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
