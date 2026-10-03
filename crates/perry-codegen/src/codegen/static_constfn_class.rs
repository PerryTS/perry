//! Conservative completed layouts for ordinary, locally defined user classes.
use super::{BirthProto, BirthShape, ModuleBirth};
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{BTreeSet, HashMap};

// Admit expressions with no explicit property mutation or receiver dispatch.
// Operators can still coerce/collect; the runtime finalizer validates the
// resulting receiver after every construction effect has completed.
fn value_proof(e: &Expr) -> bool {
    match e {
        Expr::Closure { .. } => true, // body runs later, captures are current slots
        // A direct, argument-free function effect cannot carry this receiver
        // as an argument. It may allocate/collect; final facts are validated
        // only after it returns. Receiver/property dispatch stays unsupported.
        Expr::Call { callee, args, .. } => {
            matches!(callee.as_ref(), Expr::FuncRef(_)) && args.is_empty()
        }
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::BigInt(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::This => true,
        Expr::Binary { .. }
        | Expr::Unary { .. }
        | Expr::Compare { .. }
        | Expr::Logical { .. }
        | Expr::Object(_) => {
            let mut valid = true;
            perry_hir::walker::walk_expr_children(e, &mut |child| valid &= value_proof(child));
            valid
        }
        _ => false,
    }
}

/// Concrete refusal reasons keep extensions of this proof reviewable. A local
/// root-to-leaf chain is required: imported stubs do not retain initializer
/// bodies, and dynamic/native heritage cannot prove a completed layout.
pub(crate) fn class_final(
    prefix: &str,
    class: &Class,
    classes: &HashMap<String, &Class>,
    base_rep: u64,
    class_id: u32,
) -> Result<BirthShape, &'static str> {
    if class_id == 0 || class.name.starts_with("__AnonShape_") {
        return Err("not an ordinary user class");
    }
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = class;
    loop {
        if !seen.insert(&current.name) {
            return Err("cyclic heritage");
        }
        if current.is_imported_stub()
            || current.extends_expr.is_some()
            || current.native_extends.is_some()
            || current.heritage_lexically_shadowed
        {
            return Err("unresolved or external heritage");
        }
        if current.is_nested || current.alloc_width_hint != 0 {
            return Err("captured or widened construction layout");
        }
        if !current.decorators.is_empty()
            || !current.computed_members.is_empty()
            || current.has_private_instance_elements()
            || !current.getters.is_empty()
            || !current.setters.is_empty()
            || current.methods.iter().any(|m| !m.decorators.is_empty())
            || current
                .static_fields
                .iter()
                .any(|f| f.is_private || f.key_expr.is_some() || !f.decorators.is_empty())
            || current
                .static_methods
                .iter()
                .any(|m| m.name.starts_with('#') || !m.decorators.is_empty())
        {
            return Err("computed, private, decorated or accessor members");
        }
        chain.push(current);
        match current.extends_name.as_deref() {
            Some(name) => {
                let parent = classes
                    .get(name)
                    .copied()
                    .ok_or("unresolved named heritage")?;
                if current.extends.is_some_and(|id| id != parent.id) {
                    return Err("ambiguous heritage identity");
                }
                current = parent;
            }
            None if current.extends.is_some() => return Err("unnamed heritage"),
            None => break,
        }
    }
    let mut props = Vec::new();
    for c in chain.iter().rev() {
        for field in &c.fields {
            if field.is_private
                || field.key_expr.is_some()
                || !field.decorators.is_empty()
                || field.name.starts_with("__perry_cap_")
            {
                return Err("nonpublic or captured field layout");
            }
            let init = field.init.clone().unwrap_or(Expr::Undefined);
            if !value_proof(&init) {
                return Err("initializer has unsupported dispatch or property mutation");
            }
            props.push((field.name.clone(), init));
        }
    }
    let names: BTreeSet<_> = props.iter().map(|(name, _)| name.as_str()).collect();
    for c in &chain {
        if let Some(ctor) = &c.constructor {
            if ctor.is_async
                || ctor.is_generator
                || !ctor.decorators.is_empty()
                || ctor.params.iter().any(|p| {
                    !p.decorators.is_empty() || p.default.as_ref().is_some_and(|e| !value_proof(e))
                })
            {
                return Err("unsupported constructor signature");
            }
            let mut supers = 0;
            for stmt in &ctor.body {
                let valid = match stmt {
                    Stmt::Let { init, .. } => init.as_ref().is_none_or(value_proof),
                    Stmt::Expr(Expr::SuperCall(args)) if c.extends_name.is_some() => {
                        supers += 1;
                        supers == 1 && args.iter().all(value_proof)
                    }
                    Stmt::Expr(Expr::PropertySet {
                        object,
                        property,
                        value,
                    }) => {
                        matches!(object.as_ref(), Expr::This)
                            && names.contains(property.as_str())
                            && value_proof(value)
                    }
                    Stmt::Expr(Expr::PutValueSet {
                        target,
                        key,
                        value,
                        receiver,
                        ..
                    }) => {
                        matches!(target.as_ref(), Expr::This)
                            && matches!(receiver.as_ref(), Expr::This)
                            && matches!(key.as_ref(), Expr::String(k) if names.contains(k.as_str()))
                            && value_proof(value)
                    }
                    Stmt::Expr(e) => value_proof(e),
                    _ => false, // early/value returns, branches, descriptors, delete
                };
                if !valid {
                    return Err("constructor control flow or property mutation");
                }
            }
            if c.extends_name.is_some() && supers != 1 {
                return Err("derived constructor requires one explicit super call");
            }
        }
    }
    let mut shape = super::static_constfn::literal_final(prefix, &props, base_rep)
        .ok_or("no safe closure initializer or uncertain slot order")?;
    shape.proto = BirthProto::Class(class_id);
    Ok(shape)
}

pub(crate) fn module_class_finals(
    module: &Module,
    prefix: &str,
    ordinary: &[ModuleBirth],
    keys_globals: &HashMap<String, String>,
    class_ids: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    let classes = module.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let ordinary: HashMap<_, _> = ordinary
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
            let shape = class_final(prefix, class, &classes, birth.rep, cid).ok()?;
            // Allocation is the authority for keys and live capacity too. A
            // widened/inferred layout is declined, never guessed by this producer.
            if shape.keys != birth.keys || shape.live != birth.live || shape.proto != birth.proto {
                return None;
            }
            Some(ModuleBirth {
                keys_global: format!("perry_constfn_class_final_{prefix}__{cid}"),
                class_id: cid,
                defined: false,
                shape,
            })
        })
        .collect()
}
