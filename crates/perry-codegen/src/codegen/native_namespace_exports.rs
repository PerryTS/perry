//! Runtime getters for namespace re-exports of Perry-native modules.

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::{DOUBLE, I64, PTR};

pub(super) fn emit_native_namespace_reexport_getters(
    llmod: &mut LlModule,
    hir: &HirModule,
    module_prefix: &str,
) {
    // A namespace re-export of a compiler-native module has no compiled
    // source module (and therefore no `@__perry_ns_<prefix>` global) behind
    // it. Expose it as a zero-argument value getter on the re-exporting module
    // so ordinary named imports can materialize the runtime-native namespace:
    //
    //   export * as NodeWS from "ws"
    //   import { NodeWS } from "./NodeSocket"
    //
    // The driver classifies the consumer binding as `imported_vars`, so its
    // ExternFuncRef value path calls this getter rather than creating a closure
    // around a nonexistent function export.
    for export in &hir.exports {
        let perry_hir::Export::NamespaceReExport { source, name } = export else {
            continue;
        };
        if !perry_hir::NATIVE_MODULES.contains(&source.strip_prefix("node:").unwrap_or(source)) {
            continue;
        }
        let getter_name = format!("perry_fn_{}__{}", module_prefix, name);
        if llmod.has_function(&getter_name) {
            continue;
        }
        let (source_global, source_len) = llmod.add_string_constant(source);
        let getter = llmod.define_function(&getter_name, DOUBLE, vec![]);
        let _ = getter.create_block("entry");
        let blk = getter.block_mut(0).unwrap();
        if let Some(install) = crate::nm_install::nm_install_symbol(source) {
            blk.call_void(install, &[]);
        }
        let value = blk.call(
            DOUBLE,
            "js_create_native_module_namespace",
            &[
                (PTR, &format!("@{}", source_global)),
                (I64, &source_len.to_string()),
            ],
        );
        blk.ret(DOUBLE, &value);
    }
}
