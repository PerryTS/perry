//! Emits the paired nonnegative-index method bodies used by guarded indexed
//! loop cloning. Kept separate from the artifact traversal so adding a clone
//! does not push that orchestration module back over the source-size gate.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::module::LlModule;
use crate::strings::StringPool;

use super::method::compile_method;
use super::opts::CrossModuleCtx;

pub(super) struct IndexedMethodArtifactsCtx<'a> {
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

pub(super) fn compile_indexed_method_clones(
    c: IndexedMethodArtifactsCtx<'_>,
    nonnegative_index_params: &[u32],
) -> Result<()> {
    let IndexedMethodArtifactsCtx {
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
        Some(nonnegative_index_params),
        false,
        false,
        false,
        false,
    )
    .with_context(|| {
        format!(
            "lowering nonnegative-index method clone '{}::{}'",
            class.name, method.name
        )
    })?;

    if cross_module
        .guarded_falsy_field_default_methods
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
            Some(nonnegative_index_params),
            false,
            false,
            true,
            false,
        )
        .with_context(|| {
            format!(
                "lowering guarded false-field-default indexed clone '{}::{}'",
                class.name, method.name
            )
        })?;
    }

    // Compose the exact receiver-shape and nonnegative-index facts in one
    // body. The externally published proven-receiver wrapper is emitted with
    // the receiver-shaped generic body below; it guards the live index and
    // routes here without giving up fixed receiver-field offsets.
    if let Some(fact) = cross_module
        .pshape_methods
        .get(&(class.name.clone(), method.name.clone()))
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
            Some(nonnegative_index_params),
            false,
            false,
            false,
            false,
        )
        .with_context(|| {
            format!(
                "lowering receiver-shaped nonnegative-index clone '{}::{}'",
                class.name, method.name
            )
        })?;
    }

    if super::typed_abi::nonnegative_index_fast_array_params(method, nonnegative_index_params)
        .is_empty()
    {
        return Ok(());
    }
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
        Some(nonnegative_index_params),
        true,
        false,
        false,
        false,
    )
    .with_context(|| {
        format!(
            "lowering fallback-free indexed-array method clone '{}::{}'",
            class.name, method.name
        )
    })
}
