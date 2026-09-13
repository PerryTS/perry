//! Turn the CJS wrapper's synthetic property exports into live value getters.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Module, Stmt};

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR};

pub(super) type PropertyExports = HashMap<String, (u32, String)>;

/// The wrapper emits `export const x = _cjs.x` to register value-export
/// metadata. Do not execute that read during module initialization: it can
/// invoke a getter too early and freezes exports assigned after initialization.
/// Keep the caller's HIR immutable (it is also the object-cache input).
pub(super) fn prepare(hir: &Module) -> (Cow<'_, Module>, PropertyExports) {
    // Pair the wrapper's private binding with its compiler-owned preamble;
    // an ordinary ESM variable named `_cjs` still has snapshot semantics.
    let has_preamble = hir.imports.iter().any(|import| {
        import
            .source
            .strip_prefix("node:")
            .unwrap_or(&import.source)
            == "module"
            && import.specifiers.iter().any(|specifier| {
                matches!(specifier, perry_hir::ImportSpecifier::Named { imported, local }
                    if imported == "createRequire" && local == "__perry_cjs_create_require")
            })
    });
    if !has_preamble {
        return (Cow::Borrowed(hir), HashMap::new());
    }
    let entry = super::entry_outline::logical_entry_stmts(hir);
    let Some(object_id) = entry.iter().find_map(|stmt| match stmt {
        Stmt::Let { id, name, .. } if name == "_cjs" => Some(*id),
        _ => None,
    }) else {
        return (Cow::Borrowed(hir), HashMap::new());
    };
    let exported: HashSet<&str> = hir
        .exports
        .iter()
        .filter_map(|export| match export {
            perry_hir::Export::Named { local, .. } => Some(local.as_str()),
            _ => None,
        })
        .collect();
    let mut properties = HashMap::new();
    let mut bindings = HashSet::new();
    for stmt in entry {
        if let Stmt::Let {
            id,
            name,
            init: Some(Expr::PropertyGet {
                object, property, ..
            }),
            ..
        } = stmt
        {
            if exported.contains(name.as_str())
                && matches!(object.as_ref(), Expr::LocalGet(id) if *id == object_id)
            {
                properties.insert(name.clone(), (object_id, property.clone()));
                bindings.insert(*id);
            }
        }
    }
    if properties.is_empty() {
        return (Cow::Borrowed(hir), properties);
    }
    let mut owned = hir.clone();
    let clear_snapshots = |stmts: &mut [Stmt]| {
        for stmt in stmts {
            if let Stmt::Let { id, init, .. } = stmt {
                if bindings.contains(id) {
                    *init = None;
                }
            }
        }
    };
    clear_snapshots(&mut owned.init);
    for function in &mut owned.functions {
        if super::entry_outline::is_entry_chunk(function) {
            clear_snapshots(&mut function.body);
        }
    }
    (Cow::Owned(owned), properties)
}

pub(super) fn emit_getter(
    llmod: &mut LlModule,
    getter_name: &str,
    module_prefix: &str,
    object_id: u32,
    property: &str,
) {
    let (key_global, key_len) = llmod.add_string_constant(property);
    let getter = llmod.define_function(getter_name, DOUBLE, vec![]);
    getter.create_block("entry");
    let blk = getter.block_mut(0).unwrap();
    // Allocate the key before loading the GC-rooted namespace. No raw object
    // pointer survives that allocation; the boxed property helper handles
    // accessors, prototypes and function-valued module.exports.
    let key = blk.call(
        I64,
        "js_string_from_bytes",
        &[
            (PTR, &format!("@{key_global}")),
            (I32, &key_len.to_string()),
        ],
    );
    let object = blk.load(
        DOUBLE,
        &format!("@perry_global_{module_prefix}__{object_id}"),
    );
    let value = blk.call(
        DOUBLE,
        "js_object_get_field_by_name_boxed",
        &[(DOUBLE, &object), (I64, &key)],
    );
    blk.ret(DOUBLE, &value);
}
