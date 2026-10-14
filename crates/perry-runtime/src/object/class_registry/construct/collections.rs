use super::*;

#[inline]
pub(super) fn handles(id: BuiltinConstructorIdentity) -> bool {
    matches!(
        id,
        BuiltinConstructorIdentity::Map
            | BuiltinConstructorIdentity::Set
            | BuiltinConstructorIdentity::WeakMap
            | BuiltinConstructorIdentity::WeakSet
    )
}

/// One construction path for collections: establish GetPrototypeFromConstructor
/// before any observable iterable adder Get. The original constructor supplies
/// the intrinsic default even when its global binding has been replaced.
pub(super) unsafe fn construct(
    id: BuiltinConstructorIdentity,
    constructor: f64,
    args: &[f64],
    new_target: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let nt = scope.root_nanbox_f64(new_target);
    // Arguments are external raw words, so root the iterable before prototype Get.
    let iterable = args.first().map(|&v| scope.root_nanbox_f64(v));
    let same = constructor.get_nanbox_u64() == nt.get_nanbox_u64();
    // Map's retained realm intrinsic and the weak collections' declared
    // layouts already supply the non-writable default prototype. Set's
    // native layout does not retain an intrinsic prototype, so its owner
    // needs the prototype fact even when constructor and new.target agree.
    // A different new.target still performs GetPrototypeFromConstructor.
    let proto = if same && id != BuiltinConstructorIdentity::Set {
        None
    } else {
        let intrinsic = constructor_prototype_bits(constructor.get_nanbox_f64())
            .map(|bits| scope.root_nanbox_u64(bits));
        if same {
            intrinsic
        } else {
            new_target_custom_object_prototype(nt.get_nanbox_f64())
                .map(|bits| scope.root_nanbox_u64(bits))
                .or(intrinsic)
        }
    };
    let result = match id {
        BuiltinConstructorIdentity::Map => {
            crate::value::js_nanbox_pointer(crate::map::js_map_alloc(4) as i64)
        }
        BuiltinConstructorIdentity::Set => {
            crate::value::js_nanbox_pointer(crate::set::js_set_alloc(4) as i64)
        }
        BuiltinConstructorIdentity::WeakMap => {
            crate::value::js_nanbox_pointer(crate::weakref::js_weakmap_new() as i64)
        }
        BuiltinConstructorIdentity::WeakSet => {
            crate::value::js_nanbox_pointer(crate::weakref::js_weakset_new() as i64)
        }
        _ => unreachable!(),
    };
    let result = scope.root_nanbox_f64(result);
    if matches!(
        id,
        BuiltinConstructorIdentity::WeakMap | BuiltinConstructorIdentity::WeakSet
    ) && constructor.get_nanbox_u64() != nt.get_nanbox_u64()
    {
        let finished =
            native_receiver::finish_native_object(&scope, result.get_nanbox_f64(), &nt, proto);
        result.set_nanbox_f64(finished);
    } else {
        native_receiver::apply_prototype(result.get_nanbox_f64(), proto);
    }
    let Some(iterable) = iterable else {
        return result.get_nanbox_f64();
    };
    let value = iterable.get_nanbox_f64();
    let receiver = result.get_nanbox_f64();
    match id {
        BuiltinConstructorIdentity::Map => {
            crate::value::js_nanbox_pointer(crate::map::map_init_iterable(
                crate::value::js_nanbox_get_pointer(receiver) as *mut crate::map::MapHeader,
                value,
            ) as i64)
        }
        BuiltinConstructorIdentity::Set => {
            crate::value::js_nanbox_pointer(crate::set::set_init_iterable(
                crate::value::js_nanbox_get_pointer(receiver) as *mut crate::set::SetHeader,
                value,
            ) as i64)
        }
        BuiltinConstructorIdentity::WeakMap => {
            crate::weakref::js_weakmap_init_iterable(receiver, value)
        }
        BuiltinConstructorIdentity::WeakSet => {
            crate::weakref::js_weakset_init_iterable(receiver, value)
        }
        _ => unreachable!(),
    }
}
