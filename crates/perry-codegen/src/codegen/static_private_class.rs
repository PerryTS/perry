//! #11791: the completed shape of a class with private instance elements.
//!
//! A private field is an `ENTRY_PRIVATE` entry of its holder's key list and a
//! class brand is in the shape's brand list, so construction moves every
//! instance of such a class off its birth ShapeId: each field initializer
//! claims its entry (appended after the birth keys, into an inline slot the
//! layout reserves for it, `private_field_slot_count_in`), and a class with
//! private methods or accessors adds its brand. When the construction is
//! deterministic, every completed instance carries the same facts:
//!
//! * the birth keys, then the storage key of every private field of the
//!   chain, root to leaf in declaration order, as `ENTRY_PRIVATE` entries;
//! * the birth live bound (the layout reserves the private slots);
//! * the birth rep, plus an `F64` lane at each private field whose
//!   initializer is a number literal (the claim's key-add rule);
//! * the sorted brand list of the chain's classes with private methods or
//!   accessors.
//!
//! Those facts are a static content like a birth: the driver assigns it an
//! id, module init mints it by facts (`js_object_final_shape_id_for_class_keys_static_private`),
//! and an instance whose construction reaches the same facts is found under
//! that id by the ordinary transitions. The class guards accept it as a
//! compatible completed id (`compatible_final_shapes`): its public keys,
//! slots and lanes are the birth's. An instance that ends anywhere else (a
//! fresh class evaluation, a non-number initializer value, a return
//! override) is simply not on it, and every guard sends it to its fallback.
//!
//! Refused (no content): a chain that is not wholly local and ordinary, a
//! computed-key field anywhere on it, or a constructor that adds keys of its
//! own (its key order is not the declaration order).

use super::{BirthShape, ModuleBirth};
use perry_hir::{Class, Expr, Module};
use std::collections::HashMap;

/// The storage key the runtime claims for private field `name` of the class
/// whose private elements carry `class_id` (`private_instance_value_name` of
/// the class template's evaluation).
pub(crate) fn private_storage_key(class_id: u32, name: &str) -> String {
    format!("#<perry:private-value:{class_id}:{name}>")
}

/// The chain of `class`, root first, when every link is a local ordinary
/// class.
fn local_chain<'c>(
    class: &'c Class,
    classes: &HashMap<String, &'c Class>,
) -> Option<Vec<&'c Class>> {
    let mut chain = vec![class];
    let mut current = class;
    while let Some(parent) = current.extends_name.as_deref() {
        let parent = classes.get(parent).copied()?;
        if chain.len() > 32 || chain.iter().any(|c| std::ptr::eq(*c, parent)) {
            return None;
        }
        chain.push(parent);
        current = parent;
    }
    if chain.iter().any(|c| {
        c.is_imported_stub()
            || c.native_extends.is_some()
            || c.extends_expr.is_some()
            || c.heritage_lexically_shadowed
    }) {
        return None;
    }
    chain.reverse();
    Some(chain)
}

fn is_number_literal(e: &Expr) -> bool {
    match e {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::Unary {
            op: perry_hir::UnaryOp::Neg,
            operand,
        } => matches!(operand.as_ref(), Expr::Number(_) | Expr::Integer(_)),
        _ => false,
    }
}

/// The completed content of `class` given its birth content `birth`, or
/// `None` when the class has no private instance element or its completed
/// facts are not decidable here. `element_class_id` names the class id a
/// chain class's private elements carry (`private_element_class_id`).
pub(crate) fn private_final(
    class: &Class,
    classes: &HashMap<String, &Class>,
    birth: &BirthShape,
    element_class_id: &dyn Fn(&str) -> u32,
) -> Option<BirthShape> {
    if !birth.constfn.is_empty() || !birth.private.is_empty() || birth.typed.is_some() {
        return None;
    }
    let chain = local_chain(class, classes)?;
    if !chain.iter().any(|c| c.has_private_instance_elements()) {
        return None;
    }
    // A computed key is claimed at construction in source order, and a
    // nested or widened class is allocated outside the birth layout.
    if chain.iter().any(|c| {
        c.fields.iter().any(|f| f.key_expr.is_some()) || c.is_nested || c.alloc_width_hint != 0
    }) {
        return None;
    }
    if crate::lower_call::new_alloc::constructor_added_key_count_in(class, &|name| {
        classes.get(name).copied()
    }) > 0
    {
        return None;
    }
    let mut private = Vec::new();
    let mut brands = Vec::new();
    let mut rep = birth.rep;
    let mut slot = birth.key_count;
    for c in &chain {
        let cid = element_class_id(&c.name);
        if cid == 0 {
            return None;
        }
        if c.has_private_instance_brand() {
            brands.push(u64::from(cid));
        }
        for field in c.fields.iter().filter(|f| f.is_private) {
            private.extend_from_slice(private_storage_key(cid, &field.name).as_bytes());
            private.push(0);
            if slot < 32 && slot < birth.live && field.init.as_ref().is_some_and(is_number_literal)
            {
                rep |= crate::typed_shape::birth_rep_f64_lane(slot);
            }
            slot += 1;
        }
    }
    brands.sort_unstable();
    brands.dedup();
    Some(BirthShape {
        keys: birth.keys.clone(),
        key_count: birth.key_count,
        live: birth.live,
        proto: birth.proto.clone(),
        typed: None,
        rep,
        constfn: Vec::new(),
        private,
        brands,
        attrs: Vec::new(),
    })
}

