//! Emits the ordinary tagged-ABI method body and its additive guarded clones.
//! Kept separate from the artifact traversal so clone families stay grouped
//! without pushing that orchestration module over the source-size gate.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::module::LlModule;
use crate::strings::StringPool;

use super::method::compile_method;
use super::opts::CrossModuleCtx;
use super::typed_abi::TypedFunctionTrampolineKind;

pub(super) struct OrdinaryMethodArtifactsCtx<'a> {
    pub llmod: &'a mut LlModule,
    pub class: &'a perry_hir::Class,
    pub method: &'a perry_hir::Function,
    pub func_names: &'a HashMap<u32, String>,
    pub strings: &'a mut StringPool,
    pub classes: &'a HashMap<String, &'a perry_hir::Class>,
    pub methods: &'a HashMap<(String, String), String>,
    pub module_globals: &'a HashMap<u32, String>,
    pub module_global_types: &'a HashMap<u32, perry_hir::types::Type>,
    pub import_function_prefixes: &'a HashMap<String, String>,
    pub enums: &'a HashMap<(String, String), perry_hir::EnumValue>,
    pub static_field_globals: &'a HashMap<(String, String), String>,
    pub class_ids: &'a HashMap<String, u32>,
    pub func_signatures: &'a HashMap<u32, (usize, bool, bool, bool)>,
    pub func_synthetic_arguments: &'a HashSet<u32>,
    pub module_boxed_vars: &'a HashSet<u32>,
    pub closure_rest_params: &'a HashMap<u32, usize>,
    pub cross_module: &'a CrossModuleCtx,
}

