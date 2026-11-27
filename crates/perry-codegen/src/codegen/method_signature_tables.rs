//! Per-method signature tables (declared parameter count, rest parameter,
//! synthetic `arguments`, `arguments.length`-only ABI) for local and
//! imported classes, read by every method-call dispatch site.

use perry_hir::Module as HirModule;

use super::{arguments, static_method_registry_key, CompileOptions};

/// The method signature tables `lower_module` builds, each field named for
/// the local it binds.
pub(super) struct MethodSignatureTables {
    pub(super) method_param_counts: std::collections::HashMap<(String, String), usize>,
    pub(super) method_has_rest: std::collections::HashMap<(String, String), bool>,
    pub(super) method_has_synthetic_arguments: std::collections::HashMap<(String, String), bool>,
    pub(super) method_arguments_length_only: std::collections::HashMap<(String, String), bool>,
}

pub(super) fn build_method_signature_tables(
    hir: &HirModule,
    opts: &CompileOptions,
) -> MethodSignatureTables {
    // Issue #235: per-method explicit-param-count map covering BOTH local
    // classes (from `hir.classes`) AND imported classes (from
    // `opts.imported_classes`). Every method-call dispatch site in
    // `lower_call.rs` looks up here to pad missing trailing args with
    // TAG_UNDEFINED so the callee's default-param desugaring (`if (options
    // === undefined) options = {}`) fires correctly. Pre-fix the dispatch
    // tower passed only the user-provided args, leaving the callee to read
    // uninitialized arg-register slots for any param the caller skipped —
    // a real heap pointer from a prior call's leftover state, which when
    // dereferenced for `options.session` silently hung in the dispatch chain.
    let mut method_param_counts: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    // Parallel `(class, method) → has_rest_param` map. Closes #484:
    // `b.with(1)` on `class Builder { with<T>(id, ...args: T extends void ?
    // [] | [void] : [T]): this }` left `args` as undefined because the
    // codegen-side dispatch table didn't track the rest bit, so the
    // call site never bundled trailing args (zero, in that test) into
    // a `js_array_alloc(0)` rest array. The conditional rest type is
    // a red herring — even `...args: any[]` would have shown the same
    // signature gap, except for the freestanding-function path which
    // already had `func_signatures.has_rest`.
    let mut method_has_rest: std::collections::HashMap<(String, String), bool> =
        std::collections::HashMap::new();
    let mut method_has_synthetic_arguments: std::collections::HashMap<(String, String), bool> =
        std::collections::HashMap::new();
    let mut method_arguments_length_only: std::collections::HashMap<(String, String), bool> =
        std::collections::HashMap::new();
    for cls in &hir.classes {
        for m in &cls.methods {
            let key = (cls.name.clone(), m.name.clone());
            method_param_counts.insert(key.clone(), m.params.len());
            let has_rest = m.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert(key.clone(), true);
            }
            if m.params
                .last()
                .is_some_and(|param| param.arguments_object.is_some())
            {
                method_has_synthetic_arguments.insert(key.clone(), true);
            }
            if arguments::method_supports_arguments_length_direct_abi(m) {
                method_arguments_length_only.insert(key, true);
            }
        }
        // Issue #894: track static methods too. Effect's `static pipe()` /
        // `static annotations()` synthesize a trailing `...arguments` rest
        // param when the body reads `arguments`. The StaticMethodCall
        // lowering at `expr.rs::Expr::StaticMethodCall` reads
        // `method_has_rest` to decide whether to bundle trailing args into
        // a rest array; without this, `Cls.pipe(a, b)` calls the method
        // with 2 scalar args while the signature expects (rest_array),
        // and `arguments.length` reads garbage / undefined.
        for sm in &cls.static_methods {
            let key = static_method_registry_key(&sm.name);
            method_param_counts.insert((cls.name.clone(), key.clone()), sm.params.len());
            let has_rest = sm.params.iter().any(|p| p.is_rest);
            if has_rest {
                method_has_rest.insert((cls.name.clone(), key.clone()), true);
            }
            if sm
                .params
                .last()
                .is_some_and(|param| param.arguments_object.is_some())
            {
                method_has_synthetic_arguments.insert((cls.name.clone(), key), true);
            }
        }
    }
    for ic in &opts.imported_classes {
        let effective_name = ic.effective_name();
        for (i, mname) in ic.method_names.iter().enumerate() {
            // Default to 0 if the source side hasn't populated method_param_counts
            // yet (legacy ImportedClass with no parallel Vec). 0 means "no padding".
            let count = ic.method_param_counts.get(i).copied().unwrap_or(0);
            // Register under the canonical class name and the local alias if any.
            if ic.namespace.is_none() {
                method_param_counts.insert((ic.name.clone(), mname.clone()), count);
            }
            if effective_name != ic.name {
                method_param_counts.insert((effective_name.clone(), mname.clone()), count);
            }
            // Issue #672: same propagation for the rest-flag side. Without this,
            // call sites to imported-class methods with `...rest` parameters
            // skipped the rest-array packing path, leaving trailing positional
            // args either dropped or silently spread into the next slot —
            // `c.cmd("SET", "k", "v")` reached the callee as `args = "k"`.
            if ic.method_has_rest.get(i).copied().unwrap_or(false) {
                if ic.namespace.is_none() {
                    method_has_rest.insert((ic.name.clone(), mname.clone()), true);
                }
                if effective_name != ic.name {
                    method_has_rest.insert((effective_name.clone(), mname.clone()), true);
                }
            }
            if ic
                .method_has_synthetic_arguments
                .get(i)
                .copied()
                .unwrap_or(false)
            {
                if ic.namespace.is_none() {
                    method_has_synthetic_arguments.insert((ic.name.clone(), mname.clone()), true);
                }
                if effective_name != ic.name {
                    method_has_synthetic_arguments
                        .insert((effective_name.clone(), mname.clone()), true);
                }
            }
            if ic
                .method_arguments_length_only
                .get(i)
                .copied()
                .unwrap_or(false)
            {
                method_arguments_length_only.insert((ic.name.clone(), mname.clone()), true);
                if effective_name != ic.name {
                    method_arguments_length_only
                        .insert((effective_name.clone(), mname.clone()), true);
                }
            }
        }
        for (i, method_name) in ic.static_method_names.iter().enumerate() {
            let registry_name = static_method_registry_key(method_name);
            let count = ic.static_method_param_counts.get(i).copied().unwrap_or(0);
            if ic.namespace.is_none() {
                method_param_counts.insert((ic.name.clone(), registry_name.clone()), count);
            }
            if effective_name != ic.name {
                method_param_counts.insert((effective_name.clone(), registry_name.clone()), count);
            }
            if ic.static_method_has_rest.get(i).copied().unwrap_or(false) {
                if ic.namespace.is_none() {
                    method_has_rest.insert((ic.name.clone(), registry_name.clone()), true);
                }
                if effective_name != ic.name {
                    method_has_rest.insert((effective_name.clone(), registry_name.clone()), true);
                }
            }
            if ic
                .static_method_has_synthetic_arguments
                .get(i)
                .copied()
                .unwrap_or(false)
            {
                if ic.namespace.is_none() {
                    method_has_synthetic_arguments
                        .insert((ic.name.clone(), registry_name.clone()), true);
                }
                if effective_name != ic.name {
                    method_has_synthetic_arguments
                        .insert((effective_name.clone(), registry_name), true);
                }
            }
        }
    }
    MethodSignatureTables {
        method_param_counts,
        method_has_rest,
        method_has_synthetic_arguments,
        method_arguments_length_only,
    }
}
