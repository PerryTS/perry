//! Method-name dispatch registry for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Builds `(class_name, method_name) → LLVM function name`
//! so `lower_call` knows which mangled symbol to call for `obj.method(args)`,
//! and pre-declares imported-class methods/getters/setters/ctors/statics as
//! extern LLVM functions so the linker can resolve cross-module method calls.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::DOUBLE;

// Collector and boxing-analysis walkers live in dedicated modules.

// Name-mangling helpers from the trunk (also reachable via `super::*`).
use super::helpers::{sanitize, sanitize_member, scoped_method_name, scoped_static_method_name};
use super::static_method_registry_key;
use super::ImportedClass;

/// Build the `(class, method) → symbol` registry and emit extern declares
/// for imported classes. `class_table` is the merged local+imported lookup;
/// the remaining maps disambiguate imported renamed/shadowed classes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_method_names(
    llmod: &mut LlModule,
    hir: &HirModule,
    imported_classes: &[ImportedClass],
    class_table: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
    imported_class_prefix: &HashMap<String, String>,
    imported_class_source_name: &HashMap<String, String>,
    module_prefix: &str,
) -> HashMap<(String, String), String> {
    // Method registry: (class_name, method_name) → LLVM function name.
    // Built from `class.methods` so the dispatch in `lower_call` knows
    // which mangled function name to call for `obj.method(args)`. Method
    // names are also scoped by module prefix.
    let mut method_names: HashMap<(String, String), String> = HashMap::new();
    for c in class_table.values() {
        // Use the source module prefix for imported classes so the method
        // symbol name matches where the method was actually compiled.
        let class_prefix = imported_class_prefix
            .get(&c.name)
            .map(|s| s.as_str())
            .unwrap_or(module_prefix);
        // Issue #568: when `c` is the stub for an imported renamed class
        // (`export { Widget as PublicWidget }` consumed via
        // `import { PublicWidget }`), `c.name` is the local alias
        // ("PublicWidget"). The source module emits its symbols mangled
        // with the ORIGINAL name ("Widget"); the consumer-side LLVM
        // symbol must match. `mangle_class_name` is the source-side
        // canonical name; the dispatch-table KEY stays `c.name` so
        // `receiver_class_name` lookups (which see the renamed type)
        // still hit.
        let mangle_class_name = imported_class_source_name
            .get(&c.name)
            .map(|s| s.as_str())
            .unwrap_or(c.name.as_str());
        let class_symbol_id = class_ids.get(&c.name).copied().unwrap_or(c.id);
        for m in &c.methods {
            let llvm_name = scoped_method_name(class_prefix, mangle_class_name, &m.name);
            method_names.insert((c.name.clone(), m.name.clone()), llvm_name.clone());
            // Refs #486: also register self-binding aliases (e.g. `_X` from
            // `var X = class _X`) so static method dispatch on a receiver typed
            // as `_X` (the inner name) finds the same LLVM symbol as the
            // canonical `X`-typed dispatch.
            for alias in &c.aliases {
                method_names
                    .entry((alias.clone(), m.name.clone()))
                    .or_insert_with(|| llvm_name.clone());
            }
        }
        for member in &c.computed_members {
            let llvm_name = if member.is_static {
                scoped_static_method_name(
                    class_prefix,
                    class_symbol_id,
                    mangle_class_name,
                    &member.function.name,
                )
            } else {
                scoped_method_name(class_prefix, mangle_class_name, &member.function.name)
            };
            method_names.insert(
                (
                    c.name.clone(),
                    if member.is_static {
                        static_method_registry_key(&member.function.name)
                    } else {
                        member.function.name.clone()
                    },
                ),
                llvm_name.clone(),
            );
            for alias in &c.aliases {
                method_names
                    .entry((
                        alias.clone(),
                        if member.is_static {
                            static_method_registry_key(&member.function.name)
                        } else {
                            member.function.name.clone()
                        },
                    ))
                    .or_insert_with(|| llvm_name.clone());
            }
        }
        // Constructor: register as a method so compile_method can find it.
        // Emitted for ALL classes (even without explicit constructors)
        // so cross-module `new` can call the constructor.
        {
            let ctor_method_name = format!("{}_constructor", c.name);
            method_names.insert(
                (c.name.clone(), ctor_method_name.clone()),
                format!("{}__{}_constructor", class_prefix, mangle_class_name),
            );
        }
        // Getters: register under the property name with a `__get_`
        // prefix to avoid colliding with a regular method of the same
        // name. The dispatch site for `obj.prop` checks the getter
        // map first, then falls back to the regular method registry.
        for (prop, f) in &c.getters {
            method_names.insert(
                (c.name.clone(), format!("__get_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__get_{}", f.name),
                ),
            );
        }
        for (prop, f) in &c.setters {
            method_names.insert(
                (c.name.clone(), format!("__set_{}", prop)),
                scoped_method_name(
                    class_prefix,
                    mangle_class_name,
                    &format!("__set_{}", f.name),
                ),
            );
        }
        // Static methods. Registered under a static-only key so they do not
        // collide with instance methods of the same class and name, and emitted
        // with the class id so duplicate text class names stay distinct.
        for sm in &c.static_methods {
            method_names.insert(
                (c.name.clone(), static_method_registry_key(&sm.name)),
                scoped_static_method_name(
                    class_prefix,
                    class_symbol_id,
                    mangle_class_name,
                    &sm.name,
                ),
            );
        }
    }

    // Phase F: register imported class methods in the method_names
    // registry and pre-declare them as extern LLVM functions so the
    // linker can resolve cross-module method calls.
    for ic in imported_classes {
        let effective_name = ic.local_alias.as_deref().unwrap_or(&ic.name);
        // Skip if locally defined — local methods take precedence.
        if hir.classes.iter().any(|c| c.name == *effective_name) {
            continue;
        }
        let src = &ic.source_prefix;

        for (method_idx, method_name) in ic.method_names.iter().enumerate() {
            // The source module emitted its methods as
            // `perry_method_<source_prefix>__<class>__<method>`.
            // Use the canonical class name (ic.name) for the symbol
            // since that's how the source module mangled it.
            let llvm_fn = format!(
                "perry_method_{}__{}__{}",
                sanitize(src),
                sanitize_member(&ic.name),
                sanitize_member(method_name),
            );
            method_names
                .entry((effective_name.to_string(), method_name.clone()))
                .or_insert_with(|| llvm_fn.clone());

            // Declare extern: `double method(double this, double arg0, …)`.
            // Pre-#235 this was hardcoded to 6 doubles ("safe upper bound").
            // The bug: call sites that passed fewer args (the common case for
            // methods with default params) made the callee read garbage from
            // uninitialized arg-register slots — typically a real heap pointer
            // from a prior call's leftover state. Dereferencing that garbage
            // for `options.session` etc. silently hung in the dispatch chain.
            // We now read the actual arity from the parallel
            // `method_param_counts` Vec populated by the source side. If the
            // source module didn't populate it (legacy or out-of-sync build),
            // fall back to 6 to preserve compat.
            // Total arity = explicit params + 1 implicit `this`.
            let arity = ic
                .method_param_counts
                .get(method_idx)
                .copied()
                .map(|n| n + 1)
                .unwrap_or(6);
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat_n(DOUBLE, arity).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }

        // Cross-module getters. The dispatch site at
        // `expr.rs::PropertyGet` looks up `(class, "__get_<prop>")` in
        // `method_names`; without this loop the entry is missing for
        // imported classes and `obj.prop` silently falls through to
        // `undefined`. The source module mangles getters as
        // `perry_method_<src>__<class>____get_get_<prop>` (the inner
        // `get_<prop>` is the HIR function name from
        // `lower_getter_method`, then codegen prepends `__get_`).
        for prop in &ic.getter_names {
            let inner_fn_name = format!("get_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__get_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__get_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Getters take only `this` (NaN-boxed double) and return double.
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE]);
        }

        // Cross-module setters. Symmetric to getters: source-side
        // mangling is `perry_method_<src>__<class>____set_set_<prop>`.
        for prop in &ic.setter_names {
            let inner_fn_name = format!("set_{}", prop);
            let llvm_fn = scoped_method_name(
                &sanitize(src),
                &ic.name,
                &format!("__set_{}", inner_fn_name),
            );
            method_names
                .entry((effective_name.to_string(), format!("__set_{}", prop)))
                .or_insert_with(|| llvm_fn.clone());
            // Setters take `this` plus the new value, both NaN-boxed
            // doubles, and return double (the assigned value).
            llmod.declare_function(&llvm_fn, DOUBLE, &[DOUBLE, DOUBLE]);
        }

        // Constructor: declared as
        // `<source_prefix>__<class>_constructor(double this, double arg0, …) → double`.
        // The source module's standalone ctor symbol returns DOUBLE — the
        // ECMAScript constructor return-override value (an explicit
        // `return <obj/fn>`) or `undefined` for an ordinary ctor. Declaring it
        // VOID discarded a returned object/function, so `new Chalk(opts)` (whose
        // ctor `return chalkFactory(opts)`) yielded the empty instance instead of
        // the factory. The dispatch in `lower_new` applies `js_ctor_return_override`
        // to this value.
        let ctor_fn = format!("{}__{}_constructor", sanitize(src), sanitize(&ic.name),);
        let mut ctor_params: Vec<crate::types::LlvmType> = vec![DOUBLE];
        for _ in 0..ic.constructor_param_count {
            ctor_params.push(DOUBLE);
        }
        llmod.declare_function(&ctor_fn, DOUBLE, &ctor_params);

        // Cross-module static methods. Source modules emit these as static
        // functions with no `this` receiver, normally qualified by the source
        // class id. Register them under the static-only key the lowering uses.
        for sm in &ic.static_method_names {
            let llvm_fn = if let Some(source_class_id) = ic.source_class_id {
                scoped_static_method_name(&sanitize(src), source_class_id, &ic.name, sm)
            } else {
                format!(
                    "perry_static_{}__{}__{}",
                    sanitize(src),
                    sanitize_member(&ic.name),
                    sanitize_member(sm),
                )
            };
            method_names
                .entry((effective_name.to_string(), static_method_registry_key(sm)))
                .or_insert_with(|| llvm_fn.clone());
            // Declare conservatively with 6 double params; LLVM's direct-call
            // resolution doesn't require an exact arity match for declarations.
            let param_types: Vec<crate::types::LlvmType> = std::iter::repeat_n(DOUBLE, 6).collect();
            llmod.declare_function(&llvm_fn, DOUBLE, &param_types);
        }
    }

    method_names
}
