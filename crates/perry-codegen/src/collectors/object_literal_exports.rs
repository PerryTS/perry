//! Producer-side capabilities for stable exported object literals.
//!
//! The consumer cannot inspect another module's initializer or closure bodies,
//! so this collector is the sole authority for the guarded direct-call route.
//! It recognizes the source-ordered object-building IIFE emitted by HIR only
//! when that IIFE starts from a non-zero anonymous shape and finishes with an
//! own concise method in the corresponding inline field.

use std::collections::{HashMap, HashSet};

use perry_hir::{Export, Expr, Module, Stmt};

use crate::codegen::{ExportedObjectLiteralCapability, ImportedObjectLiteralMethod};

fn local_get_is(expr: &Expr, expected: u32) -> bool {
    matches!(expr, Expr::LocalGet(id) if *id == expected)
}

fn eligible_method(
    hir: &Module,
    class: &perry_hir::Class,
    key: &str,
    value: &Expr,
) -> Option<ImportedObjectLiteralMethod> {
    let source_prefix = crate::codegen::helpers::sanitize(&hir.name);
    let (func_id, params, target) = match value {
        Expr::Closure {
            func_id,
            params,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            ..
        } => (
            *func_id,
            params,
            format!("perry_closure_{source_prefix}__{func_id}"),
        ),
        // HIR lifts a concise method that does not read `this` into a normal
        // private function and stores its closure wrapper through IndexSet.
        // The live-slot identity guard must compare/call that wrapper, not a
        // nonexistent `perry_closure_*` symbol.
        Expr::FuncRef(func_id) => {
            let function = hir
                .functions
                .iter()
                .find(|function| function.id == *func_id)?;
            if function.is_async || function.is_generator {
                return None;
            }
            let target = format!(
                "__perry_wrap_perry_fn_{}__{}",
                source_prefix,
                crate::codegen::helpers::sanitize(&function.name)
            );
            (*func_id, &function.params, target)
        }
        _ => return None,
    };
    // The first slice uses the existing exact-arity closure guard. Rest and
    // synthesized `arguments` slots remain generic.
    if params
        .iter()
        .any(|param| param.is_rest || param.arguments_object.is_some())
    {
        return None;
    }
    let field_index = class.fields.iter().position(|field| field.name == key)? as u32;
    Some(ImportedObjectLiteralMethod {
        name: key.to_string(),
        func_id,
        target,
        param_count: params.len(),
        field_index,
    })
}

fn class_shape_id_global(hir: &Module, class_name: &str) -> Option<String> {
    let source_prefix = crate::codegen::helpers::sanitize(&hir.name);
    let mut used = HashSet::new();
    for class in &hir.classes {
        let base = format!(
            "perry_class_keys_{}__{}",
            source_prefix,
            crate::codegen::helpers::sanitize(&class.name)
        );
        let keys_global = if used.insert(base.clone()) {
            base
        } else {
            let mut suffix = 1u32;
            loop {
                let candidate = format!("{base}_{suffix}");
                if used.insert(candidate.clone()) {
                    break candidate;
                }
                suffix += 1;
            }
        };
        if class.name == class_name {
            return Some(crate::typed_shape::shape_id_global_name_from_keys_global(
                &keys_global,
            ));
        }
    }
    None
}

