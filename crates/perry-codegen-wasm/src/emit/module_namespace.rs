//! The namespace object of `import * as W from "./mod"` on the web target.
//!
//! A member read written on the namespace (`W.x`) resolves at compile time to the
//! source module's global, function or class. `W` used as a value is the namespace
//! object the runtime's `module_namespace` makes once per source module: its exports
//! sorted on a null prototype, tagged `Module`, not extensible. An exported
//! promoted-let reads its wasm global on every access, so the binding stays live.

use super::js_fallback::js_string_literal;
use super::locals::resolve_module_idx_by_source;
use super::*;

/// What `import * as <local>` exposes, per consumer module.
pub(super) struct NamespaceImport {
    /// The source module's index: the namespace object's identity.
    pub(super) source: usize,
    /// The source module's public names, as `collect_exported_names` gives them.
    pub(super) members: Vec<String>,
    /// The members that are classes, by the class's own name.
    pub(super) classes: BTreeMap<String, String>,
}

/// Resolve module `mod_idx`'s export `name` to the name of the class it binds,
/// following `export { K as L }`, `export { K } from` and `export * from`.
pub(super) fn resolve_export_to_class(
    modules: &[(String, perry_hir::ir::Module)],
    name_to_idx: &std::collections::HashMap<&str, usize>,
    mod_idx: usize,
    name: &str,
    depth: u32,
) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let m = &modules[mod_idx].1;
    let local_class = |local: &str| m.classes.iter().find(|c| c.name == local);
    for export in &m.exports {
        match export {
            perry_hir::ir::Export::Named { local, exported } if exported == name => {
                if let Some(c) = local_class(local) {
                    return Some(c.name.clone());
                }
                // The exported local may itself be an import binding.
                for import in m.imports.iter().filter(|i| !i.type_only) {
                    for spec in &import.specifiers {
                        if let perry_hir::ir::ImportSpecifier::Named {
                            imported,
                            local: il,
                        } = spec
                        {
                            if il == local {
                                let si = resolve_source_module_idx(modules, import, name_to_idx)?;
                                return resolve_export_to_class(
                                    modules,
                                    name_to_idx,
                                    si,
                                    imported,
                                    depth - 1,
                                );
                            }
                        }
                    }
                }
            }
            perry_hir::ir::Export::ReExport {
                source,
                imported,
                exported,
            } if exported == name => {
                let si = resolve_module_idx_by_source(modules, source)?;
                return resolve_export_to_class(modules, name_to_idx, si, imported, depth - 1);
            }
            _ => {}
        }
    }
    if let Some(c) = m.classes.iter().find(|c| c.is_exported && c.name == name) {
        return Some(c.name.clone());
    }
    if name == "default" {
        return None;
    }
    m.exports.iter().find_map(|export| match export {
        perry_hir::ir::Export::ExportAll { source } => {
            resolve_module_idx_by_source(modules, source)
                .filter(|&si| si != mod_idx)
                .and_then(|si| resolve_export_to_class(modules, name_to_idx, si, name, depth - 1))
        }
        _ => None,
    })
}

impl WasmModuleEmitter {
    /// The namespace import `local` names in the module being emitted, if it is one.
    pub(super) fn namespace_import(&self, local: &str) -> Option<&NamespaceImport> {
        self.namespace_imports
            .get(&(self.current_mod_idx, local.to_string()))
    }

    /// `W.<name>` in JS-fallback code: the member's NaN-boxed value, or `None` when the
    /// member resolves to nothing at compile time.
    pub(super) fn js_namespace_member(&self, local: &str, name: &str) -> Option<String> {
        let key = (self.current_mod_idx, format!("{local}.{name}"));
        if let Some(&gidx) = self.imported_var_globals.get(&key) {
            return Some(format!(
                "u64ToF64(wasmInstance.exports.__wasm_global_{gidx}.value)"
            ));
        }
        if let Some(&func_idx) = self.imported_ns_funcs.get(&key) {
            return Some(self.js_function_value(func_idx));
        }
        let class_name = self.namespace_import(local)?.classes.get(name)?;
        Some(format!(
            "fromJsValue(__memDispatch.class_ref({}, {}))",
            js_string_literal(class_name),
            self.class_ctor_slot(class_name)
        ))
    }

    /// `W` as a value in JS-fallback code.
    pub(super) fn js_namespace_value(&self, local: &str) -> Option<String> {
        let ns = self.namespace_import(local)?;
        let mut values = Vec::new();
        let mut globals = Vec::new();
        for name in &ns.members {
            let key = (self.current_mod_idx, format!("{local}.{name}"));
            if let Some(&gidx) = self.imported_var_globals.get(&key) {
                globals.push(format!("{}: {gidx}", js_string_literal(name)));
            } else {
                let value = self
                    .js_namespace_member(local, name)
                    .map_or_else(|| "undefined".to_string(), |v| format!("toJsValue({v})"));
                values.push(format!("{}: {value}", js_string_literal(name)));
            }
        }
        Some(format!(
            "fromJsValue(__memDispatch.module_namespace({}, {{{}}}, {{{}}}))",
            ns.source,
            values.join(", "),
            globals.join(", ")
        ))
    }
}

impl<'a> FuncEmitCtx<'a> {
    /// `W` as a value: `module_namespace(source, values, globals)`, where `values`
    /// holds the function and class members and `globals` the wasm global index of
    /// each promoted-let member.
    pub(super) fn emit_namespace_value(&mut self, func: &mut Function, local: &str) {
        let Some(ns) = self.emitter.namespace_import(local) else {
            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            return;
        };
        let source = ns.source;
        let mut values = Vec::new();
        let mut globals = Vec::new();
        for name in &ns.members {
            let key = (self.emitter.current_mod_idx, format!("{local}.{name}"));
            if let Some(&gidx) = self.emitter.imported_var_globals.get(&key) {
                globals.push((name.clone(), Expr::Number(gidx as f64)));
            } else if self.emitter.imported_ns_funcs.contains_key(&key)
                || ns.classes.contains_key(name)
            {
                let member = Expr::PropertyGet {
                    object: Box::new(Expr::ExternFuncRef {
                        name: local.to_string(),
                        param_types: Vec::new(),
                        return_type: perry_hir::types::Type::Any,
                    }),
                    property: name.clone(),
                    byte_offset: 0,
                    byte_end: 0,
                };
                values.push((name.clone(), member));
            } else {
                values.push((name.clone(), Expr::Undefined));
            }
        }
        self.emit_frame_begin(func, 3);
        self.emit_store_const(func, 0, source as f64);
        self.emit_store_arg(func, 1, &Expr::Object(values));
        self.emit_store_arg(func, 2, &Expr::Object(globals));
        self.emit_memcall(func, "module_namespace", 3);
    }

    /// `W.<name>` for a class member: the class value.
    pub(super) fn emit_namespace_class_member(
        &mut self,
        func: &mut Function,
        local: &str,
        name: &str,
    ) -> bool {
        let Some(class_name) = self
            .emitter
            .namespace_import(local)
            .and_then(|ns| ns.classes.get(name))
            .cloned()
        else {
            return false;
        };
        self.emit_expr(func, &Expr::ClassRef(class_name));
        true
    }
}
