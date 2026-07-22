//! HIR → JavaScript emitter
//!
//! Recursively translates HIR statements and expressions into JavaScript source code.

use perry_hir::ir::*;
use perry_hir::types::{FuncId, GlobalId, LocalId};
use std::collections::{BTreeMap, BTreeSet};

/// App metadata baked into compile-time `perry/system` introspection APIs
/// (`getAppVersion`/`getAppBuildNumber`/`getBundleId`) when emitting JS.
///
/// Mirrors `perry_codegen::AppMetadata` — duplicated here so this crate
/// doesn't take a backend dep on perry-codegen.
#[derive(Debug, Clone)]
pub struct AppMetadata {
    pub version: String,
    pub build_number: i64,
    pub bundle_id: String,
}

impl Default for AppMetadata {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            build_number: 1,
            bundle_id: "com.perry.app".to_string(),
        }
    }
}

/// JavaScript code emitter that translates HIR to JavaScript.
pub struct JsEmitter {
    /// Output buffer
    output: String,
    /// Current indentation level
    indent: usize,
    /// Mapping from LocalId to generated variable name
    local_names: BTreeMap<LocalId, String>,
    /// Mapping from GlobalId to generated variable name
    global_names: BTreeMap<GlobalId, String>,
    /// Mapping from FuncId to generated function name
    func_names: BTreeMap<FuncId, String>,
    /// Set of variable names already used (to avoid collisions)
    used_names: BTreeSet<String>,
    /// Module name (for cross-module references)
    // #854: captured at construction for future cross-module reference
    // emission; not read back on the current single-module emit path.
    #[allow(dead_code)]
    module_name: String,
    /// Exported names from this module
    exported_names: BTreeSet<String>,
    /// Whether to mangle (obfuscate) variable and function names
    minify: bool,
    /// Counter for generating short mangled names
    mangle_counter: usize,
    /// App metadata baked into compile-time `perry/system` introspection APIs.
    app_metadata: AppMetadata,
}

impl JsEmitter {
    pub fn new(module_name: &str, minify: bool) -> Self {
        Self::with_metadata(module_name, minify, AppMetadata::default())
    }

    pub fn with_metadata(module_name: &str, minify: bool, app_metadata: AppMetadata) -> Self {
        Self {
            output: String::with_capacity(8192),
            indent: 0,
            local_names: BTreeMap::new(),
            global_names: BTreeMap::new(),
            func_names: BTreeMap::new(),
            used_names: BTreeSet::new(),
            module_name: module_name.to_string(),
            exported_names: BTreeSet::new(),
            minify,
            mangle_counter: 0,
            app_metadata,
        }
    }

    /// Emit a complete module and return the JavaScript source
    pub fn emit_module(mut self, module: &Module) -> String {
        // Collect exported names
        for export in &module.exports {
            if let Export::Named { local, exported } = export {
                self.exported_names.insert(exported.clone());
                let _ = local; // used below during function/class naming
            }
        }

        // Pre-register function names
        for func in &module.functions {
            let name = self.make_func_name(&func.name, func.id);
            self.func_names.insert(func.id, name);
        }
        for class in &module.classes {
            if let Some(ctor) = &class.constructor {
                self.func_names
                    .insert(ctor.id, format!("_ctor_{}", class.name));
            }
            for method in &class.methods {
                self.func_names
                    .insert(method.id, format!("{}_{}", class.name, method.name));
            }
            for method in &class.static_methods {
                self.func_names
                    .insert(method.id, format!("{}_static_{}", class.name, method.name));
            }
        }

        // When minifying, reserve class and enum names to prevent mangled name collisions
        if self.minify {
            for class in &module.classes {
                self.used_names.insert(class.name.clone());
            }
            for en in &module.enums {
                self.used_names.insert(en.name.clone());
            }
        }

        // Pre-register global names
        for global in &module.globals {
            let name = if self.minify && !self.exported_names.contains(&global.name) {
                self.next_mangled_name()
            } else {
                self.sanitize_name(&global.name)
            };
            self.used_names.insert(name.clone());
            self.global_names.insert(global.id, name);
        }

        // Emit enums
        for en in &module.enums {
            self.emit_enum(en);
        }

        // Emit global variable declarations
        for global in &module.globals {
            self.emit_global(global);
        }

        // Pre-register module-level init local names so functions can reference them
        // (functions are emitted before init statements, so without this,
        //  get_local_name falls back to _l{id} instead of the actual variable name)
        for stmt in &module.init {
            if let Stmt::Let { id, name, .. } = stmt {
                self.make_local_name(name, *id);
            }
        }

        // Emit classes
        for class in &module.classes {
            self.emit_class(class);
        }

        // Emit top-level functions
        for func in &module.functions {
            self.emit_function(func);
        }

        // Emit init statements (top-level code)
        for stmt in &module.init {
            self.emit_stmt(stmt);
        }

        // Emit exports object
        if !self.exported_names.is_empty() {
            // We'll handle exports via the return value of the IIFE wrapper in lib.rs
        }

        self.output
    }

