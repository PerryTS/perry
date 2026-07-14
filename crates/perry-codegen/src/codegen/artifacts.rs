//! Tail of `compile_module`: emits closure bodies, class methods +
//! ctors + statics, wrapper symbols (function-value wrappers,
//! ExternFuncRef closure wrappers, export-rename aliases + stubs,
//! method closure-call wrappers), namespace globals + extern declares,
//! the module entry function, and finally the string-pool init.
//!
//! Split out of `codegen/mod.rs` purely to keep mod.rs under the
//! 2000-line LOC budget. No behavior changes — the function body
//! below is a verbatim move of the original inline block.

use std::collections::HashMap;

use anyhow::{Context, Result};
use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I64, VOID};

use super::closure::{
    compile_closure, compile_typed_f64_closure, compile_typed_i1_closure,
    compile_typed_i32_closure, compile_typed_string_closure,
};
use super::entry::compile_module_entry;
use super::helpers::{function_body_returns_generator_object, sanitize, scoped_fn_name};
use super::method::{
    compile_method, compile_static_method, compile_typed_f64_method,
    compile_typed_f64_receiver_method, compile_typed_i1_method, compile_typed_i32_method,
    compile_typed_string_method,
};
use super::opts::CrossModuleCtx;
use super::spec_function_length;
use super::typed_abi::TypedFunctionTrampolineKind;

/// Read-only view of the `CompileOptions` fields that the artifact
/// emission step references via `opts.X`. Bundled into a struct so the
/// moved block (originally written against `let opts = …;` of type
/// `CompileOptions`) can keep its `opts.X` syntax without holding a
/// `&CompileOptions` borrow — that borrow is unavailable at the call
/// site, because `compile_module`'s prelude moves several `opts`
/// fields into `CrossModuleCtx` before invoking this function.
struct OptsView<'a> {
    import_function_prefixes: &'a std::collections::HashMap<String, String>,
    imported_classes: &'a [super::opts::ImportedClass],
    is_entry_module: bool,
    non_entry_module_prefixes: &'a [String],
    output_type: &'a str,
}
use super::string_pool::emit_string_pool;

/// All the data computed by the prelude of `compile_module` that the
/// tail half (this file) needs. Bundled so the call from
/// `compile_module` stays a single line; field names mirror the
/// in-prelude local names so the moved block reads unchanged once
/// destructured.
pub(super) struct ModuleArtifactsCtx<'a> {
    pub llmod: &'a mut LlModule,
    pub strings: &'a mut StringPool,
    pub hir: &'a HirModule,
    pub import_function_prefixes: &'a std::collections::HashMap<String, String>,
    pub imported_classes: &'a [super::opts::ImportedClass],
    pub is_entry_module: bool,
    pub non_entry_module_prefixes: &'a [String],
    pub output_type: &'a str,
    pub module_prefix: &'a String,
    pub class_table: &'a HashMap<String, &'a perry_hir::Class>,
    pub class_ids: &'a HashMap<String, u32>,
    pub enum_table: &'a HashMap<(String, String), perry_hir::EnumValue>,
    pub module_globals: &'a HashMap<u32, String>,
    pub module_global_types: &'a HashMap<u32, perry_types::Type>,
    pub static_field_globals: &'a HashMap<(String, String), String>,
    pub method_names: &'a HashMap<(String, String), String>,
    pub func_names: &'a HashMap<u32, String>,
    pub func_signatures: &'a HashMap<u32, (usize, bool, bool, bool)>,
    pub func_synthetic_arguments: &'a std::collections::HashSet<u32>,
    pub module_boxed_vars: &'a std::collections::HashSet<u32>,
    /// Typed-ABI capture-representation oracle: module-wide `Stmt::Let` types
    /// MINUS boxed ids (#5869). Only the typed closure clones read this.
    pub module_local_types: &'a HashMap<u32, perry_types::Type>,
    /// #6369: receiver-type oracle for closure bodies — the same module-wide
    /// `Stmt::Let` types with no representation filtering, mirroring the
    /// `module_global_types` seed that `compile_function` / `compile_method`
    /// already use. Feeds `FnCtx.local_types` only.
    pub module_receiver_types: &'a HashMap<u32, perry_types::Type>,
    pub closure_rest_params: &'a HashMap<u32, usize>,
    pub closure_synthetic_arguments: &'a std::collections::HashSet<u32>,
    pub closure_rest_and_arguments: &'a std::collections::HashSet<u32>,
    pub closure_arities: &'a HashMap<u32, u32>,
    pub closure_lengths: &'a HashMap<u32, u32>,
    pub closure_arrow_functions: &'a std::collections::HashSet<u32>,
    pub closures: &'a [(perry_types::FuncId, perry_hir::Expr)],
    pub class_keys_init_data: &'a [(String, String, u32, Vec<u64>, Vec<u64>)],
    pub imported_class_stubs: &'a [perry_hir::Class],
    pub cross_module: &'a CrossModuleCtx,
}

/// The standalone-constructor arity Perry emits for `class`, accounting for the
/// JS spec default ctor `constructor(...args) { super(...args) }` that a class
/// with NO own constructor but WITH heritage inherits. Walks the ancestor chain
/// (local `class_table` + cross-module `imported_classes` ctor-param counts +
/// imported stubs) for the nearest ctor-bearing parent's user arity, mirroring
/// the `found_params` walk in the per-class ctor emission below.
///
/// This MUST agree with the synthesized standalone ctor's actual LLVM signature,
/// otherwise the runtime registers a different `total_params` than the function
/// declares and `replay_registered_class_constructor` forwards the wrong number
/// of args (Next.js wall 51: `class AppRouteRouteMatcher extends
/// _mod.RouteMatcher {}` emitted a 1-param forwarding ctor but registered it as
/// 0 params, so `new mod.AppRouteRouteMatcher(def)` dropped `def` before
/// `RouteMatcher(definition)` ran and every matcher's `this.definition` was a
/// garbage number).
fn synthesized_ctor_param_count(
    class: &perry_hir::Class,
    class_table: &HashMap<String, &perry_hir::Class>,
    imported_class_stubs: &[perry_hir::Class],
    imported_classes: &[super::opts::ImportedClass],
) -> usize {
    if let Some(c) = class.constructor.as_ref() {
        return c.params.len();
    }
    // A native parent (`extends Error` / `extends events.EventEmitter`) has its
    // own native construction path that consumes the construction args directly;
    // don't synthesize a forwarding ctor for it.
    if class.native_extends.is_some() {
        return 0;
    }
    // No heritage at all → nothing to forward.
    if class.extends_name.is_none() && class.extends_expr.is_none() {
        return 0;
    }
    // Walk ancestors for the nearest ctor-bearing parent's user arity.
    let mut cur = class.extends_name.clone();
    while let Some(pname) = cur {
        let imported_ctor_params = imported_classes
            .iter()
            .find(|i| i.local_alias.as_deref().unwrap_or(&i.name) == pname.as_str())
            .map(|ic| ic.constructor_param_count)
            .unwrap_or(0);
        if let Some(pclass) = class_table.get(pname.as_str()) {
            if let Some(pctor) = &pclass.constructor {
                return pctor.params.len();
            }
            if imported_ctor_params > 0 {
                return imported_ctor_params;
            }
            cur = pclass.extends_name.clone();
        } else if let Some(stub) = imported_class_stubs.iter().find(|c| c.name == pname) {
            if imported_ctor_params > 0 {
                return imported_ctor_params;
            }
            cur = stub.extends_name.clone();
        } else {
            break;
        }
    }
    // The parent's exact ctor arity is genuinely unavailable here in some build
    // modes: the auto-optimize / standalone path compiles each nested
    // `node_modules` module with an EMPTY `imported_classes` list and resolves
    // the cross-module parent purely as a runtime DYNAMIC parent
    // (`extends_expr` + `js_register_class_parent_dynamic`), so it is absent
    // from `class_table` and `imported_class_stubs` here. Without a forwarding
    // signature the synthesized `super()` dropped every construction arg
    // (Next.js wall 51: `class PagesRouteMatcher extends _mod.RouteMatcher {}`
    // → `RouteMatcher(definition)` saw garbage → every matcher's
    // `this.definition` was undefined). Forward a generous fixed band of
    // positional params: the `new` site pads missing slots with `undefined`,
    // and a parent ctor reading fewer params ignores the trailing `undefined`s,
    // so over-declaring is correct for any (non-native) parent up to this band.
    const UNRESOLVED_PARENT_FWD_ARITY: usize = 8;
    UNRESOLVED_PARENT_FWD_ARITY
}