fn capability_from_init(
    hir: &Module,
    global_id: u32,
    init: &Expr,
) -> Option<ExportedObjectLiteralCapability> {
    let Expr::Call { callee, args, .. } = init else {
        return None;
    };
    let Expr::Closure {
        params,
        body,
        is_async: false,
        is_generator: false,
        ..
    } = callee.as_ref()
    else {
        return None;
    };
    let [param] = params.as_slice() else {
        return None;
    };
    if param.name != "__perry_obj_iife" {
        return None;
    }
    let [Expr::New {
        class_name,
        args: seed_args,
        ..
    }] = args.as_slice()
    else {
        return None;
    };
    let class = hir.classes.iter().find(|class| {
        class.name == *class_name
            && class.id != 0
            && class.fields.iter().all(|field| field.key_expr.is_none())
    })?;
    if class.fields.len() != seed_args.len()
        || !seed_args.iter().all(|arg| matches!(arg, Expr::Undefined))
    {
        return None;
    }

    let shape_id_global = class_shape_id_global(hir, &class.name)?;

    // Last source write wins. A non-arrow closure stored through the ordinary
    // data-property path is eligible too: HIR uses that path for a concise
    // method whose body does not read `this` (the public suite's `perform(ctx)`
    // shape), as well as for stable function-valued properties. The exact live
    // closure guard makes both cases safe. Other values erase the capability.
    let mut final_methods: HashMap<String, Option<ImportedObjectLiteralMethod>> = HashMap::new();
    let mut saw_return = false;
    for stmt in body {
        match stmt {
            Stmt::Expr(Expr::IndexSet {
                object,
                index,
                value,
            }) if local_get_is(object, param.id) => {
                let Expr::String(key) = index.as_ref() else {
                    return None;
                };
                final_methods.insert(key.clone(), eligible_method(hir, class, key, value));
            }
            Stmt::Expr(Expr::Call { callee, args, .. }) => {
                let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
                    return None;
                };
                if name != "js_object_set_method_by_name" {
                    return None;
                }
                let [receiver, Expr::String(key), value] = args.as_slice() else {
                    return None;
                };
                if !local_get_is(receiver, param.id) {
                    return None;
                }
                final_methods.insert(key.clone(), eligible_method(hir, class, key, value));
            }
            Stmt::Return(Some(value)) if local_get_is(value, param.id) && !saw_return => {
                saw_return = true;
            }
            _ => return None,
        }
    }
    if !saw_return {
        return None;
    }

    let field_names: Vec<String> = class
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect();
    let field_set: HashSet<&str> = field_names.iter().map(String::as_str).collect();
    if final_methods
        .keys()
        .any(|key| !field_set.contains(key.as_str()))
    {
        return None;
    }
    let mut methods: Vec<ImportedObjectLiteralMethod> =
        final_methods.into_values().flatten().collect();
    methods.sort_by_key(|method| method.field_index);
    if methods.is_empty() {
        return None;
    }

    Some(ExportedObjectLiteralCapability {
        class_name: class.name.clone(),
        class_id: class.id,
        global_id,
        shape_id_global,
        field_names,
        methods,
    })
}

pub(crate) fn exported_object_literal_capabilities(
    hir: &Module,
) -> HashMap<String, ExportedObjectLiteralCapability> {
    let exported_objects: HashSet<&str> = hir.exported_objects.iter().map(String::as_str).collect();
    let mut exported_locals: HashSet<&str> = exported_objects.clone();
    for export in &hir.exports {
        if let Export::Named { local, exported } = export {
            if exported_objects.contains(exported.as_str()) {
                exported_locals.insert(local.as_str());
            }
        }
    }
    let mut by_local = HashMap::new();
    for stmt in crate::codegen::entry_outline::logical_entry_stmts(hir) {
        let Stmt::Let {
            id,
            name,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        else {
            continue;
        };
        if !exported_locals.contains(name.as_str()) {
            continue;
        }
        if let Some(capability) = capability_from_init(hir, *id, init) {
            by_local.insert(name.clone(), capability);
        }
    }

    let mut published = HashMap::new();
    for (local, capability) in &by_local {
        // `export default { ... }` and direct named exports both use their
        // public name in `exported_objects`; retain that fail-safe route even
        // if an older HIR producer omitted a redundant `Export::Named` row.
        published.insert(local.clone(), capability.clone());
    }
    for export in &hir.exports {
        if let Export::Named { local, exported } = export {
            if let Some(capability) = by_local.get(local) {
                published.insert(exported.clone(), capability.clone());
            }
        }
    }
    published
}
