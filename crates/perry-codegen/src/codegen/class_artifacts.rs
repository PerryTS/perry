//! Emits the per-class artifact family: method bodies and their typed/guarded
//! clones, computed members, accessors, the standalone constructor, and static
//! methods. Kept separate from the artifact traversal so the class walk stays
//! grouped without pushing that orchestration module over the source-size gate.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use anyhow::{Context, Result};

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::strings::StringPool;

use super::artifact_context::OptsView;
use super::ctor_arity::synthesized_ctor_param_count;
use super::indexed_method_artifacts::{compile_indexed_method_clones, IndexedMethodArtifactsCtx};
use super::method::{
    compile_method, compile_static_method, compile_typed_f64_method,
    compile_typed_f64_receiver_method, compile_typed_i1_method, compile_typed_i32_method,
    compile_typed_string_method,
};
use super::opts::CrossModuleCtx;
use super::ordinary_method_artifacts::{
    compile_ordinary_method_artifacts, OrdinaryMethodArtifactsCtx,
};
use super::typed_abi::TypedFunctionTrampolineKind;

/// Borrowed inputs for the per-class artifact walk. Mirrors the subset of
/// `ModuleArtifactsCtx` the class loop reads.
pub(super) struct ClassArtifactsCtx<'a> {
    pub progress: &'a super::CompileProgress,
    pub llmod: &'a mut LlModule,
    pub target_triple: &'a str,
    pub strings: &'a mut StringPool,
    pub hir: &'a HirModule,
    pub module_prefix: &'a String,
    pub class_table: &'a HashMap<String, &'a perry_hir::Class>,
    pub class_ids: &'a HashMap<String, u32>,
    pub enum_table: &'a HashMap<(String, String), perry_hir::EnumValue>,
    pub module_globals: &'a HashMap<u32, String>,
    pub module_global_types: &'a HashMap<u32, perry_hir::types::Type>,
    pub static_field_globals: &'a HashMap<(String, String), String>,
    pub method_names: &'a HashMap<(String, String), String>,
    pub func_names: &'a HashMap<u32, String>,
    pub func_signatures: &'a HashMap<u32, (usize, bool, bool, bool)>,
    pub func_synthetic_arguments: &'a HashSet<u32>,
    pub module_boxed_vars: &'a HashSet<u32>,
    pub closure_rest_params: &'a HashMap<u32, usize>,
    pub imported_class_stubs: &'a [perry_hir::Class],
    pub opts: OptsView<'a>,
    pub cross_module: &'a CrossModuleCtx,
}

/// Lower every class defined in this module: instance methods (plus their
/// typed/indexed/proven-`this` clones), computed members, accessors, the
/// standalone constructor, and statics.
pub(super) fn emit_class_artifacts(c: ClassArtifactsCtx<'_>) -> Result<()> {
    let ClassArtifactsCtx {
        progress,
        llmod,
        target_triple,
        strings,
        hir,
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
        closure_rest_params,
        imported_class_stubs,
        opts,
        cross_module,
    } = c;

    // Lower each class method as `perry_method_<modprefix>__<class>__<name>(
    // this_box, arg0, arg1, ...) -> double`. Methods are emitted as
    // standalone LLVM functions; the dispatch in `lower_call` calls
    // them directly.
    let classes_started = Instant::now();
    let class_progress_step = (hir.classes.len() / 20).max(1);
    for (class_index, class) in hir.classes.iter().enumerate() {
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
                compile_typed_f64_receiver_method(
                    llmod,
                    class,
                    method,
                    method_names,
                    receiver,
                    crate::target_layout::object_header_size_bytes(target_triple),
                )
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
            if let Some(nonnegative_index_params) = c
                .cross_module
                .nonnegative_index_methods
                .get(&(class.name.clone(), method.name.clone()))
            {
                compile_indexed_method_clones(
                    IndexedMethodArtifactsCtx {
                        llmod,
                        class,
                        method,
                        func_names,
                        strings,
                        classes: class_table,
                        methods: method_names,
                        module_globals,
                        module_global_types,
                        import_function_prefixes: opts.import_function_prefixes,
                        enums: enum_table,
                        static_field_globals,
                        class_ids,
                        func_signatures,
                        func_synthetic_arguments,
                        module_boxed_vars,
                        closure_rest_params,
                        cross_module,
                    },
                    nonnegative_index_params,
                )?;
            }
            compile_ordinary_method_artifacts(
                OrdinaryMethodArtifactsCtx {
                    llmod,
                    class,
                    method,
                    func_names,
                    strings,
                    classes: class_table,
                    methods: method_names,
                    module_globals,
                    module_global_types,
                    import_function_prefixes: opts.import_function_prefixes,
                    enums: enum_table,
                    static_field_globals,
                    class_ids,
                    func_signatures,
                    func_synthetic_arguments,
                    module_boxed_vars,
                    closure_rest_params,
                    cross_module,
                },
                typed_public_trampoline,
            )?;
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
                None,
                None,
                false,
                false,
                false,
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
                None,
                None,
                false,
                false,
                false,
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
                None,
                None,
                false,
                false,
                false,
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
                    opts.constructor_param_counts,
                );
                let found_params: Vec<perry_hir::Param> = (0..n)
                    .map(|i| perry_hir::Param {
                        id: 0xFFFF_0000 + i as u32,
                        name: format!("__forward_arg{}", i),
                        ty: perry_hir::types::Type::Any,
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
                return_type: perry_hir::types::Type::Void,
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
                None,
                None,
                false,
                false,
                false,
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
        let done = class_index + 1;
        if done == hir.classes.len() || done % class_progress_step == 0 {
            progress.items(
                "classes (methods, constructors, and statics)",
                done,
                hir.classes.len(),
                classes_started,
            );
        }
    }

    Ok(())
}