/// Emit the artifact tail: bodies, wrappers, namespace globals, entry
/// function, string pool. Mirrors the in-prelude execution order of
/// the original `compile_module`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_module_artifacts(c: ModuleArtifactsCtx<'_>) -> Result<()> {
    // Destructure so the verbatim block below reads against the
    // original local names. `llmod` / `strings` are `&mut` bindings
    // (auto-reborrowed on each per-function call site below); the
    // rest are shared borrows.
    let ModuleArtifactsCtx {
        llmod,
        strings,
        hir,
        import_function_prefixes,
        imported_classes,
        is_entry_module,
        non_entry_module_prefixes,
        output_type,
        module_prefix,
        class_table,
        class_ids,
        enum_table,
        module_globals,
        module_global_types,
        static_field_globals,
        method_names,
        func_names,
        func_signatures,
        func_synthetic_arguments,
        module_boxed_vars,
        module_local_types,
        module_receiver_types,
        closure_rest_params,
        closure_synthetic_arguments,
        closure_rest_and_arguments,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
        closures,
        class_keys_init_data,
        imported_class_stubs,
        cross_module,
    } = c;

    // Local aliases so the verbatim moved block (which referenced
    // `opts.X`) still reads naturally without an `opts` borrow we
    // can't take after the prelude moved fields into `cross_module`.
    // Each alias is a stable view of the same data the original
    // `&opts.X` would have produced.
    let opts = OptsView {
        import_function_prefixes,
        imported_classes,
        is_entry_module,
        non_entry_module_prefixes,
        output_type,
    };

    for (func_id, closure_expr) in closures {
        if cross_module.typed_f64_closures.contains(func_id) {
            compile_typed_f64_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-f64 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_i1_closures.contains(func_id) {
            compile_typed_i1_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-i1 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_i32_closures.contains(func_id) {
            compile_typed_i32_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-i32 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_string_closures.contains(func_id) {
            compile_typed_string_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-string closure clone func_id={}", func_id))?;
        }
        compile_closure(
            llmod,
            *func_id,
            closure_expr,
            func_names,
            strings,
            class_table,
            method_names,
            module_globals,
            opts.import_function_prefixes,
            enum_table,
            static_field_globals,
            class_ids,
            func_signatures,
            func_synthetic_arguments,
            module_prefix,
            module_boxed_vars,
            module_receiver_types,
            closure_rest_params,
            cross_module,
        )
        .with_context(|| format!("lowering closure func_id={}", func_id))?;
    }

    // Lower each class method as `perry_method_<modprefix>__<class>__<name>(
    // this_box, arg0, arg1, ...) -> double`. Methods are emitted as
    // standalone LLVM functions; the dispatch in `lower_call` calls
    // them directly.
    for class in &hir.classes {
        for method in &class.methods {
            let typed_public_trampoline = if cross_module
                .typed_f64_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                Some(TypedFunctionTrampolineKind::F64)
            } else if cross_module
                .typed_i32_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                Some(TypedFunctionTrampolineKind::I32)
            } else if cross_module
                .typed_i1_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                Some(TypedFunctionTrampolineKind::I1)
            } else if cross_module
                .typed_string_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                Some(TypedFunctionTrampolineKind::StringRef)
            } else {
                None
            };
            if cross_module
                .typed_f64_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                compile_typed_f64_method(llmod, class, method, method_names).with_context(
                    || {
                        format!(
                            "lowering typed-f64 method clone '{}::{}'",
                            class.name, method.name
                        )
                    },
                )?;
            }
            if let Some(receiver) = cross_module
                .typed_f64_receiver_methods
                .get(&(class.name.clone(), method.name.clone()))
            {
                compile_typed_f64_receiver_method(llmod, class, method, method_names, receiver)
                    .with_context(|| {
                        format!(
                            "lowering typed-f64 receiver method clone '{}::{}'",
                            class.name, method.name
                        )
                    })?;
            }
            if cross_module
                .typed_i32_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                compile_typed_i32_method(llmod, class, method, method_names).with_context(
                    || {
                        format!(
                            "lowering typed-i32 method clone '{}::{}'",
                            class.name, method.name
                        )
                    },
                )?;
            }
            if cross_module
                .typed_i1_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                compile_typed_i1_method(llmod, class, method, method_names).with_context(|| {
                    format!(
                        "lowering typed-i1 method clone '{}::{}'",
                        class.name, method.name
                    )
                })?;
            }
            if cross_module
                .typed_string_methods
                .contains(&(class.name.clone(), method.name.clone()))
            {
                compile_typed_string_method(llmod, class, method, method_names).with_context(
                    || {
                        format!(
                            "lowering typed-string method clone '{}::{}'",
                            class.name, method.name
                        )
                    },
                )?;
            }
            compile_method(
                llmod,
                class,
                method,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                typed_public_trampoline,
                cross_module
                    .typed_f64_receiver_methods
                    .contains_key(&(class.name.clone(), method.name.clone())),
            )
            .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;
        }
        for member in class
            .computed_members
            .iter()
            .filter(|member| !member.is_static)
        {
            compile_method(
                llmod,
                class,
                &member.function,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
            )
            .with_context(|| {
                format!(
                    "lowering computed method '{}::{}'",
                    class.name, member.function.name
                )
            })?;
        }
        // Getters and setters are also methods, just registered under
        // a __get_/__set_ prefix in the registry. Emit their bodies
        // with the same prefix as the LLVM function name.
        for (prop, getter_fn) in &class.getters {
            let mut renamed = getter_fn.clone();
            renamed.name = format!("__get_{}", prop);
            // Static accessors compile with the static calling convention (no
            // `this` param; `this` is the implicit-this slot the constructor-ref
            // dispatch sets) so they match the CLASS_STATIC_ACCESSORS reader,
            // exactly like static computed accessors below.
            if class.static_accessor_fn_ids.contains(&getter_fn.id) {
                compile_static_method(
                    llmod,
                    class,
                    &renamed,
                    func_names,
                    strings,
                    class_table,
                    method_names,
                    module_globals,
                    module_global_types,
                    opts.import_function_prefixes,
                    enum_table,
                    static_field_globals,
                    class_ids,
                    func_signatures,
                    func_synthetic_arguments,
                    module_prefix,
                    module_boxed_vars,
                    closure_rest_params,
                    cross_module,
                )
                .with_context(|| format!("lowering static getter '{}::{}'", class.name, prop))?;
                continue;
            }
            compile_method(
                llmod,
                class,
                &renamed,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
            )
            .with_context(|| format!("lowering getter '{}::{}'", class.name, prop))?;
        }
        for (prop, setter_fn) in &class.setters {
            let mut renamed = setter_fn.clone();
            renamed.name = format!("__set_{}", prop);
            if class.static_accessor_fn_ids.contains(&setter_fn.id) {
                compile_static_method(
                    llmod,
                    class,
                    &renamed,
                    func_names,
                    strings,
                    class_table,
                    method_names,
                    module_globals,
                    module_global_types,
                    opts.import_function_prefixes,
                    enum_table,
                    static_field_globals,
                    class_ids,
                    func_signatures,
                    func_synthetic_arguments,
                    module_prefix,
                    module_boxed_vars,
                    closure_rest_params,
                    cross_module,
                )
                .with_context(|| format!("lowering static setter '{}::{}'", class.name, prop))?;
                continue;
            }
            compile_method(
                llmod,
                class,
                &renamed,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
            )
            .with_context(|| format!("lowering setter '{}::{}'", class.name, prop))?;
        }
        // Emit standalone constructor for cross-module use.
        // Compiled like a method: takes (i64 this, double arg0, ...) → void.
        // The constructor name matches the import declaration:
        // `<prefix>__<class>_constructor`.
        //
        // Refs #420: when class has no own ctor but extends a parent that
        // does, JS spec's default ctor is `constructor(...args) { super(...args); }`.
        // Adopt the closest ancestor-with-ctor's params as the synthesized
        // ctor's signature, so when this standalone ctor is called via
        // super() with the args meant for the grandparent, they can be
        // forwarded correctly. The compile_method post-init step (below)
        // emits the actual super-call.
        {
            let ctor_body = if let Some(c) = class.constructor.as_ref() {
                (c.params.clone(), c.body.clone(), c.captures.clone())
            } else if class.extends_name.is_some() || class.extends_expr.is_some() {
                // `extends_expr.is_some()` (with NO extends_name): a PURELY
                // dynamic parent — `class extends <local var> {}`, the shape a
                // bundled mysql2 promise-mixin takes (`module.exports = class
                // extends Pool { promise() {…} }`). It needs the same
                // forwarding signature; `synthesized_ctor_param_count` already
                // resolves it to the unresolved-parent fixed band. Previously
                // this fell to the empty-ctor branch, so the standalone ctor
                // dropped every construction arg AND never called super — the
                // instance silently lost all inherited ctor state (wall 7).
                // No own ctor + heritage → JS spec default ctor
                // `constructor(...args) { super(...args) }`. Synthesize forwarding
                // params matching the closest ancestor ctor's arity (incl.
                // cross-module parents) via the shared helper — its result is
                // ALSO what gets registered into CLASS_CONSTRUCTORS below, so the
                // emitted signature and the runtime `total_params` always agree
                // (Next.js wall 51). The actual `super(...)` is emitted by the
                // compile_method post-init step from these params positionally.
                let n = synthesized_ctor_param_count(
                    class,
                    class_table,
                    imported_class_stubs,
                    opts.imported_classes,
                );
                let found_params: Vec<perry_hir::Param> = (0..n)
                    .map(|i| perry_hir::Param {
                        id: 0xFFFF_0000 + i as u32,
                        name: format!("__forward_arg{}", i),
                        ty: perry_types::Type::Any,
                        default: None,
                        decorators: Vec::new(),
                        is_rest: false,
                        arguments_object: None,
                    })
                    .collect();
                (found_params, Vec::new(), Vec::new())
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };
            let ctor_as_method = perry_hir::Function {
                id: 0,
                name: format!("{}_constructor", class.name),
                type_params: Vec::new(),
                params: ctor_body.0,
                return_type: perry_types::Type::Void,
                body: ctor_body.1,
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
                is_exported: false,
                captures: ctor_body.2,
                decorators: Vec::new(),
            };
            compile_method(
                llmod,
                class,
                &ctor_as_method,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
            )
            .with_context(|| format!("lowering constructor for '{}'", class.name))?;
        }
        // Static methods compile as ID-qualified plain functions with no
        // `this` parameter and no class_stack push.
        for sm in &class.static_methods {
            compile_static_method(
                llmod,
                class,
                sm,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_prefix,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
            )
            .with_context(|| format!("lowering static method '{}::{}'", class.name, sm.name))?;
        }
        for member in class
            .computed_members
            .iter()
            .filter(|member| member.is_static)
        {
            compile_static_method(
                llmod,
                class,
                &member.function,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                module_global_types,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_prefix,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
            )
            .with_context(|| {
                format!(
                    "lowering static computed method '{}::{}'",
                    class.name, member.function.name
                )
            })?;
        }
    }

    // Emit FuncRef-as-value wrappers. For each user function, generate
    // a thin wrapper `__perry_wrap_<name>` whose signature matches the
    // closure-call ABI: `double(i64 this_closure, double arg0, double
    // arg1, ...)`. Most wrappers discard the closure pointer and forward the
    // args to the underlying function; generator wrappers reuse it to link the
    // returned iterator to the closure-cached `prototype`.
    //
    // The wrapper exists so that `apply(add, 3, 4)` can pass `add` as
    // a value and have `apply` call it via `js_closure_call2`. Without
    // a wrapper, the closure call would invoke `add(closure, 3, 4)`
    // (wrong calling convention) instead of `add(3, 4)`.
    //
    // Wrappers are emitted unconditionally for every user function;
    // dead-code elimination at link time will remove unused ones.
    for f in &hir.functions {
        let original_name = func_names.get(&f.id).cloned().unwrap();
        // Wrapper signature: i64 closure_ptr + N doubles for args. Cap at 16 to
        // match the `js_closure_call0..16` dispatch family (the closure-call ABI
        // tops out at 16 positional args; a function with more must be reached
        // via a rest-bundling path). Pre-fix this was capped at 5 with a stale
        // "js_closure_call only goes up to 5 args" comment, so any function
        // invoked as a closure value (object-literal method, callback, `apply`
        // target) with 6+ params silently dropped every argument past the 5th
        // — e.g. test262's `TemporalHelpers.assertDuration(d, y, mo, w, d, h, …)`
        // (11 args) read `hours` onward as 0.
        let arity = f.params.len().min(16);
        let arg_names: Vec<String> = (0..arity).map(|i| format!("%a{}", i)).collect();
        let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
        for name in &arg_names {
            wrap_params.push((DOUBLE, name.clone()));
        }
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with just the arg doubles.
        let call_args: Vec<(LlvmType, &str)> =
            arg_names.iter().map(|n| (DOUBLE, n.as_str())).collect();
        let mut result = blk.call(DOUBLE, &original_name, &call_args);
        if function_body_returns_generator_object(&f.body) {
            result = blk.call(
                DOUBLE,
                "js_generator_attach_closure_prototype",
                &[(DOUBLE, &result), (I64, "%this_closure")],
            );
        }
        blk.ret(DOUBLE, &result);
    }

    // Closes #837 / refs #836: emit `__perry_wrap_perry_fn_<src>__<exported>`
    // closure wrappers for every `Export::Named { local, exported }` rename
    // where `exported != local`. The regular wrapper loop above keys
    // wrappers on the local HIR name (`__perry_wrap_perry_fn_<src>__<local>`),
    // and the `perry_fn_<src>__<exported>` alias loop further down emits
    // the direct-call alias. The MISSING piece was the closure-wrapper
    // alias: consumer-side `expr.rs:~3819` builds
    // `__perry_wrap_perry_fn_<src>__<exported>` whenever an imported name
    // is referenced as a function VALUE (passed as a callback, stored in
    // a variable, etc.) — for renamed exports that symbol had no definition
    // and the link failed.
    //
    // Concrete failures pre-fix:
    //   * uuid's `v35.js`: `export default function v35(...)` — the local
    //     is `v35`, the exported name is `default`. Consumers
    //     (`v3.js`, `v5.js`) pass `v35` as a closure value to their own
    //     default wrappers, which transitively pulls
    //     `__perry_wrap_perry_fn_<v35.js>__default` into the link.
    //   * zod's `regexes.ts`: `const _null = ...; export { _null as null }`
    //     (same for `undefined`). A consumer that reads `regexes.null`
    //     /`regexes.undefined` as a value falls through to the
    //     function-shape branch (no `imported_vars` entry under the
    //     renamed name) and references the wrapper symbol.
    //
    // Function-renames forward to the local function body (same shape as
    // the regular wrapper above). Variable-renames have no callable body —
    // emit a no-op that returns undefined, matching the
    // `__perry_wrap_perry_unknown_func` fallback pattern below.
    {
        use std::collections::HashSet;
        let mut emitted_export_wrappers: HashSet<String> = HashSet::new();
        // Pre-compute the local-name → Function lookup. The renamed
        // export may resolve to a real HIR function (forward to it) or
        // to something else (variable / class / type / `export default
        // function NAME` whose body the HIR lowerer never recorded) —
        // for the latter we emit a no-op so the link still succeeds.
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            if local == exported {
                continue;
            }
            let exported_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported)
            );
            // Skip if a wrapper with this exact name already exists — the
            // regular wrapper loop above may have produced it when the
            // *local* HIR name happens to sanitize to the same symbol.
            if llmod.has_function(&exported_wrap) {
                continue;
            }
            if !emitted_export_wrappers.insert(exported_wrap.clone()) {
                continue;
            }
            // Determine the local target. Prefer a real HIR function;
            // fall back to a no-op (variable/class/type rename).
            if let Some(f) = func_by_local_name.get(local.as_str()) {
                let arity = f.params.len().min(32);
                let mut wrap_params: Vec<(LlvmType, String)> =
                    vec![(I64, "%this_closure".to_string())];
                for i in 0..arity {
                    wrap_params.push((DOUBLE, format!("%a{}", i)));
                }
                let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let target = scoped_fn_name(module_prefix, &f.name);
                let call_args: Vec<(LlvmType, String)> =
                    (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                let call_args_ref: Vec<(LlvmType, &str)> =
                    call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                let result = blk.call(DOUBLE, &target, &call_args_ref);
                blk.ret(DOUBLE, &result);
            } else {
                // Variable / class / type rename — no callable function
                // body to forward to. Emit a no-op returning undefined,
                // mirroring `__perry_wrap_perry_unknown_func`.
                //
                // External linkage (the default): consumers in OTHER
                // translation units take this symbol's address through
                // `js_closure_alloc_singleton(@__perry_wrap_<sym>)`, so
                // the wrapper must be visible across TUs. Internal
                // linkage gets DCE'd because the defining TU never
                // references the wrapper itself — `expr.rs:~3819`
                // imports it via `pending_declares`, which generates a
                // `declare` extern in the consumer TU.
                let wf = llmod.define_function(
                    &exported_wrap,
                    DOUBLE,
                    vec![
                        (I64, "%this_closure".to_string()),
                        (DOUBLE, "%a0".to_string()),
                        (DOUBLE, "%a1".to_string()),
                        (DOUBLE, "%a2".to_string()),
                        (DOUBLE, "%a3".to_string()),
                        (DOUBLE, "%a4".to_string()),
                    ],
                );
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                blk.ret(DOUBLE, "0x7FFC000000000001");
            }
        }
    }

    // Closes #836: two additional producer-side emissions for cross-module
    // exports that the regular wrapper/getter/stub loops above leave behind.
    //
    // Sub-bug A — sanitize-mismatch raw aliases. The producer side hashes
    // every export through `sanitize()` (replaces every non-alphanumeric +
    // non-underscore char with `_`), so `export const $ZodCheck` lands at
    // `perry_fn_<src>___ZodCheck` (one `_` from the `__` separator, two
    // from the sanitized `$`). The consumer side, in contrast, builds the
    // callee symbol with `import_origin_suffix()`, which returns the
    // exported name VERBATIM — so consumer references read
    // `perry_fn_<src>__$ZodCheck` and link-fail. The mismatch hits every
    // export whose name contains `$` (zod's `$ZodCheck`/`$ZodCheckString
    // Format`/`$constructor` family — ~80 symbols on the zod surface), or
    // any other character `sanitize()` rewrites. Fix: for every named
    // export where `sanitize(name) != name`, emit raw-name aliases
    // forwarding to the sanitized definition.
    //
    // Sub-bug B — missing `__perry_wrap_perry_fn_<src>__<exported>` for
    // `local == exported` non-function exports. Concrete shape:
    //   import * as z from "./external.js";
    //   export { z };
    //   export default z;
    // The `Export::Named { local: "z", exported: "z" }` entry is skipped
    // by every loop above because `local == exported` and `z` is not a
    // HIR function (it's a namespace import alias). A consumer that
    // imports `{ z }` from this module references the wrapper symbol
    // when reading `z` as a value (e.g. `console.log(z)` or passing it
    // as a callback), and link-fails on the missing wrapper. Fix: emit
    // a no-op wrapper for every named export whose `local==exported`
    // name is not the body of a HIR function (and where no wrapper has
    // already been emitted by the regular `__perry_wrap_<fn>` loop).
    //
    // Note: anonymous `export default function() {...}` produces zero
    // exports and zero functions in the HIR today (lower.rs drops it on
    // the floor — separate bug, will need its own PR to wire up the
    // synthetic `default` name). That path is OUT OF SCOPE here; the
    // test below uses a named default to sidestep it.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            let sanitized = sanitize(exported);

            // Sub-bug A: emit raw-name aliases when the exported name
            // sanitizes to a different symbol. Two aliases per mismatch:
            //   * `perry_fn_<src>__<raw_exported>` — value/getter form,
            //     forwards to the already-emitted sanitized symbol.
            //   * `__perry_wrap_perry_fn_<src>__<raw_exported>` — closure-
            //     wrapper form, forwards to the sanitized wrapper if it
            //     exists, otherwise emits a no-op (matches the variable/
            //     class branch in the #837 loop above).
            if sanitized != *exported {
                let sanitized_target = format!("perry_fn_{}__{}", module_prefix, sanitized);
                let raw_target = format!("perry_fn_{}__{}", module_prefix, exported);
                if !llmod.has_function(&raw_target) && emitted_aliases.insert(raw_target.clone()) {
                    // Look up the param count to match the sanitized
                    // target's arity. Default to 0 — that matches the
                    // variable-getter shape (zero-arg fetcher) which is
                    // the common case here. Functions with `$`-prefixed
                    // names (rare, but possible) need to match arity;
                    // we look it up from the HIR if the local resolves
                    // to a known function.
                    let param_count = func_by_local_name
                        .get(local.as_str())
                        .map(|f| f.params.len())
                        .unwrap_or(0);
                    let wrap_params: Vec<(LlvmType, String)> = (0..param_count)
                        .map(|i| (DOUBLE, format!("%a{}", i)))
                        .collect();
                    let wf = llmod.define_function(&raw_target, DOUBLE, wrap_params);
                    let _ = wf.create_block("entry");
                    let blk = wf.block_mut(0).unwrap();
                    let arg_names: Vec<String> =
                        (0..param_count).map(|i| format!("%a{}", i)).collect();
                    let call_args: Vec<(LlvmType, &str)> =
                        arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    let result = blk.call(DOUBLE, &sanitized_target, &call_args);
                    blk.ret(DOUBLE, &result);
                }
                let raw_wrap = format!("__perry_wrap_perry_fn_{}__{}", module_prefix, exported);
                let sanitized_wrap =
                    format!("__perry_wrap_perry_fn_{}__{}", module_prefix, sanitized);
                if !llmod.has_function(&raw_wrap) && emitted_aliases.insert(raw_wrap.clone()) {
                    if llmod.has_function(&sanitized_wrap) {
                        // Forward to the sanitized wrapper. Both have the
                        // same closure-call ABI: (i64 this_closure, double
                        // a0, …, double a4).
                        let wf = llmod.define_function(
                            &raw_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let result = blk.call(
                            DOUBLE,
                            &sanitized_wrap,
                            &[
                                (I64, "%this_closure"),
                                (DOUBLE, "%a0"),
                                (DOUBLE, "%a1"),
                                (DOUBLE, "%a2"),
                                (DOUBLE, "%a3"),
                                (DOUBLE, "%a4"),
                            ],
                        );
                        blk.ret(DOUBLE, &result);
                    } else {
                        // No sanitized wrapper either (variable/class/
                        // namespace re-export with sanitize-mismatch
                        // name). Emit a no-op returning undefined.
                        let wf = llmod.define_function(
                            &raw_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }

            // Sub-bug B: emit no-op wrapper for `local==exported` named
            // exports where local isn't a HIR function and no wrapper
            // is yet defined. Catches `import * as z; export { z };`
            // and any `export { ClassName }` or `export { someConst }`
            // where the consumer reads the value as a closure.
            //
            // Issue #967: when the `local==exported` name IS registered
            // in `hir.exported_functions` as an alias for a real function
            // body (the canonical shape: `function add(a,b){…}; export
            // default add;` lowers to `Export::Named { local: "default",
            // exported: "default" }` *and* pushes `("default", add_id)`
            // into `exported_functions`), the no-op wrapper short-circuits
            // any consumer-side closure dispatch through this default
            // import. The consumer's
            // `__perry_wrap_perry_fn_<src>__default` then transmutes a
            // function pointer that returns `undefined` no matter the args
            // — `const fn = add; fn(2,3)` evaluates to `undefined` instead
            // of `5`. Ramda/date-fns trip this on every `var sum =
            // reduce(add, 0)` style barrel where `add`/`reduce` are
            // default-exports of locally-named functions. Fix: when
            // `exported_functions` points the name at a real HIR function,
            // emit a forwarding wrapper to that function's body (mirrors
            // the `local != exported` branch at L2792 above).
            if local == exported && !func_by_local_name.contains_key(local.as_str()) {
                let exported_wrap = format!(
                    "__perry_wrap_perry_fn_{}__{}",
                    module_prefix,
                    sanitize(exported)
                );
                if !llmod.has_function(&exported_wrap)
                    && emitted_aliases.insert(exported_wrap.clone())
                {
                    // Check if this name is registered as an alias
                    // pointing at a real HIR function (`export default
                    // <namedFn>` shape).
                    let aliased_func: Option<&perry_hir::Function> = hir
                        .exported_functions
                        .iter()
                        .find(|(n, _)| n == exported)
                        .and_then(|(_, fid)| hir.functions.iter().find(|f| f.id == *fid));
                    if let Some(f) = aliased_func {
                        let arity = f.params.len().min(32);
                        let mut wrap_params: Vec<(LlvmType, String)> =
                            vec![(I64, "%this_closure".to_string())];
                        for i in 0..arity {
                            wrap_params.push((DOUBLE, format!("%a{}", i)));
                        }
                        let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let target = scoped_fn_name(module_prefix, &f.name);
                        let call_args: Vec<(LlvmType, String)> =
                            (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                        let call_args_ref: Vec<(LlvmType, &str)> =
                            call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                        let result = blk.call(DOUBLE, &target, &call_args_ref);
                        blk.ret(DOUBLE, &result);
                    } else {
                        let wf = llmod.define_function(
                            &exported_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }
        }
    }

    // Issue #774: emit closure-call wrappers for class instance methods
    // so `Expr::SuperPropertyGet` (value-form `super.<method>`) can
    // materialize them via `js_closure_alloc_singleton(@__perry_wrap_<method>)`.
    // Methods have signature `perry_method_<...>(this_box, args...)`;
    // the closure-call ABI is `(i64 closure, double a0, ...)` and
    // doesn't carry a separate `this`. The receiver therefore comes from
    // IMPLICIT_THIS, set by the dispatcher right before the call:
    //   * A method-style invocation of a stored super-method value
    //     (`this._complete = super._complete; obj._complete()` — rxjs's
    //     `OperatorSubscriber` forwarding `complete`/`error`/`next` to the
    //     base `Subscriber` when no override callback was supplied) sets
    //     IMPLICIT_THIS to the receiver, so the base method runs with the
    //     right `this`. Pre-fix this hardcoded `this=undefined`, so the
    //     forwarded `complete` never reached `this.destination.complete()`
    //     and the pipeline stalled (top-level await never settled, #5138).
    //   * A bare call (`const fn = super.greet; fn(x)`) leaves
    //     IMPLICIT_THIS undefined, matching strict-mode `this`.
    let mut emitted_wrappers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for class in &hir.classes {
        for method in &class.methods {
            let Some(method_fn_name) = method_names
                .get(&(class.name.clone(), method.name.clone()))
                .cloned()
            else {
                continue;
            };
            let wrap_name = format!("__perry_wrap_{}", method_fn_name);
            if !emitted_wrappers.insert(wrap_name.clone()) {
                continue;
            }
            let arity = method.params.len().min(32);
            let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
            for i in 0..arity {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            // Forward the call-site receiver (IMPLICIT_THIS) as `this`,
            // then the args. See the block comment above (#5138).
            let this_box = blk.call(DOUBLE, "js_implicit_this_get", &[]);
            let mut call_args: Vec<(LlvmType, String)> = Vec::with_capacity(arity + 1);
            call_args.push((DOUBLE, this_box));
            for i in 0..arity {
                call_args.push((DOUBLE, format!("%a{}", i)));
            }
            let call_args_ref: Vec<(LlvmType, &str)> =
                call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let result = blk.call(DOUBLE, &method_fn_name, &call_args_ref);
            blk.ret(DOUBLE, &result);
        }
    }

    // #337: emit an always-defined fallback wrapper for the
    // `perry_unknown_func` sentinel. `expr.rs::Expr::FuncRef` falls
    // through to `wrap_name = "__perry_wrap_perry_unknown_func"` when the
    // FuncRef's id isn't in `func_names` (cross-module reference whose
    // Source HIR wasn't lowered into THIS LLVM module — should normally
    // route to ExternFuncRef instead, but some HIR shapes still emit
    // FuncRef with an unresolvable id). Pre-fix the wrapper was never
    // defined and clang errored with `use of undefined value
    // @__perry_wrap_perry_unknown_func`. This stub returns TAG_UNDEFINED
    // (encoded as `f64::from_bits(0x7FFC_0000_0000_0001)` =
    // 1.7800590868057611e-307 — the canonical undefined sentinel matching
    // `value::TAG_UNDEFINED`); any runtime-side `js_closure_callN`
    // through this wrapper just observes "the callable returned
    // undefined", which is the right fail-closed behavior matching how
    // the `__perry_wrap_extern_*` wrappers handle missing extern
    // imported classes (see the existing comment block below).
    //
    // Emitted unconditionally — link-time DCE strips it when no
    // `Expr::FuncRef(unknown_id)` site exists in this module.
    {
        let wrap_name = "__perry_wrap_perry_unknown_func";
        let wf = llmod.define_function(
            wrap_name,
            DOUBLE,
            vec![
                (I64, "%this_closure".to_string()),
                (DOUBLE, "%a0".to_string()),
                (DOUBLE, "%a1".to_string()),
                (DOUBLE, "%a2".to_string()),
                (DOUBLE, "%a3".to_string()),
                (DOUBLE, "%a4".to_string()),
            ],
        );
        // Fix #420 (v0.5.576): internal linkage so multi-module
        // programs (drizzle-orm has 5+ modules each emitting this
        // fallback) don't fail link with `duplicate symbol
        // ___perry_wrap_perry_unknown_func`. Same pattern as the
        // `__perry_wrap_extern_*` wrappers below — comment block at
        // line ~1957 explicitly calls out that wrappers like this
        // should be `internal` linkage so each module gets its own
        // dead-code-eliminable copy.
        wf.linkage = "internal".to_string();
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // f64::from_bits(0x7FFC_0000_0000_0001) — TAG_UNDEFINED. Format
        // identical to how other expr.rs lowerings emit it.
        blk.ret(DOUBLE, "0x7FFC000000000001");
    }

    // Emit ExternFuncRef-as-value wrappers for every imported function in
    // `opts.import_function_prefixes`. Each gets a thin wrapper plus a
    // static `ClosureHeader` so the value can be passed as a callback,
    // stored in a variable, or used in a truthiness / equality check —
    // all the things you can do with a regular closure pointer.
    //
    // The wrappers are `internal` linkage so multiple modules can each
    // emit their own copy without colliding at link time. Dead-code
    // elimination strips wrappers for externs that are never referenced
    // as values.
    //
    // Why this exists: when an imported function appears as a STANDALONE
    // value (`if (this.ffi.setCursors)` capability check, `arr.forEach(
    // importedFn)` callback, or `someFn === otherFn` reference equality),
    // the lowering needs *some* JSValue to thread through. The previous
    // pragmatic fix returned `TAG_TRUE` — correct for truthiness but it
    // would crash at runtime the moment anything called the value via
    // `js_closure_callN` (the runtime would dereference garbage from
    // the function pointer's prefix bytes looking for a `ClosureHeader`).
    // The static-ClosureHeader approach makes those calls actually work:
    // `get_valid_func_ptr` reads `type_tag` at offset 12, sees
    // `CLOSURE_MAGIC = 0x434C4F53 ("CLOS")`, and dispatches to the
    // wrapper, which forwards the args to `perry_fn_<src>__<name>`.
    {
        use std::collections::HashSet;
        let mut emitted_wrappers: HashSet<String> = HashSet::new();
        // Build a quick lookup of imported class names (and their local aliases).
        // Classes have no `perry_fn_<src>__<Class>` symbol — method/constructor/
        // static dispatch happens via separate tables. For these we still need
        // the `__perry_extern_closure_*` global (other code may load it as a
        // value), but the wrapper body must NOT call a missing function: emit
        // a no-op that returns `undefined` so any indirect call through the
        // closure header fails closed instead of failing at link time.
        let mut imported_class_names: HashSet<String> = HashSet::new();
        for ic in opts.imported_classes {
            imported_class_names.insert(ic.name.clone());
            if let Some(alias) = &ic.local_alias {
                imported_class_names.insert(alias.clone());
            }
        }
        // Stable iteration order for deterministic IR output.
        let mut imports: Vec<(&String, &String)> = opts.import_function_prefixes.iter().collect();
        imports.sort_by(|a, b| a.0.cmp(b.0));
        for (name, source_prefix) in imports {
            let is_class = imported_class_names.contains(name);
            // Issue #678 followup: V8-fallback imports have no native target
            // — the wrapper body cannot call `perry_fn_<src>__<name>` because
            // that symbol doesn't exist. Emit the same no-op wrapper +
            // ClosureHeader as the imported-class branch so direct calls of
            // the function-reference-as-value still link (and fail closed at
            // runtime). Actual call sites — Call/PropertyGet-Call/namespace
            // member call — route through `emit_v8_export_call` and do NOT
            // touch this wrapper.
            let is_v8_import = cross_module
                .import_function_v8_specifiers
                .contains_key(name);
            let wrapper_name = format!("__perry_wrap_extern_{}__{}", source_prefix, name);
            if !emitted_wrappers.insert(wrapper_name.clone()) {
                continue;
            }
            if is_class || is_v8_import {
                // No-op wrapper + a closure header that points at it. The
                // wrapper returns NaN-tagged `undefined` so any indirect call
                // (`MyClass.somethingThatIsActuallyAFn()`) returns undefined.
                // Match the regular wrapper's calling convention — `%this_closure`
                // followed by 6 double params — so direct calls in the IR don't
                // tear off into garbage stack slots.
                let mut wrap_params: Vec<(LlvmType, String)> = Vec::with_capacity(7);
                wrap_params.push((I64, "%this_closure".to_string()));
                for i in 0..6 {
                    wrap_params.push((DOUBLE, format!("%a{}", i)));
                }
                let wf = llmod.define_function(&wrapper_name, DOUBLE, wrap_params);
                wf.linkage = "internal".to_string();
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let undef =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                blk.ret(DOUBLE, &undef);
                let global_name = format!("__perry_extern_closure_{}__{}", source_prefix, name);
                let init = format!("{{ ptr @{}, i32 0, i32 1129074515 }}", wrapper_name);
                llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
                continue;
            }
            // Issue #678: when a re-export rename routes this name to an
            // origin export with a different suffix (`export { default as
            // render }`), call into the origin's real symbol — `perry_fn_<
            // src>__default`, not `perry_fn_<src>__render`. The local
            // wrapper still uses the consumer-visible name so this
            // module's own callers can find it.
            let origin_suffix =
                crate::expr::import_origin_suffix(&cross_module.import_function_origin_names, name);
            let target_name = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
            // Look up the param count from the import metadata. Fall back
            // to 0 if missing — emits a no-arg wrapper, which is wrong
            // for nonzero-arity functions but won't break compilation.
            // (Read from `cross_module.imported_func_param_counts` rather
            // than `opts.imported_func_param_counts` because the latter
            // was moved into `cross_module` earlier in this function.)
            let param_count = cross_module
                .imported_func_param_counts
                .get(name)
                .copied()
                .unwrap_or(0);
            // Make sure the target is declared. The lazy-declares path
            // in `lower_call.rs::ExternFuncRef` only fires when the
            // function is actually CALLED — if it's only referenced as
            // a value, the declare would be missing without this.
            let param_types: Vec<crate::types::LlvmType> =
                std::iter::repeat_n(DOUBLE, param_count).collect();
            llmod.declare_function(&target_name, DOUBLE, &param_types);
            // Wrapper: `define internal double @__perry_wrap_extern_<src>__<name>(
            //              i64 %this_closure, double %a0, …, double %aN-1)`
            // discards the closure pointer and forwards the doubles to
            // `perry_fn_<src>__<name>`.
            let mut wrap_params: Vec<(LlvmType, String)> = Vec::with_capacity(param_count + 1);
            wrap_params.push((I64, "%this_closure".to_string()));
            for i in 0..param_count {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrapper_name, DOUBLE, wrap_params);
            wf.linkage = "internal".to_string();
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            let arg_names: Vec<String> = (0..param_count).map(|i| format!("%a{}", i)).collect();
            let call_args: Vec<(LlvmType, &str)> =
                arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = blk.call(DOUBLE, &target_name, &call_args);
            blk.ret(DOUBLE, &result);
            // Static `ClosureHeader` global pointing at the wrapper.
            // Layout matches `crates/perry-runtime/src/closure.rs`:
            //   { *const u8 func_ptr (8 bytes),
            //     u32 capture_count (4 bytes),
            //     u32 type_tag      (4 bytes) }
            // The runtime's `get_valid_func_ptr` reads `type_tag` at
            // offset 12 and validates against `CLOSURE_MAGIC = 0x434C4F53`
            // ("CLOS" in ASCII = 1129074515 decimal). If the magic doesn't
            // match, the call fast-paths to `undefined` instead of
            // dispatching, so any non-closure value passed where a closure
            // is expected fails closed rather than crashing.
            let global_name = format!("__perry_extern_closure_{}__{}", source_prefix, name);
            let init = format!("{{ ptr @{}, i32 0, i32 1129074515 }}", wrapper_name);
            llmod.add_internal_constant(&global_name, "{ ptr, i32, i32 }", &init);
        }
    }

    // Issue #100: emit the per-module `@__perry_ns_<prefix>` global iff
    // this module is the target of at least one dynamic `import()` site
    // anywhere in the program. Defined here with external linkage so the
    // consumer-side `Expr::DynamicImport` lowering (which may live in a
    // different LLVM module) can `load double, ptr @__perry_ns_<prefix>`.
    // Initialized to NaN-boxed `undefined`; the populator at the end of
    // `__perry_init_<prefix>` overwrites this with the real namespace
    // object built via `js_create_namespace`. Registered as a GC root
    // (same as every other module global) so the underlying ObjectHeader
    // survives sweeps after init returns.
    // Pre-emit each export-key string constant at module level. We add
    // them BEFORE compile_module_entry so the populator can reference
    // them by their stable `.str.N` global name. Stored alongside the
    // entries so the populator emit-site can look up `(global_name,
    // byte_len)` per key without rebuilding any LLVM IR. Vec entries
    // are parallel to `cross_module.namespace_entries`.
    let mut namespace_key_globals: Vec<(String, usize)> = Vec::new();
    // Issue #842: also emit `@__perry_ns_<prefix>` for side-effect-only
    // dynamic-import targets (`namespace_entries` empty but the module
    // is still a target). Without this, the consumer-side extern
    // declaration links to nothing. The empty-entries case still emits
    // the global; the populator below handles `n == 0` by calling
    // `js_create_namespace(0, ...)` which returns an empty object.
    if !cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target {
        let ns_name = format!("__perry_ns_{}", module_prefix);
        // Hex double literal for TAG_UNDEFINED (0x7FFC_0000_0000_0001).
        llmod.add_global(&ns_name, DOUBLE, "0x7FFC000000000001");
        for entry in &cross_module.namespace_entries {
            let (gname, byte_len) = llmod.add_string_constant(&entry.name);
            namespace_key_globals.push((gname, byte_len));
        }
    }
    // For each `Expr::DynamicImport` target this module dispatches to,
    // declare the foreign module's `@__perry_ns_<target_prefix>` as an
    // extern global so the dispatch site can load it. Deduped via
    // `BTreeSet` so a multi-path site that resolves to N targets emits
    // N declarations exactly once even when multiple `paths` arrays
    // share entries.
    if !cross_module.dynamic_import_path_to_prefix.is_empty() {
        let mut foreign_prefixes: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for prefix in cross_module.dynamic_import_path_to_prefix.values() {
            if prefix != module_prefix.as_str() {
                foreign_prefixes.insert(prefix.clone());
            }
        }
        for prefix in foreign_prefixes {
            // #3938: `__native_mod__<name>` / `__node_submod__<key>` sentinel
            // prefixes are resolved at the `Expr::DynamicImport` dispatch site
            // via runtime namespace builders (`js_create_native_module_namespace`
            // / `js_node_submodule_namespace`); they have no compiled-module
            // `@__perry_ns_<prefix>` global or `<prefix>__init` function, so the
            // extern decls below are dead. Worse, for slash-bearing submodule
            // names (`node:path/posix`, `node:util/types`) the `/` is illegal in
            // an LLVM global identifier and broke the whole module. Skip them.
            if prefix.starts_with("__native_mod__") || prefix.starts_with("__node_submod__") {
                continue;
            }
            let ns_name = format!("__perry_ns_{}", prefix);
            llmod.add_external_global(&ns_name, DOUBLE);
            // Issue #753: declare each dynamic-import target's `__init`
            // so the dispatch site in `Expr::DynamicImport` can call it
            // before loading the namespace. The wrapper-side init is
            // idempotent — calling it for an already-initialized
            // target costs a load + cmp + cond_br. For Deferred
            // targets it's the only thing that triggers their init.
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }
    }

    // Emit either `int main()` (entry module) or `void <prefix>__init()`
    // (non-entry module). The entry main calls each non-entry init in
    // order before running its own statements.
    compile_module_entry(
        llmod,
        hir,
        func_names,
        strings,
        class_table,
        method_names,
        module_globals,
        opts.import_function_prefixes,
        enum_table,
        static_field_globals,
        class_ids,
        func_signatures,
        func_synthetic_arguments,
        module_prefix,
        opts.is_entry_module,
        opts.non_entry_module_prefixes,
        module_boxed_vars,
        closure_rest_params,
        cross_module,
        opts.output_type,
        &namespace_key_globals,
    )
    .with_context(|| format!("lowering entry of module '{}'", hir.name))?;

    // Issue #392: pre-intern every user-class method name into the
    // string pool so `emit_string_pool` (which takes `&strings`) can
    // emit the `js_register_class_method(class_id, name_ptr, ...)`
    // calls in module init without needing mutable access to the
    // pool. Only iterate classes DEFINED in this module — the
    // `perry_method_*` symbols for imported classes live in the
    // defining module's object file. `imported_class_prefix` lists
    // every imported class name; we exclude those.
    for class in &hir.classes {
        let cid = class_ids.get(&class.name).copied().unwrap_or(0);
        if cid == 0 {
            continue;
        }
        for method in &class.methods {
            let _ = strings.intern(&method.name);
        }
        // #1788: intern static-method names too, so the
        // `js_register_class_static_method` registration loop in
        // emit_string_pool finds their bytes_global without re-running
        // through the string pool's mutable interner.
        for sm in &class.static_methods {
            let _ = strings.intern(&sm.name);
        }
        // Refs #486: also intern accessor property names so the cross-module
        // `js_register_class_getter` / setter registration loops in emit_string_pool
        // can find their bytes_global without re-running through the
        // string pool's mutable interner.
        for (prop, _) in &class.getters {
            let _ = strings.intern(prop);
        }
        for (prop, _) in &class.setters {
            let _ = strings.intern(prop);
        }
    }

    // After all user code is lowered, the string pool's contents are final.
    // Emit the bytes globals, handle globals, and the
    // `__perry_init_strings_<prefix>` function that runs once at startup.
    // The function name is scoped by module prefix so multiple modules
    // can each have their own string-pool init without colliding.
    // Issue #653: build wrapper-rest map for top-level user functions.
    // Each `__perry_wrap_<name>` was emitted above; if the underlying
    // function declares a rest param, we tell the runtime to bundle
    // trailing args into an array before invoking the wrapper. Without
    // this, calling a function-as-value via the closure-spread path
    // (`js_closure_call_apply_with_spread`) leaks raw element bits into
    // the rest-param slot.
    let user_fn_wrapper_rest: Vec<(String, usize)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            f.params.iter().position(|p| p.is_rest).and_then(|idx| {
                func_names
                    .get(&f.id)
                    .map(|name| (format!("__perry_wrap_{}", name), idx))
            })
        })
        .collect();

    // Refs #915 (gap 1 from #899): wrappers whose underlying function ends in
    // the HIR-synthesized `arguments` rest param. The wrapper symbol is keyed
    // the same way (`__perry_wrap_<scoped_name>`) but registered with the
    // synthetic-arguments runtime fn so `dispatch_rest_bundled` bundles ALL
    // passed args, matching JS spec semantics for `arguments.length`.
    let user_fn_wrapper_synthetic_arguments: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let last_is_synth_args = f
                .params
                .last()
                .map(|p| p.arguments_object.is_some())
                .unwrap_or(false);
            if last_is_synth_args {
                func_names
                    .get(&f.id)
                    .map(|name| format!("__perry_wrap_{}", name))
            } else {
                None
            }
        })
        .collect();
    let mut user_fn_wrapper_rest_and_arguments: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let last = f.params.last()?;
            let has_user_rest = f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            if last.arguments_object.is_some() && has_user_rest {
                func_names
                    .get(&f.id)
                    .map(|name| format!("__perry_wrap_{}", name))
            } else {
                None
            }
        })
        .collect();
    let mut user_fn_wrapper_rest = user_fn_wrapper_rest;
    let mut user_fn_wrapper_synthetic_arguments = user_fn_wrapper_synthetic_arguments;

    // #5134-followup: a renamed export (`export { local as exported }`, which
    // includes `export default function local`) gets a forwarding value wrapper
    // `__perry_wrap_perry_fn_<src>__<exported>` (emitted above), but the
    // rest/synthetic-`arguments` *metadata* loops below key only on the local
    // function name (`__perry_wrap_..._<local>`). So a consumer that referenced
    // the renamed export as a VALUE and called it via `.apply`/`.call`
    // (`compose`'s `pipe.apply(this, reverse(arguments))` in ramda — `pipe` is
    // `export default function pipe()` with a synthetic `arguments`) reached
    // `js_native_call_value` with an unregistered wrapper func_ptr, so
    // `lookup_closure_rest_full` missed and the args were dispatched positionally
    // instead of bundled — the variadic function saw `arguments.length === 0`.
    // Register the exported-alias wrapper symbol with the same metadata as the
    // local one so the runtime bundles correctly through the rename.
    {
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            if local == exported {
                continue;
            }
            let Some(f) = func_by_local_name.get(local.as_str()) else {
                continue;
            };
            let alias_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported)
            );
            // The registration loop (`string_pool.rs`) iterates
            // `user_fn_wrapper_rest`; the synthetic/rest_and_arguments sets only
            // *refine* which runtime fn each entry uses. So the alias must be
            // added to `user_fn_wrapper_rest` (keyed on the rest param index) in
            // EVERY case, plus the matching refinement set.
            let Some(rest_idx) = f.params.iter().position(|p| p.is_rest) else {
                continue;
            };
            let last_is_synth_args = f
                .params
                .last()
                .map(|p| p.arguments_object.is_some())
                .unwrap_or(false);
            let has_user_rest = f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            user_fn_wrapper_rest.push((alias_wrap.clone(), rest_idx));
            if last_is_synth_args && has_user_rest {
                user_fn_wrapper_rest_and_arguments.insert(alias_wrap);
            } else if last_is_synth_args {
                user_fn_wrapper_synthetic_arguments.insert(alias_wrap);
            }
        }
    }

    // Wrapper arities — ABI param count per top-level user-function wrapper.
    // Used by dynamic closure dispatch to pad omitted trailing parameters
    // before invoking the wrapper.
    let user_fn_wrapper_arity: Vec<(String, u32)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| (format!("__perry_wrap_{}", name), f.params.len() as u32))
        })
        .collect();

    // Wrapper lengths — ECMAScript-visible `.length`, which stops at the
    // first default parameter and before rest. Refs ramda's `converge` /
    // `juxt` / `useWith` chain that reads `.length` off function values to
    // compute curry arities.
    let user_fn_wrapper_length: Vec<(String, u32)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            func_names.get(&f.id).map(|name| {
                (
                    format!("__perry_wrap_{}", name),
                    spec_function_length(&f.params) as u32,
                )
            })
        })
        .collect();

    let user_fn_wrapper_async: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| f.is_async || f.was_plain_async)
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();

    let mut user_fn_wrapper_generator: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| !f.was_plain_async && function_body_returns_generator_object(&f.body))
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, expr) in closures {
        if let perry_hir::Expr::Closure { body, is_async, .. } = expr {
            if !*is_async && function_body_returns_generator_object(body) {
                user_fn_wrapper_generator
                    .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
            }
        }
    }
    // Strict-mode user functions (file-level `"use strict"` or body
    // directive), for OrdinaryCallBindThis in `call`/`apply`/`bind`: a
    // strict callee must observe the raw primitive `thisArg`, a sloppy one
    // gets it boxed. Same two symbol forms as the generator registries.
    let mut user_fn_wrapper_strict: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| f.is_strict)
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, expr) in closures {
        if let perry_hir::Expr::Closure { is_strict, .. } = expr {
            if *is_strict {
                user_fn_wrapper_strict
                    .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
            }
        }
    }

    // #3664: async-generator wrapper symbols, identified by the func_ids the
    // generator transform recorded (it cleared `is_async` before we get here,
    // so the body shape alone can't tell async generators from sync ones).
    // Named declarations use the `__perry_wrap_<name>` singleton symbol;
    // generator EXPRESSIONS use the inline `perry_closure_<modprefix>__<id>`
    // symbol — the same two symbol forms as `user_fn_wrapper_generator`.
    let mut user_fn_wrapper_async_generator: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| hir.async_generator_funcs.contains(&f.id))
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, _expr) in closures {
        if hir.async_generator_funcs.contains(func_id) {
            user_fn_wrapper_async_generator
                .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
        }
    }

    // Display names so `console.log` / `util.inspect` print `[Function:
    // <name>]` instead of `[Function (anonymous)]` (#1202). Two kinds:
    //   (a) Top-level `function name() {}` declarations — keyed against
    //       the singleton-wrapper address (`__perry_wrap_<name>`),
    //       because that's what `js_closure_alloc_singleton` stamps into
    //       ClosureHeader for `FuncRef` references. Skips empty / underscore-
    //       prefixed synthesized names (factories, iife, lambdas).
    //   (b) Arrow functions assigned to a binding (`const fn = () => …`)
    //       — keyed against the inline closure symbol
    //       `perry_closure_<modprefix>__<func_id>` that `js_closure_alloc`
    //       stamps into ClosureHeader for inline closures. We only
    //       harvest top-level `Stmt::Let { init: Closure }` shapes here;
    //       nested closures keep the anonymous label, matching Node
    //       (Node uses `inferred-name` only for direct assignments,
    //       which are exactly these top-level lets).
    // (a) Top-level user functions — keyed against the singleton-wrapper
    // address (`__perry_wrap_<sym>`). #2076: prefer the HIR display-name
    // override (set by lower_fn_expr / lower_method_prop) so synthetic
    // names like `__obj_method_method_42` register as `"method"`.
    let mut user_fn_display_names: Vec<(String, String)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let display = hir.closure_display_names.get(&f.id).cloned().or_else(|| {
                if f.name.is_empty() || f.name.starts_with('_') {
                    None
                } else {
                    Some(f.name.clone())
                }
            })?;
            func_names
                .get(&f.id)
                .map(|sym| (format!("__perry_wrap_{}", sym), display))
        })
        .collect();
    // (b) Closures bound to a top-level `let`/`const`. #2076: a named
    // function expression's own name takes precedence over the binding
    // name (`const bar = function namedBar(){}` ⇒ `"namedBar"`).
    let mut named_inline_closure_ids: std::collections::HashSet<perry_types::FuncId> =
        std::collections::HashSet::new();
    for stmt in &hir.init {
        if let perry_hir::Stmt::Let { name, init, .. } = stmt {
            if name.is_empty() || name.starts_with('_') {
                continue;
            }
            if let Some(perry_hir::Expr::Closure { func_id, .. }) = init {
                let display = hir
                    .closure_display_names
                    .get(func_id)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
                user_fn_display_names.push((sym, display));
                named_inline_closure_ids.insert(*func_id);
            }
        }
    }
    // (c) Inline closures with a HIR display name that weren't picked up
    // by (a) or (b) — e.g. an object-literal shorthand method that
    // captured locals or used `this` and lowered to an inline Closure
    // instead of a FuncRef. Skip ids already covered above and any id
    // that hir.functions already produced a wrapper entry for.
    let registered_fn_ids: std::collections::HashSet<perry_types::FuncId> =
        hir.functions.iter().map(|f| f.id).collect();
    // #3527: only register a display name for a `perry_closure_*` symbol when
    // that closure was actually materialized as an LLVM global (i.e. it is in
    // the `closures` set compiled above via `compile_closure`). `hir.closure_display_names`
    // can hold stale entries for closures that were never emitted — e.g.
    // `module.exports = function named(){}` records a display name for a fid that
    // CJS export lowering replaces with a different, materialized fid. Registering
    // a name for the stale fid emits a `js_register_function_name` call referencing
    // an undefined `@perry_closure_*` global, which makes `clang -c` fail with
    // "use of undefined value" (regression class of #318/#343).
    let materialized_closure_ids: std::collections::HashSet<perry_types::FuncId> =
        closures.iter().map(|(id, _)| *id).collect();
    for (func_id, display) in &hir.closure_display_names {
        if !materialized_closure_ids.contains(func_id) {
            continue;
        }
        if display.is_empty() || named_inline_closure_ids.contains(func_id) {
            continue;
        }
        if registered_fn_ids.contains(func_id) {
            continue;
        }
        let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
        user_fn_display_names.push((sym, display.clone()));
    }

    // #4101: collect retained function source text, keyed by the same
    // wrapper/closure symbol the name registration uses. Top-level functions
    // always have a `__perry_wrap_<name>` global (emitted unconditionally
    // above); inline closures only have a `perry_closure_*` global when
    // materialized, so gate those on `materialized_closure_ids` to avoid
    // referencing an undefined global (the #318/#343 clang-failure class).
    let mut user_fn_source: Vec<(String, String)> = Vec::new();
    for f in &hir.functions {
        if let Some(src) = hir.closure_source_text.get(&f.id) {
            if let Some(sym) = func_names.get(&f.id) {
                user_fn_source.push((format!("__perry_wrap_{}", sym), src.clone()));
            }
        }
    }
    for (func_id, src) in &hir.closure_source_text {
        if registered_fn_ids.contains(func_id) || !materialized_closure_ids.contains(func_id) {
            continue;
        }
        let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
        user_fn_source.push((sym, src.clone()));
    }

    // Wall 51: the standalone-ctor arity registered into CLASS_CONSTRUCTORS must
    // match the arity of the ctor function actually emitted above (which, for a
    // no-own-ctor class with heritage, is the synthesized `super(...args)`
    // forwarding ctor whose arity comes from the nearest ancestor ctor — possibly
    // cross-module). Compute that arity here with the same walk so the runtime
    // forwards the right number of construction args to the parent ctor.
    let ctor_arity_overrides: HashMap<String, u32> = class_table
        .iter()
        .filter(|(name, class)| **name == class.name && class.id != 0)
        .map(|(name, class)| {
            let n = synthesized_ctor_param_count(
                class,
                class_table,
                imported_class_stubs,
                opts.imported_classes,
            ) as u32;
            (name.clone(), n)
        })
        .collect();

    emit_string_pool(
        llmod,
        strings,
        module_prefix,
        class_keys_init_data,
        class_ids,
        class_table,
        &hir.class_display_names,
        &ctor_arity_overrides,
        closure_rest_params,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
        &user_fn_wrapper_rest,
        closure_synthetic_arguments,
        &user_fn_wrapper_synthetic_arguments,
        closure_rest_and_arguments,
        &user_fn_wrapper_rest_and_arguments,
        &user_fn_wrapper_arity,
        &user_fn_wrapper_length,
        &user_fn_wrapper_async,
        &user_fn_wrapper_generator,
        &user_fn_wrapper_async_generator,
        &user_fn_wrapper_strict,
        &user_fn_display_names,
        &user_fn_source,
    );

    Ok(())
}
