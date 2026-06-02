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
            async_generator_funcs,
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
        } = self;
        source.hash(h);
        specifiers.hash(h);
        is_native.hash(h);
        module_kind.hash(h);
        resolved_path.hash(h);
        type_only.hash(h);
        is_dynamic.hash(h);
        is_dynamic_target.hash(h);
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