pub(super) fn compile_ordinary_method_artifacts(
    c: OrdinaryMethodArtifactsCtx<'_>,
    typed_public_trampoline: Option<TypedFunctionTrampolineKind>,
) -> Result<()> {
    let OrdinaryMethodArtifactsCtx {
        llmod,
        class,
        method,
        func_names,
        strings,
        classes,
        methods,
        module_globals,
        module_global_types,
        import_function_prefixes,
        enums,
        static_field_globals,
        class_ids,
        func_signatures,
        func_synthetic_arguments,
        module_boxed_vars,
        closure_rest_params,
        cross_module,
    } = c;

    let guarded_index_public = typed_public_trampoline.is_none()
        && cross_module
            .nonnegative_index_methods
            .contains_key(&(class.name.clone(), method.name.clone()));

    compile_method(
        llmod,
        class,
        method,
        func_names,
        strings,
        classes,
        methods,
        module_globals,
        module_global_types,
        import_function_prefixes,
        enums,
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
            .contains_key(&(class.name.clone(), method.name.clone()))
            || guarded_index_public,
        None,
        None,
        false,
        false,
        false,
        false,
    )
    .with_context(|| format!("lowering method '{}::{}'", class.name, method.name))?;

    // A separate externally callable body keeps the public/runtime ABI exact:
    // dynamic dispatch still receives a marked argument bundle, while a
    // guarded direct caller may pass the actual argument count in the same
    // trailing tagged-value slot. The internal marker type makes exact
    // `arguments.length` reads lower to that scalar without changing source
    // HIR or teaching generic property dispatch about the specialized ABI.
    if super::arguments::method_supports_arguments_length_direct_abi(method) {
        let mut clone = method.clone();
        let synth_param = clone
            .params
            .last_mut()
            .expect("length-only arguments method has a synthetic parameter");
        synth_param.ty = perry_hir::types::Type::Named(
            super::arguments::SYNTHETIC_ARGUMENTS_LENGTH_TYPE.to_string(),
        );
        compile_method(
            llmod,
            class,
            &clone,
            func_names,
            strings,
            classes,
            methods,
            module_globals,
            module_global_types,
            import_function_prefixes,
            enums,
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
                "lowering scalar arguments-length clone of method '{}::{}'",
                class.name, method.name
            )
        })?;
    }

    if cross_module
        .guarded_undefined_method_params
        .contains_key(&(class.name.clone(), method.name.clone()))
    {
        compile_method(
            llmod,
            class,
            method,
            func_names,
            strings,
            classes,
            methods,
            module_globals,
            module_global_types,
            import_function_prefixes,
            enums,
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
            true,
            false,
        )
        .with_context(|| {
            format!(
                "lowering exact-undefined clone of method '{}::{}'",
                class.name, method.name
            )
        })?;
    }

    // Representation-selection Phase 5a: the additive `internal`
    // proven-`this` clone. Same HIR, same ABI, same shadow-bound tagged-at-rest
    // receiver slot; only `this.field` lowering differs. It is reached solely
    // from call sites that already prove the receiver's exact shape.
    if let Some(fact) = cross_module
        .pshape_methods
        .get(&(class.name.clone(), method.name.clone()))
    {
        let guarded_index_pshape = cross_module
            .nonnegative_index_methods
            .contains_key(&(class.name.clone(), method.name.clone()));
        compile_method(
            llmod,
            class,
            method,
            func_names,
            strings,
            classes,
            methods,
            module_globals,
            module_global_types,
            import_function_prefixes,
            enums,
            static_field_globals,
            class_ids,
            func_signatures,
            func_synthetic_arguments,
            module_boxed_vars,
            closure_rest_params,
            cross_module,
            None,
            guarded_index_pshape,
            Some(fact.clone()),
            None,
            false,
            false,
            false,
            false,
        )
        .with_context(|| {
            format!(
                "lowering proven-`this` clone of method '{}::{}'",
                class.name, method.name
            )
        })?;

        if cross_module
            .guarded_undefined_method_params
            .contains_key(&(class.name.clone(), method.name.clone()))
        {
            compile_method(
                llmod,
                class,
                method,
                func_names,
                strings,
                classes,
                methods,
                module_globals,
                module_global_types,
                import_function_prefixes,
                enums,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
                Some(fact.clone()),
                None,
                false,
                false,
                true,
                false,
            )
            .with_context(|| {
                format!(
                    "lowering proven-`this` exact-undefined clone of method '{}::{}'",
                    class.name, method.name
                )
            })?;
        }

        // #8607: a stricter provenance+containment clone. Synthetic immutable
        // aliases keep stable array-valued fields in local slots so existing
        // local-array loop optimizations see through repeated `this.field`.
        if let Some(cached_method) = crate::collectors::ptr_array_cached_method(class, method) {
            compile_method(
                llmod,
                class,
                &cached_method,
                func_names,
                strings,
                classes,
                methods,
                module_globals,
                module_global_types,
                import_function_prefixes,
                enums,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_boxed_vars,
                closure_rest_params,
                cross_module,
                None,
                false,
                Some(fact.clone()),
                None,
                false,
                true,
                false,
                false,
            )
            .with_context(|| {
                format!(
                    "lowering contained-receiver array-cache clone of method '{}::{}'",
                    class.name, method.name
                )
            })?;
        }
    }

    // #8774: internal exact-shape argument clone with the ordinary tagged ABI.
    // Every route guards all selected arguments before entry. If the receiver
    // is also proven, compose that fact into this same clone.
    if cross_module
        .pshape_arg_methods
        .contains_key(&(class.name.clone(), method.name.clone()))
    {
        compile_method(
            llmod,
            class,
            method,
            func_names,
            strings,
            classes,
            methods,
            module_globals,
            module_global_types,
            import_function_prefixes,
            enums,
            static_field_globals,
            class_ids,
            func_signatures,
            func_synthetic_arguments,
            module_boxed_vars,
            closure_rest_params,
            cross_module,
            None,
            false,
            cross_module
                .pshape_methods
                .get(&(class.name.clone(), method.name.clone()))
                .cloned(),
            None,
            false,
            false,
            false,
            true,
        )
        .with_context(|| {
            format!(
                "lowering exact-shape argument clone of method '{}::{}'",
                class.name, method.name
            )
        })?;
    }

    Ok(())
}