    /// Get the list of exported names for use by the IIFE wrapper
    pub fn get_exported_names(&self) -> &BTreeSet<String> {
        &self.exported_names
    }

    // --- Indentation helpers ---

    pub(super) fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    pub(super) fn writeln(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    // --- Name generation ---

    pub(super) fn sanitize_name(&mut self, name: &str) -> String {
        let sanitized: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        // Avoid JS reserved words
        let result = match sanitized.as_str() {
            "abstract" | "arguments" | "await" | "boolean" | "break" | "byte" | "case"
            | "catch" | "char" | "class" | "const" | "continue" | "debugger" | "default"
            | "delete" | "do" | "double" | "else" | "enum" | "eval" | "export" | "extends"
            | "false" | "final" | "finally" | "float" | "for" | "function" | "goto" | "if"
            | "implements" | "import" | "in" | "instanceof" | "int" | "interface" | "let"
            | "long" | "native" | "new" | "null" | "package" | "private" | "protected"
            | "public" | "return" | "short" | "static" | "super" | "switch" | "synchronized"
            | "this" | "throw" | "throws" | "transient" | "true" | "try" | "typeof"
            | "undefined" | "var" | "void" | "volatile" | "while" | "with" | "yield" => {
                format!("_{}", sanitized)
            }
            _ => sanitized,
        };

        result
    }

    pub(super) fn make_local_name(&mut self, name: &str, id: LocalId) -> String {
        if let Some(existing) = self.local_names.get(&id) {
            return existing.clone();
        }
        let final_name = if self.minify && !self.exported_names.contains(name) {
            self.next_mangled_name()
        } else {
            let base = self.sanitize_name(name);
            if self.used_names.contains(&base) {
                // #854: the loop unconditionally overwrites `n` on the
                // first iteration, so the prior `base.clone()` was dead.
                let mut n;
                let mut counter = 2;
                loop {
                    n = format!("{}_{}", base, counter);
                    if !self.used_names.contains(&n) {
                        break;
                    }
                    counter += 1;
                }
                n
            } else {
                base
            }
        };
        self.used_names.insert(final_name.clone());
        self.local_names.insert(id, final_name.clone());
        final_name
    }

    pub(super) fn get_local_name(&self, id: LocalId) -> String {
        self.local_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_l{}", id))
    }

    pub(super) fn get_global_name(&self, id: GlobalId) -> String {
        self.global_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_g{}", id))
    }

    pub(super) fn get_func_name(&self, id: FuncId) -> String {
        self.func_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_f{}", id))
    }

    pub(super) fn make_func_name(&mut self, name: &str, id: FuncId) -> String {
        if let Some(existing) = self.func_names.get(&id) {
            return existing.clone();
        }
        let final_name = if self.minify && !self.exported_names.contains(name) {
            self.next_mangled_name()
        } else {
            let base = self.sanitize_name(name);
            if self.used_names.contains(&base) {
                format!("{}_{}", base, id)
            } else {
                base
            }
        };
        self.used_names.insert(final_name.clone());
        final_name
    }

    /// Generate the next short mangled name, skipping collisions and reserved words.
    pub(super) fn next_mangled_name(&mut self) -> String {
        loop {
            let candidate = gen_short_name(self.mangle_counter);
            self.mangle_counter += 1;
            if !self.used_names.contains(&candidate) && !is_js_reserved(&candidate) {
                return candidate;
            }
        }
    }
}

// --- Submodules ---

mod calls;
mod decls;
mod exprs;
mod exprs_more;
mod helpers;
mod native;
mod stmts;

// --- Internal re-exports for sibling modules (`use super::*;`) ---

pub(super) use helpers::{gen_short_name, is_js_reserved, is_valid_identifier};
