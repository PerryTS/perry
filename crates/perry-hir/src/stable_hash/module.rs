//! `SH` impls for the top-level `Module` and its imports/exports.
//! Split out of `stable_hash.rs` (no behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

// --- Module ----------------------------------------------------------------

impl SH for Module {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Module {
            name,
            imports,
            exports,
            classes,
            interfaces,
            type_aliases,
            enums,
            globals,
            functions,
            script_global_functions,
            references_global_this,
            annexb_global_undefined_names,
            init,
            exported_native_instances,
            exported_func_return_native_instances,
            exported_objects,
            exported_functions,
            widgets,
            uses_fetch,
            uses_webassembly,
            extern_funcs,
            init_was_unrolled,
            has_top_level_await,
            init_kind,
            async_step_closures,
            closure_display_names,
            class_display_names,
            closure_source_text,
            async_generator_funcs,
            gen_param_prologue_len,
        } = self;
        name.hash(h);
        imports.hash(h);
        exports.hash(h);
        classes.hash(h);
        interfaces.hash(h);
        type_aliases.hash(h);
        enums.hash(h);
        globals.hash(h);
        functions.hash(h);
        // #5579: both drive the globalThis function-reflection codegen, so
        // they participate in the stable hash (else a cached object could omit
        // the reflection or emit it under the wrong gate).
        script_global_functions.hash(h);
        references_global_this.hash(h);
        annexb_global_undefined_names.hash(h);
        init.hash(h);
        exported_native_instances.hash(h);
        exported_func_return_native_instances.hash(h);
        exported_objects.hash(h);
        exported_functions.hash(h);
        widgets.hash(h);
        uses_fetch.hash(h);
        uses_webassembly.hash(h);
        extern_funcs.hash(h);
        init_was_unrolled.hash(h);
        has_top_level_await.hash(h);
        init_kind.hash(h);
        // HashSet has nondeterministic iteration order; sort for stable hashing.
        let mut ids: Vec<u32> = async_step_closures.iter().copied().collect();
        ids.sort_unstable();
        ids.hash(h);
        // #3664: async-generator func_ids participate in codegen (drive the
        // async-generator registry calls), so they're part of the stable hash.
        let mut async_gen_ids: Vec<u32> = async_generator_funcs.iter().copied().collect();
        async_gen_ids.sort_unstable();
        async_gen_ids.hash(h);
        // HashMap has nondeterministic iteration order; sort by key.
        let mut display_pairs: Vec<(u32, &String)> =
            closure_display_names.iter().map(|(k, v)| (*k, v)).collect();
        display_pairs.sort_unstable_by_key(|(k, _)| *k);
        for (id, name) in display_pairs {
            id.hash(h);
            name.hash(h);
        }
        // #5592: class `.name` overrides drive js_register_class_name codegen,
        // so they participate in the stable hash.
        let mut class_name_pairs: Vec<(u32, &String)> =
            class_display_names.iter().map(|(k, v)| (*k, v)).collect();
        class_name_pairs.sort_unstable_by_key(|(k, _)| *k);
        for (id, name) in class_name_pairs {
            id.hash(h);
            name.hash(h);
        }
        // #4101: function source text participates in codegen (drives the
        // js_register_function_source calls), so include it in the hash.
        let mut source_pairs: Vec<(u32, &String)> =
            closure_source_text.iter().map(|(k, v)| (*k, v)).collect();
        source_pairs.sort_unstable_by_key(|(k, _)| *k);
        for (id, src) in source_pairs {
            id.hash(h);
            src.hash(h);
        }
        // Generator param-prologue lengths drive the transform's prologue lift,
        // which changes codegen output — include in the stable hash.
        let mut prologue_pairs: Vec<(u32, usize)> = gen_param_prologue_len
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        prologue_pairs.sort_unstable_by_key(|(k, _)| *k);
        for (id, len) in prologue_pairs {
            id.hash(h);
            (len as u64).hash(h);
        }
    }
}

impl SH for ModuleInitKind {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            ModuleInitKind::Eager => tag(h, 0),
            ModuleInitKind::Deferred => tag(h, 1),
        }
    }
}

// --- Imports / Exports / ModuleKind ---------------------------------------

impl SH for ModuleKind {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            ModuleKind::NativeCompiled => tag(h, 0),
            ModuleKind::NativeRust => tag(h, 1),
            ModuleKind::Interpreted => tag(h, 2),
        }
    }
}

impl SH for Import {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Import {
            source,
            specifiers,
            is_native,
            module_kind,
            resolved_path,
            type_only,
            is_dynamic,
            is_dynamic_target,
            is_deferred_require,
            is_adopted_require,
        } = self;
        source.hash(h);
        specifiers.hash(h);
        is_native.hash(h);
        module_kind.hash(h);
        resolved_path.hash(h);
        type_only.hash(h);
        is_dynamic.hash(h);
        is_dynamic_target.hash(h);
        is_deferred_require.hash(h);
        is_adopted_require.hash(h);
    }
}

impl SH for ImportSpecifier {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            ImportSpecifier::Named { imported, local } => {
                tag(h, 0);
                imported.hash(h);
                local.hash(h);
            }
            ImportSpecifier::Default { local } => {
                tag(h, 1);
                local.hash(h);
            }
            ImportSpecifier::Namespace { local } => {
                tag(h, 2);
                local.hash(h);
            }
        }
    }
}

impl SH for Export {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            Export::Named { local, exported } => {
                tag(h, 0);
                local.hash(h);
                exported.hash(h);
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } => {
                tag(h, 1);
                source.hash(h);
                imported.hash(h);
                exported.hash(h);
            }
            Export::ExportAll { source } => {
                tag(h, 2);
                source.hash(h);
            }
            Export::NamespaceReExport { source, name } => {
                tag(h, 3);
                source.hash(h);
                name.hash(h);
            }
        }
    }
}
