//! Consume the receiver birth's F64 lanes in the existing proven-this body.
//! Only the ordinary typed-receiver analysis may prove a read-only body has
//! no intervening calls, writes or coercions of unknown values.
use perry_hir::{Class, Function};
use std::collections::{HashMap, HashSet};

pub(crate) fn fields(
    receiver: &Class,
    method: &Function,
    classes: &HashMap<String, &Class>,
    keys: &str,
) -> HashSet<String> {
    if !method.params.is_empty() {
        return HashSet::new();
    }
    let Some(info) =
        super::typed_abi::typed_f64_accessor_receiver_method_info(receiver, method, classes)
    else {
        return HashSet::new();
    };
    let Some(birth) = super::static_shape_ids::static_birth_content(keys) else {
        return HashSet::new();
    };
    if info.fields.iter().any(|f| {
        f.index >= 32
            || (birth.rep >> (2 * f.index)) & 3 != 1
            || birth.keys.split(|&b| b == 0).nth(f.index as usize) != Some(f.name.as_bytes())
    }) {
        return HashSet::new();
    }
    info.fields.into_iter().map(|f| f.name).collect()
}

/// An aliased receiver proof cannot survive a callback or a coercion of an
/// unknown value. Keep those bodies on their ordinary field semantics. The
/// numeric receiver analysis above proves its operands are raw primitives;
/// this remaining subset only copies values and reads/writes own data slots.
pub(crate) fn preserves_layout(
    receiver: &Class,
    method: &Function,
    classes: &HashMap<String, &Class>,
    keys: &str,
) -> bool {
    use perry_hir::{Expr, Stmt};
    if !fields(receiver, method, classes, keys).is_empty() {
        return true;
    }
    let Some(birth) = super::static_shape_ids::static_birth_content(keys) else {
        return false;
    };
    let own: HashSet<&[u8]> = birth
        .keys
        .split(|&b| b == 0)
        .take(birth.key_count as usize)
        .collect();
    fn safe(e: &Expr, own: &HashSet<&[u8]>) -> bool {
        match e {
            Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::LocalGet(_)
            | Expr::This => true,
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => own.contains(property.as_bytes()),
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } if matches!(object.as_ref(), Expr::This) => {
                own.contains(property.as_bytes()) && safe(value, own)
            }
            _ => false,
        }
    }
    method.body.iter().all(|s| match s {
        Stmt::Return(Some(e)) | Stmt::Expr(e) => safe(e, &own),
        Stmt::Return(None) => true,
        Stmt::Let { init: Some(e), .. } => safe(e, &own),
        _ => false,
    })
}

/// Reuse ordinary typed receiver lowering for a pure numeric accessor. This
/// replaces its general proven-this body, rather than adding another body.
/// The site proves the exact F64 shape before entering; the body has no calls,
/// coercions, allocation or recursion, so it needs no nested stack check.
pub(super) fn compile_leaf(
    module: &mut crate::module::LlModule,
    name: &str,
    receiver: &Class,
    method: &Function,
    classes: &HashMap<String, &Class>,
    keys: &str,
) -> anyhow::Result<bool> {
    if fields(receiver, method, classes, keys).is_empty() {
        return Ok(false);
    }
    let info = super::typed_abi::typed_f64_accessor_receiver_method_info(receiver, method, classes)
        .expect("same numeric receiver admission");
    let body = module.define_function(
        name,
        crate::types::DOUBLE,
        vec![(crate::types::DOUBLE, "%this_arg".into())],
    );
    body.linkage = "internal".into();
    body.pre_statepoint_inline = true;
    body.create_block("entry");
    let block = body.block_mut(0).unwrap();
    let bits = block.bitcast_double_to_i64("%this_arg");
    block.emit_raw(format!(
        "%this_obj = and i64 {bits}, {}",
        crate::nanbox::POINTER_MASK
    ));
    let result = super::typed_abi::lower_typed_f64_receiver_body(
        block,
        &method.params,
        &method.body,
        &info,
        crate::target_layout::object_slot_base_bytes(
            "",
            crate::target_layout::class_instance_form(&receiver.name),
        ),
    );
    // The standard receiver lowerer uses its canonical raw receiver operand.
    // Supply it in the same block so ordinary LLVM optimization can coalesce
    // the caller's unboxing and field operations.
    let result = result?;
    block.ret(crate::types::DOUBLE, &result);
    Ok(true)
}

/// Numeric GET contracts derived from an existing closed receiver proof.
/// These names feed Number-by-construction only; they never become storage
/// fields or authorize an offset. Lineage-only receivers must not use them.
pub(crate) fn numeric_get_properties(
    fact: &crate::collectors::PtrShapeLocal,
    classes: &HashMap<String, &Class>,
) -> HashSet<String> {
    let Some(receiver) = classes.get(&fact.class_name).copied() else {
        return HashSet::new();
    };
    if receiver.specialized_from.is_some() {
        return HashSet::new();
    }
    let mut result = HashSet::new();
    let mut names = HashSet::new();
    let mut seen = HashSet::new();
    let mut current = Some(receiver);
    while let Some(class) = current {
        if class.specialized_from.is_some() || !seen.insert(&class.name) || seen.len() > 64 {
            return HashSet::new();
        }
        for f in &class.fields {
            names.insert(f.name.clone());
        }
        for m in &class.methods {
            names.insert(m.name.clone());
        }
        for (property, getter) in &class.getters {
            if !names.insert(property.clone())
                || property.is_empty()
                || property.contains('\0')
                || !getter.params.is_empty()
                || class.static_accessor_names.contains(property)
            {
                continue;
            }
            if let Some(info) =
                super::typed_abi::typed_f64_accessor_receiver_method_info(receiver, getter, classes)
            {
                if info
                    .fields
                    .iter()
                    .all(|f| fact.numeric_fields.contains(&f.name))
                {
                    result.insert(property.clone());
                }
            }
        }
        // A setter-only descriptor also shadows an ancestor getter.
        for (property, _) in &class.setters {
            names.insert(property.clone());
        }
        current = class
            .extends_name
            .as_deref()
            .and_then(|p| classes.get(p).copied());
    }
    result
}