/// The class id `name`'s private elements carry: its own, or the class it
/// was specialized from (`field_init::private_element_class_id`).
pub(crate) fn element_class_id_in(
    classes: &HashMap<String, &Class>,
    class_ids: &HashMap<String, u32>,
    name: &str,
) -> u32 {
    let mut declaring = name;
    for _ in 0..classes.len() {
        let Some(origin) = classes
            .get(declaring)
            .and_then(|class| class.specialized_from.as_deref())
        else {
            break;
        };
        declaring = origin;
    }
    class_ids.get(declaring).copied().unwrap_or(0)
}

/// The pre-pass contents: one completed content per class this module
/// defines that has private instance elements.
pub(crate) fn module_private_finals(
    module: &Module,
    prefix: &str,
    ordinary: &[ModuleBirth],
    keys_globals: &HashMap<String, String>,
    class_ids: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    let classes: HashMap<String, &Class> =
        module.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let ordinary: HashMap<&str, &BirthShape> = ordinary
        .iter()
        .map(|b| (b.keys_global.as_str(), &b.shape))
        .collect();
    module
        .classes
        .iter()
        .filter_map(|class| {
            let keys = keys_globals.get(&class.name)?;
            let birth = *ordinary.get(keys.as_str())?;
            let cid = *class_ids.get(&class.name)?;
            let shape = private_final(class, &classes, birth, &|name| {
                element_class_id_in(&classes, class_ids, name)
            })?;
            Some(ModuleBirth {
                keys_global: format!("perry_private_class_final_{prefix}__{cid}"),
                class_id: cid,
                defined: false,
                shape,
            })
        })
        .collect()
}

/// The private storage keys and brands of a completed content, as the module
/// constants its init mint passes (`<symbol>_keys` bytes, `<symbol>_brands`
/// words).
pub(crate) fn final_symbol(prefix: &str, id: u32) -> String {
    format!("perry_private_final_{prefix}__{id}")
}

/// Emit the module constants of one completed content.
pub(crate) fn emit_final_constants(
    module: &mut crate::module::LlModule,
    prefix: &str,
    shape: &BirthShape,
    id: u32,
) {
    let symbol = final_symbol(prefix, id);
    if !shape.private.is_empty() {
        let packed = shape
            .private
            .iter()
            .map(|byte| format!("\\{byte:02X}"))
            .collect::<String>();
        module.add_named_string_constant(
            &format!("{symbol}_keys"),
            shape.private.len(),
            &format!("c\"{packed}\""),
        );
    }
    if !shape.brands.is_empty() {
        let words = shape
            .brands
            .iter()
            .map(|b| format!("i64 {b}"))
            .collect::<Vec<_>>()
            .join(", ");
        module.add_raw_global(format!(
            "@{symbol}_brands = private constant [{} x i64] [{words}]",
            shape.brands.len()
        ));
    }
}

/// The static completed content of `class_name` (as compiled in this
/// function's module) and its id: completed instances of exactly this class
/// carry the id, and the class guards accept it.
pub(crate) fn class_static_private_final(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
) -> Option<(u32, BirthShape)> {
    let class = ctx.classes.get(class_name).copied()?;
    let keys_global = ctx.class_keys_globals.get(class_name)?;
    let birth = super::static_shape_ids::static_birth_content(keys_global)?;
    let shape = private_final(class, ctx.classes, &birth, &|name| {
        element_class_id_in(ctx.classes, ctx.class_ids, name)
    })?;
    let id = super::static_shape_ids::static_final_shape_id(&shape)?;
    Some((id, shape))
}

/// Does `class_name` have a static completed content?
pub(crate) fn class_has_static_private_final(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
) -> bool {
    class_static_private_final(ctx, class_name).is_some()
}

/// The completed static id of `class_name`, and the slot of the private field
/// stored under `storage_key` in it with whether that slot is an `F64` lane,
/// when the field is an inline slot of it.
pub(crate) fn static_private_field_slot(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
    storage_key: &str,
) -> Option<(u32, u32, bool)> {
    let (id, shape) = class_static_private_final(ctx, class_name)?;
    let index = shape
        .private
        .split(|&b| b == 0)
        .position(|name| name == storage_key.as_bytes())? as u32;
    let slot = shape.key_count + index;
    if slot >= shape.live {
        return None;
    }
    Some((
        id,
        slot,
        crate::typed_shape::birth_rep_slot_is_f64(shape.rep, slot),
    ))
}
