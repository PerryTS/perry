//! HIR → JavaScript emitter
//!
//! Recursively translates HIR statements and expressions into JavaScript source code.

use perry_hir::ir::*;
use perry_types::{FuncId, GlobalId, LocalId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as FmtWrite;

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

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn writeln(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    // --- Name generation ---

    fn sanitize_name(&mut self, name: &str) -> String {
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

    fn make_local_name(&mut self, name: &str, id: LocalId) -> String {
        if let Some(existing) = self.local_names.get(&id) {
            return existing.clone();
        }
        let final_name = if self.minify && !self.exported_names.contains(name) {
            self.next_mangled_name()
        } else {
            let base = self.sanitize_name(name);
            if self.used_names.contains(&base) {
                let mut n = base.clone();
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

    fn get_local_name(&self, id: LocalId) -> String {
        self.local_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_l{}", id))
    }

    fn get_global_name(&self, id: GlobalId) -> String {
        self.global_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_g{}", id))
    }

    fn get_func_name(&self, id: FuncId) -> String {
        self.func_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("_f{}", id))
    }

    fn make_func_name(&mut self, name: &str, id: FuncId) -> String {
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
    fn next_mangled_name(&mut self) -> String {
        loop {
            let candidate = gen_short_name(self.mangle_counter);
            self.mangle_counter += 1;
            if !self.used_names.contains(&candidate) && !is_js_reserved(&candidate) {
                return candidate;
            }
        }
    }

    // --- Enum emission ---

    fn emit_enum(&mut self, en: &Enum) {
        self.write_indent();
        let _ = write!(self.output, "const {} = Object.freeze({{", en.name);
        for (i, member) in en.members.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            match &member.value {
                EnumValue::Number(n) => {
                    let _ = write!(self.output, "{}: {}", member.name, n);
                }
                EnumValue::String(s) => {
                    let _ = write!(self.output, "{}: {}", member.name, self.quote_string(s));
                }
            }
        }
        self.output.push_str("});\n");
    }

    // --- Global emission ---

    fn emit_global(&mut self, global: &Global) {
        self.write_indent();
        let name = self.get_global_name(global.id);
        if global.mutable {
            let _ = write!(self.output, "let {}", name);
        } else {
            let _ = write!(self.output, "const {}", name);
        }
        if let Some(init) = &global.init {
            self.output.push_str(" = ");
            self.emit_expr(init);
        } else if global.name == "__platform__" || name == "__platform__" {
            // Inject web platform ID for --target web
            // 0=macOS, 1=iOS, 2=Android, 3=Windows, 4=Linux, 5=Web
            self.output.push_str(" = 5");
        }
        self.output.push_str(";\n");
    }

    // --- Class emission ---

    fn emit_class(&mut self, class: &Class) {
        self.write_indent();
        let _ = write!(self.output, "class {}", class.name);
        if let Some(extends_name) = &class.extends_name {
            let _ = write!(self.output, " extends {}", extends_name);
        }
        self.output.push_str(" {\n");
        self.indent += 1;

        // Constructor
        if let Some(ctor) = &class.constructor {
            self.write_indent();
            self.output.push_str("constructor(");
            self.emit_params(&ctor.params);
            self.output.push_str(") {\n");
            self.indent += 1;

            // Emit field initializers that aren't in constructor body
            for field in &class.fields {
                if let Some(init) = &field.init {
                    // Only emit if constructor body doesn't set this field
                    self.write_indent();
                    let _ = write!(self.output, "this.{} = ", field.name);
                    self.emit_expr(init);
                    self.output.push_str(";\n");
                }
            }

            for stmt in &ctor.body {
                self.emit_stmt(stmt);
            }
            self.indent -= 1;
            self.writeln("}");
        } else if !class.fields.is_empty() {
            // Auto-generate constructor with field initializers
            self.write_indent();
            self.output.push_str("constructor() {\n");
            self.indent += 1;
            if class.extends.is_some() || class.extends_name.is_some() {
                self.writeln("super();");
            }
            for field in &class.fields {
                self.write_indent();
                let _ = write!(self.output, "this.{} = ", field.name);
                if let Some(init) = &field.init {
                    self.emit_expr(init);
                } else {
                    self.output.push_str("undefined");
                }
                self.output.push_str(";\n");
            }
            self.indent -= 1;
            self.writeln("}");
        }

        // Instance methods
        for method in &class.methods {
            self.emit_method(method);
        }

        // Getters
        for (prop_name, func) in &class.getters {
            self.write_indent();
            let _ = writeln!(self.output, "get {}() {{", prop_name);
            self.indent += 1;
            for stmt in &func.body {
                self.emit_stmt(stmt);
            }
            self.indent -= 1;
            self.writeln("}");
        }

        // Setters
        for (prop_name, func) in &class.setters {
            self.write_indent();
            let _ = write!(self.output, "set {}(", prop_name);
            self.emit_params(&func.params);
            self.output.push_str(") {\n");
            self.indent += 1;
            for stmt in &func.body {
                self.emit_stmt(stmt);
            }
            self.indent -= 1;
            self.writeln("}");
        }

        // Static methods
        for method in &class.static_methods {
            self.write_indent();
            let _ = write!(self.output, "static ");
            if method.is_async {
                self.output.push_str("async ");
            }
            let _ = write!(self.output, "{}(", method.name);
            self.emit_params(&method.params);
            self.output.push_str(") {\n");
            self.indent += 1;
            for stmt in &method.body {
                self.emit_stmt(stmt);
            }
            self.indent -= 1;
            self.writeln("}");
        }

        self.indent -= 1;
        self.writeln("}");

        // Static field initializers (outside class body)
        for field in &class.static_fields {
            if let Some(init) = &field.init {
                self.write_indent();
                let _ = write!(self.output, "{}.{} = ", class.name, field.name);
                self.emit_expr(init);
                self.output.push_str(";\n");
            }
        }
    }

    fn emit_method(&mut self, method: &Function) {
        self.write_indent();
        if method.is_async {
            self.output.push_str("async ");
        }
        if method.is_generator {
            let _ = write!(self.output, "*{}(", method.name);
        } else {
            let _ = write!(self.output, "{}(", method.name);
        }
        self.emit_params(&method.params);
        self.output.push_str(") {\n");
        self.indent += 1;
        for stmt in &method.body {
            self.emit_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    // --- Function emission ---

    fn emit_function(&mut self, func: &Function) {
        self.write_indent();
        if func.is_async {
            self.output.push_str("async ");
        }
        let name = self.get_func_name(func.id);
        if func.is_generator {
            let _ = write!(self.output, "function* {}(", name);
        } else {
            let _ = write!(self.output, "function {}(", name);
        }
        self.emit_params(&func.params);
        self.output.push_str(") {\n");
        self.indent += 1;
        for stmt in &func.body {
            self.emit_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_params(&mut self, params: &[Param]) {
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            if param.is_rest {
                self.output.push_str("...");
            }
            let name = self.make_local_name(&param.name, param.id);
            self.output.push_str(&name);
            if let Some(default) = &param.default {
                self.output.push_str(" = ");
                self.emit_expr(default);
            }
        }
    }

    // --- Statement emission ---

    pub fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                id,
                name,
                mutable,
                init,
                ..
            } => {
                self.write_indent();
                let var_name = self.make_local_name(name, *id);
                if *mutable {
                    let _ = write!(self.output, "let {}", var_name);
                } else {
                    let _ = write!(self.output, "const {}", var_name);
                }
                if let Some(init) = init {
                    self.output.push_str(" = ");
                    self.emit_expr(init);
                } else if name == "__platform__" {
                    // Inject web platform ID for --target web
                    // 0=macOS, 1=iOS, 2=Android, 3=Windows, 4=Linux, 5=Web
                    self.output.push_str(" = 5");
                }
                self.output.push_str(";\n");
            }
            Stmt::Expr(expr) => {
                self.write_indent();
                self.emit_expr(expr);
                self.output.push_str(";\n");
            }
            Stmt::Return(expr) => {
                self.write_indent();
                if let Some(expr) = expr {
                    self.output.push_str("return ");
                    self.emit_expr(expr);
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str("return;\n");
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.write_indent();
                self.output.push_str("if (");
                self.emit_expr(condition);
                self.output.push_str(") {\n");
                self.indent += 1;
                for s in then_branch {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                if let Some(else_stmts) = else_branch {
                    self.writeln("} else {");
                    self.indent += 1;
                    for s in else_stmts {
                        self.emit_stmt(s);
                    }
                    self.indent -= 1;
                }
                self.writeln("}");
            }
            Stmt::While { condition, body } => {
                self.write_indent();
                self.output.push_str("while (");
                self.emit_expr(condition);
                self.output.push_str(") {\n");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                self.writeln("}");
            }
            Stmt::DoWhile { body, condition } => {
                self.writeln("do {");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("} while (");
                self.emit_expr(condition);
                self.output.push_str(");\n");
            }
            Stmt::Labeled { label, body } => {
                self.write_indent();
                let _ = write!(self.output, "{}: ", label);
                // Emit the body statement without extra indentation prefix
                self.emit_stmt(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.write_indent();
                self.output.push_str("for (");
                if let Some(init_stmt) = init {
                    // For init is a statement, but we emit it inline without semicolon
                    match init_stmt.as_ref() {
                        Stmt::Let {
                            id,
                            name,
                            mutable,
                            init: let_init,
                            ..
                        } => {
                            let var_name = self.make_local_name(name, *id);
                            if *mutable {
                                let _ = write!(self.output, "let {}", var_name);
                            } else {
                                let _ = write!(self.output, "const {}", var_name);
                            }
                            if let Some(init_expr) = let_init {
                                self.output.push_str(" = ");
                                self.emit_expr(init_expr);
                            }
                        }
                        Stmt::Expr(expr) => {
                            self.emit_expr(expr);
                        }
                        _ => {}
                    }
                }
                self.output.push_str("; ");
                if let Some(cond) = condition {
                    self.emit_expr(cond);
                }
                self.output.push_str("; ");
                if let Some(upd) = update {
                    self.emit_expr(upd);
                }
                self.output.push_str(") {\n");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                self.writeln("}");
            }
            Stmt::Break => {
                self.writeln("break;");
            }
            Stmt::Continue => {
                self.writeln("continue;");
            }
            Stmt::LabeledBreak(label) => {
                self.write_indent();
                let _ = writeln!(self.output, "break {};", label);
            }
            Stmt::LabeledContinue(label) => {
                self.write_indent();
                let _ = writeln!(self.output, "continue {};", label);
            }
            Stmt::Throw(expr) => {
                self.write_indent();
                self.output.push_str("throw ");
                self.emit_expr(expr);
                self.output.push_str(";\n");
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.writeln("try {");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                if let Some(catch_clause) = catch {
                    self.write_indent();
                    if let Some((id, name)) = &catch_clause.param {
                        let var_name = self.make_local_name(name, *id);
                        let _ = writeln!(self.output, "}} catch ({}) {{", var_name);
                    } else {
                        self.output.push_str("} catch {\n");
                    }
                    self.indent += 1;
                    for s in &catch_clause.body {
                        self.emit_stmt(s);
                    }
                    self.indent -= 1;
                }
                if let Some(finally_stmts) = finally {
                    self.writeln("} finally {");
                    self.indent += 1;
                    for s in finally_stmts {
                        self.emit_stmt(s);
                    }
                    self.indent -= 1;
                }
                self.writeln("}");
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.write_indent();
                self.output.push_str("switch (");
                self.emit_expr(discriminant);
                self.output.push_str(") {\n");
                self.indent += 1;
                for case in cases {
                    if let Some(test) = &case.test {
                        self.write_indent();
                        self.output.push_str("case ");
                        self.emit_expr(test);
                        self.output.push_str(":\n");
                    } else {
                        self.writeln("default:");
                    }
                    self.indent += 1;
                    for s in &case.body {
                        self.emit_stmt(s);
                    }
                    self.indent -= 1;
                }
                self.indent -= 1;
                self.writeln("}");
            }
            // Issue #569: PreallocateBoxes is a perry-codegen-only directive
            // (alloca slot+box for hoisted FnDecl ids). The JS backend has no
            // equivalent — JS hoisting handles this for free in the V8 / JSC
            // runtime. Emit nothing.
            Stmt::PreallocateBoxes(_) => {}
        }
    }

    // --- Expression emission ---

    pub fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            // --- Literals ---
            Expr::Undefined => self.output.push_str("undefined"),
            Expr::Null => self.output.push_str("null"),
            Expr::Bool(b) => self.output.push_str(if *b { "true" } else { "false" }),
            Expr::Number(n) => {
                if n.is_nan() {
                    self.output.push_str("NaN");
                } else if n.is_infinite() {
                    if *n > 0.0 {
                        self.output.push_str("Infinity");
                    } else {
                        self.output.push_str("-Infinity");
                    }
                } else if *n == 0.0 && n.is_sign_negative() {
                    self.output.push_str("-0");
                } else {
                    // Use integer format when possible for cleaner output
                    let i = *n as i64;
                    if i as f64 == *n && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                        let _ = write!(self.output, "{}", i);
                    } else {
                        let _ = write!(self.output, "{}", n);
                    }
                }
            }
            Expr::Integer(i) => {
                let _ = write!(self.output, "{}", i);
            }
            Expr::BigInt(s) => {
                let _ = write!(self.output, "{}n", s);
            }
            Expr::String(s) => {
                self.output.push_str(&self.quote_string(s));
            }
            Expr::I18nString { key, .. } => {
                // JS codegen: emit as regular string (i18n handled by JS runtime)
                self.output.push_str(&self.quote_string(key));
            }

            // --- Variables ---
            Expr::LocalGet(id) => {
                let name = self.get_local_name(*id);
                self.output.push_str(&name);
            }
            Expr::LocalSet(id, val) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "({} = ", name);
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::GlobalGet(id) => {
                let name = self.get_global_name(*id);
                // GlobalGet(0) for unregistered globals is the implicit console object
                if name.starts_with("_g") && !self.global_names.contains_key(id) {
                    self.output.push_str("console");
                } else {
                    self.output.push_str(&name);
                }
            }
            Expr::GlobalSet(id, val) => {
                let name = self.get_global_name(*id);
                let _ = write!(self.output, "({} = ", name);
                self.emit_expr(val);
                self.output.push(')');
            }

            // --- Update ---
            Expr::Update { id, op, prefix } => {
                let name = self.get_local_name(*id);
                let op_str = match op {
                    UpdateOp::Increment => "++",
                    UpdateOp::Decrement => "--",
                };
                if *prefix {
                    let _ = write!(self.output, "{}{}", op_str, name);
                } else {
                    let _ = write!(self.output, "{}{}", name, op_str);
                }
            }

            // --- Binary operations ---
            Expr::Binary { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    BinaryOp::Add => " + ",
                    BinaryOp::Sub => " - ",
                    BinaryOp::Mul => " * ",
                    BinaryOp::Div => " / ",
                    BinaryOp::Mod => " % ",
                    BinaryOp::Pow => " ** ",
                    BinaryOp::BitAnd => " & ",
                    BinaryOp::BitOr => " | ",
                    BinaryOp::BitXor => " ^ ",
                    BinaryOp::Shl => " << ",
                    BinaryOp::Shr => " >> ",
                    BinaryOp::UShr => " >>> ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Unary operations ---
            Expr::Unary { op, operand } => {
                match op {
                    UnaryOp::Neg => { self.output.push_str("(-"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::Not => { self.output.push_str("(!"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::BitNot => { self.output.push_str("(~"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::Pos => { self.output.push_str("(+"); self.emit_expr(operand); self.output.push(')'); }
                }
            }

            // --- Comparison ---
            Expr::Compare { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    CompareOp::Eq => " === ",
                    CompareOp::Ne => " !== ",
                    CompareOp::LooseEq => " == ",
                    CompareOp::LooseNe => " != ",
                    CompareOp::Lt => " < ",
                    CompareOp::Le => " <= ",
                    CompareOp::Gt => " > ",
                    CompareOp::Ge => " >= ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Logical ---
            Expr::Logical { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    LogicalOp::And => " && ",
                    LogicalOp::Or => " || ",
                    LogicalOp::Coalesce => " ?? ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Function calls ---
            Expr::Call { callee, args, .. } => {
                self.emit_expr(callee);
                self.output.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            Expr::CallSpread { callee, args, .. } => {
                self.emit_expr(callee);
                self.output.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    match arg {
                        CallArg::Expr(e) => self.emit_expr(e),
                        CallArg::Spread(e) => {
                            self.output.push_str("...");
                            self.emit_expr(e);
                        }
                    }
                }
                self.output.push(')');
            }

            // --- Function reference ---
            Expr::FuncRef(id) => {
                let name = self.get_func_name(*id);
                self.output.push_str(&name);
            }

            Expr::ExternFuncRef { name, .. } => {
                self.output.push_str(name);
            }

            // --- Native module handling ---
            Expr::NativeModuleRef(_module) => {
                // Native module reference - in web, this is a no-op or maps to a shim
                self.output.push_str("undefined");
            }

            Expr::NativeMethodCall { module, class_name, object, method, args } => {
                self.emit_native_method_call(module, class_name.as_deref(), object.as_deref(), method, args);
            }

            // --- Property access ---
            Expr::PropertyGet { object, property } => {
                self.emit_expr(object);
                if is_valid_identifier(property) {
                    let _ = write!(self.output, ".{}", property);
                } else {
                    let _ = write!(self.output, "[{}]", self.quote_string(property));
                }
            }
            Expr::PropertySet { object, property, value } => {
                self.output.push('(');
                self.emit_expr(object);
                if is_valid_identifier(property) {
                    let _ = write!(self.output, ".{}", property);
                } else {
                    let _ = write!(self.output, "[{}]", self.quote_string(property));
                }
                self.output.push_str(" = ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::PropertyUpdate { object, property, op, prefix } => {
                let op_str = match op {
                    BinaryOp::Add => "++",
                    BinaryOp::Sub => "--",
                    _ => "++",
                };
                if *prefix {
                    let _ = write!(self.output, "{}", op_str);
                    self.emit_expr(object);
                    let _ = write!(self.output, ".{}", property);
                } else {
                    self.emit_expr(object);
                    let _ = write!(self.output, ".{}{}", property, op_str);
                }
            }

            // --- Index access ---
            Expr::IndexGet { object, index } => {
                self.emit_expr(object);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::IndexSet { object, index, value } => {
                self.output.push('(');
                self.emit_expr(object);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::IndexUpdate { object, index, op, prefix } => {
                let op_str = match op {
                    BinaryOp::Add => "++",
                    BinaryOp::Sub => "--",
                    _ => "++",
                };
                if *prefix {
                    self.output.push_str(op_str);
                    self.emit_expr(object);
                    self.output.push('[');
                    self.emit_expr(index);
                    self.output.push(']');
                } else {
                    self.emit_expr(object);
                    self.output.push('[');
                    self.emit_expr(index);
                    self.output.push(']');
                    self.output.push_str(op_str);
                }
            }

            // --- Object literal ---
            Expr::Object(fields) => {
                self.output.push('{');
                for (i, (key, val)) in fields.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    if is_valid_identifier(key) {
                        self.output.push_str(key);
                    } else {
                        self.output.push_str(&self.quote_string(key));
                    }
                    self.output.push_str(": ");
                    self.emit_expr(val);
                }
                self.output.push('}');
            }
            Expr::ObjectSpread { parts } => {
                self.output.push('{');
                let mut first = true;
                for (key_opt, val) in parts.iter() {
                    if !first { self.output.push_str(", "); }
                    first = false;
                    match key_opt {
                        None => {
                            self.output.push_str("...(");
                            self.emit_expr(val);
                            self.output.push(')');
                        }
                        Some(key) => {
                            if is_valid_identifier(key) {
                                self.output.push_str(key);
                            } else {
                                self.output.push_str(&self.quote_string(key));
                            }
                            self.output.push_str(": ");
                            self.emit_expr(val);
                        }
                    }
                }
                self.output.push('}');
            }

            // --- Array literal ---
            Expr::Array(elements) => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(el);
                }
                self.output.push(']');
            }

            Expr::ArraySpread(elements) => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    match el {
                        ArrayElement::Expr(e) => self.emit_expr(e),
                        ArrayElement::Spread(e) => {
                            self.output.push_str("...");
                            self.emit_expr(e);
                        }
                    }
                }
                self.output.push(']');
            }

            // --- Conditional (ternary) ---
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.output.push('(');
                self.emit_expr(condition);
                self.output.push_str(" ? ");
                self.emit_expr(then_expr);
                self.output.push_str(" : ");
                self.emit_expr(else_expr);
                self.output.push(')');
            }

            // --- Type operations ---
            Expr::TypeOf(operand) => {
                self.output.push_str("typeof ");
                self.emit_expr(operand);
            }
            Expr::Void(operand) => {
                self.output.push_str("void ");
                self.emit_expr(operand);
            }
            Expr::InstanceOf { expr, ty, .. } => {
                self.output.push('(');
                self.emit_expr(expr);
                let _ = write!(self.output, " instanceof {})", ty);
            }
            Expr::In { property, object } => {
                self.output.push('(');
                self.emit_expr(property);
                self.output.push_str(" in ");
                self.emit_expr(object);
                self.output.push(')');
            }

            // --- Await / Yield ---
            Expr::Await(expr) => {
                self.output.push_str("(await ");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::Yield { value, delegate } => {
                if *delegate {
                    self.output.push_str("yield* ");
                } else {
                    self.output.push_str("yield ");
                }
                if let Some(val) = value {
                    self.emit_expr(val);
                }
            }

            // --- New expression ---
            Expr::New { class_name, args, .. } => {
                let _ = write!(self.output, "new {}(", class_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::NewDynamic { callee, args } => {
                self.output.push_str("new (");
                self.emit_expr(callee);
                self.output.push_str(")(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- Class/Enum reference ---
            Expr::ClassRef(name) => {
                self.output.push_str(name);
            }
            Expr::EnumMember { enum_name, member_name } => {
                let _ = write!(self.output, "{}.{}", enum_name, member_name);
            }

            // --- Static field/method ---
            Expr::StaticFieldGet { class_name, field_name } => {
                let _ = write!(self.output, "{}.{}", class_name, field_name);
            }
            Expr::StaticFieldSet { class_name, field_name, value } => {
                let _ = write!(self.output, "({}.{} = ", class_name, field_name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                let _ = write!(self.output, "{}.{}(", class_name, method_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- This / Super ---
            Expr::This => self.output.push_str("this"),
            Expr::SuperCall(args) => {
                self.output.push_str("super(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::SuperMethodCall { method, args } => {
                let _ = write!(self.output, "super.{}(", method);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- Process/Environment ---
            Expr::EnvGet(name) => {
                // In browser, no real env vars
                let _ = write!(self.output, "(typeof process !== 'undefined' ? process.env.{} : undefined)", name);
            }
            Expr::EnvGetDynamic(expr) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.env[");
                self.emit_expr(expr);
                self.output.push_str("] : undefined)");
            }
            Expr::ProcessUptime => {
                self.output.push_str("(performance.now() / 1000)");
            }
            Expr::ProcessCwd => {
                self.output.push_str("(typeof process !== 'undefined' ? process.cwd() : '/')");
            }
            Expr::ProcessArgv => {
                self.output.push_str("(typeof process !== 'undefined' ? process.argv : [])");
            }
            Expr::ProcessMemoryUsage => {
                self.output.push_str("(typeof process !== 'undefined' ? process.memoryUsage() : {rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0})");
            }
            Expr::ProcessPid => {
                self.output.push_str("(typeof process !== 'undefined' ? process.pid : 0)");
            }
            Expr::ProcessPpid => {
                self.output.push_str("(typeof process !== 'undefined' ? process.ppid : 0)");
            }
            Expr::ProcessVersion => {
                self.output.push_str("(typeof process !== 'undefined' ? process.version : 'v22.0.0')");
            }
            Expr::ProcessVersions => {
                self.output.push_str("(typeof process !== 'undefined' ? process.versions : {node:'22.0.0', v8:'12.4.254.21'})");
            }
            Expr::ProcessHrtimeBigint => {
                self.output.push_str("(typeof process !== 'undefined' ? process.hrtime.bigint() : BigInt(Date.now()) * 1000000n)");
            }
            Expr::ProcessNextTick(cb) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.nextTick(");
                self.emit_expr(cb);
                self.output.push_str(") : queueMicrotask(");
                self.emit_expr(cb);
                self.output.push_str("))");
            }
            Expr::ProcessOn { event, handler } => {
                self.output.push_str("(typeof process !== 'undefined' ? process.on(");
                self.emit_expr(event);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessChdir(dir) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.chdir(");
                self.emit_expr(dir);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessKill { pid, signal } => {
                self.output.push_str("(typeof process !== 'undefined' ? process.kill(");
                self.emit_expr(pid);
                if let Some(s) = signal {
                    self.output.push_str(", ");
                    self.emit_expr(s);
                }
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessExit(code) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.exit(");
                if let Some(c) = code {
                    self.emit_expr(c);
                } else {
                    self.output.push('0');
                }
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessStdin => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stdin : { write: () => true })");
            }
            Expr::ProcessStdout => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stdout : { write: (s) => { console.log(s); return true; } })");
            }
            Expr::ProcessStderr => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stderr : { write: (s) => { console.error(s); return true; } })");
            }

            // --- File System (web-compatible stubs) ---
            Expr::FsReadFileSync(path) => {
                self.output.push_str("__perry.fs_readFileSync(");
                self.emit_expr(path);
                self.output.push(')');
            }
            Expr::FsWriteFileSync(_, _) |
            Expr::FsMkdirSync(_) |
            Expr::FsUnlinkSync(_) |
            Expr::FsAppendFileSync(_, _) |
            Expr::FsReadFileBinary(_) |
            Expr::FsRmRecursive(_) => {
                self.output.push_str("((() => { throw new Error('fs write operations not available in browser'); })())");
            }
            Expr::FsExistsSync(path) => {
                self.output.push_str("__perry.fs_existsSync(");
                self.emit_expr(path);
                self.output.push(')');
            }

            // --- Path operations ---
            Expr::PathJoin(a, b) => {
                self.output.push_str("__perry.path.join(");
                self.emit_expr(a);
                self.output.push_str(", ");
                self.emit_expr(b);
                self.output.push(')');
            }
            Expr::PathDirname(p) => {
                self.output.push_str("__perry.path.dirname(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathBasename(p) => {
                self.output.push_str("__perry.path.basename(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathExtname(p) => {
                self.output.push_str("__perry.path.extname(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathResolve(p) => {
                self.output.push_str("__perry.path.resolve(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathIsAbsolute(p) => {
                self.output.push_str("__perry.path.isAbsolute(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathRelative(from, to) => {
                self.output.push_str("__perry.path.relative(");
                self.emit_expr(from);
                self.output.push_str(", ");
                self.emit_expr(to);
                self.output.push(')');
            }
            Expr::PathNormalize(p) => {
                self.output.push_str("__perry.path.normalize(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathParse(p) => {
                self.output.push_str("__perry.path.parse(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathFormat(p) => {
                self.output.push_str("__perry.path.format(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathBasenameExt(p, ext) => {
                self.output.push_str("__perry.path.basename(");
                self.emit_expr(p);
                self.output.push_str(", ");
                self.emit_expr(ext);
                self.output.push(')');
            }
            Expr::PathSep => {
                self.output.push_str("__perry.path.sep");
            }
            Expr::PathDelimiter => {
                self.output.push_str("__perry.path.delimiter");
            }
            Expr::PathToNamespacedPath(p) => {
                self.output.push_str("__perry.path.toNamespacedPath(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathMatchesGlob(p, pat) => {
                self.output.push_str("__perry.path.matchesGlob(");
                self.emit_expr(p);
                self.output.push_str(", ");
                self.emit_expr(pat);
                self.output.push(')');
            }
            Expr::PathResolveJoin(a, b) => {
                // Match Node's path.resolve(a, b) two-arg behavior.
                self.output.push_str("__perry.path.resolve(");
                self.emit_expr(a);
                self.output.push_str(", ");
                self.emit_expr(b);
                self.output.push(')');
            }

            // --- WeakRef and FinalizationRegistry ---
            Expr::WeakRefNew(target) => {
                self.output.push_str("new WeakRef(");
                self.emit_expr(target);
                self.output.push(')');
            }
            Expr::WeakRefDeref(weakref_expr) => {
                self.output.push('(');
                self.emit_expr(weakref_expr);
                self.output.push_str(").deref()");
            }
            Expr::FinalizationRegistryNew(callback) => {
                self.output.push_str("new FinalizationRegistry(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::FinalizationRegistryRegister { registry, target, held, token } => {
                self.output.push('(');
                self.emit_expr(registry);
                self.output.push_str(").register(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(held);
                if let Some(t) = token {
                    self.output.push_str(", ");
                    self.emit_expr(t);
                }
                self.output.push(')');
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.output.push('(');
                self.emit_expr(registry);
                self.output.push_str(").unregister(");
                self.emit_expr(token);
                self.output.push(')');
            }

            // --- URL ---
            Expr::FileURLToPath(u) => {
                self.output.push_str("(new URL(");
                self.emit_expr(u);
                self.output.push_str(")).pathname");
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.output.push_str("JSON.parse(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::JsonStringify(val) => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::JsonStringifyPretty { value, replacer, space } => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(value);
                self.output.push_str(", ");
                if let Some(r) = replacer { self.emit_expr(r); } else { self.output.push_str("null"); }
                self.output.push_str(", ");
                self.emit_expr(space);
                self.output.push(')');
            }
            Expr::JsonParseReviver { text, reviver } | Expr::JsonParseWithReviver(text, reviver) => {
                self.output.push_str("JSON.parse(");
                self.emit_expr(text);
                self.output.push_str(", ");
                self.emit_expr(reviver);
                self.output.push(')');
            }
            Expr::JsonStringifyFull(value, replacer, spacer) => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(value);
                self.output.push_str(", ");
                self.emit_expr(replacer);
                self.output.push_str(", ");
                self.emit_expr(spacer);
                self.output.push(')');
            }

            // --- Math ---
            Expr::MathFloor(x) => { self.emit_math_unary("Math.floor", x); }
            Expr::MathCeil(x) => { self.emit_math_unary("Math.ceil", x); }
            Expr::MathRound(x) => { self.emit_math_unary("Math.round", x); }
            Expr::MathAbs(x) => { self.emit_math_unary("Math.abs", x); }
            Expr::MathSqrt(x) => { self.emit_math_unary("Math.sqrt", x); }
            Expr::MathLog(x) => { self.emit_math_unary("Math.log", x); }
            Expr::MathLog2(x) => { self.emit_math_unary("Math.log2", x); }
            Expr::MathLog10(x) => { self.emit_math_unary("Math.log10", x); }
            Expr::MathSin(x) => { self.emit_math_unary("Math.sin", x); }
            Expr::MathCos(x) => { self.emit_math_unary("Math.cos", x); }
            Expr::MathTan(x) => { self.emit_math_unary("Math.tan", x); }
            Expr::MathAsin(x) => { self.emit_math_unary("Math.asin", x); }
            Expr::MathAcos(x) => { self.emit_math_unary("Math.acos", x); }
            Expr::MathAtan(x) => { self.emit_math_unary("Math.atan", x); }
            Expr::MathAtan2(y, x) => {
                self.output.push_str("Math.atan2(");
                self.emit_expr(y);
                self.output.push_str(", ");
                self.emit_expr(x);
                self.output.push(')');
            }
            Expr::MathCbrt(x) => { self.emit_math_unary("Math.cbrt", x); }
            Expr::MathFround(x) => { self.emit_math_unary("Math.fround", x); }
            Expr::MathClz32(x) => { self.emit_math_unary("Math.clz32", x); }
            Expr::MathExpm1(x) => { self.emit_math_unary("Math.expm1", x); }
            Expr::MathLog1p(x) => { self.emit_math_unary("Math.log1p", x); }
            Expr::MathSinh(x) => { self.emit_math_unary("Math.sinh", x); }
            Expr::MathCosh(x) => { self.emit_math_unary("Math.cosh", x); }
            Expr::MathTanh(x) => { self.emit_math_unary("Math.tanh", x); }
            Expr::MathAsinh(x) => { self.emit_math_unary("Math.asinh", x); }
            Expr::MathAcosh(x) => { self.emit_math_unary("Math.acosh", x); }
            Expr::MathAtanh(x) => { self.emit_math_unary("Math.atanh", x); }
            Expr::MathExp(x) => { self.emit_math_unary("Math.exp", x); }
            Expr::MathHypot(args) => { self.emit_math_variadic("Math.hypot", args); }
            Expr::MathPow(base, exp) => {
                self.output.push_str("Math.pow(");
                self.emit_expr(base);
                self.output.push_str(", ");
                self.emit_expr(exp);
                self.output.push(')');
            }
            Expr::MathMin(args) => { self.emit_math_variadic("Math.min", args); }
            Expr::MathMax(args) => { self.emit_math_variadic("Math.max", args); }
            Expr::MathMinSpread(arr) => {
                self.output.push_str("Math.min(...");
                self.emit_expr(arr);
                self.output.push(')');
            }
            Expr::MathMaxSpread(arr) => {
                self.output.push_str("Math.max(...");
                self.emit_expr(arr);
                self.output.push(')');
            }
            Expr::MathRandom => self.output.push_str("Math.random()"),

            // --- Crypto ---
            Expr::CryptoRandomBytes(size) => {
                self.output.push_str("Array.from(crypto.getRandomValues(new Uint8Array(");
                self.emit_expr(size);
                self.output.push_str("))).map(b => b.toString(16).padStart(2, '0')).join('')");
            }
            Expr::CryptoRandomUUID => {
                self.output.push_str("crypto.randomUUID()");
            }
            Expr::CryptoSha256(data) => {
                // Use SubtleCrypto (async in browser, but we emit it inline)
                self.output.push_str("(await (async () => { const d = new TextEncoder().encode(");
                self.emit_expr(data);
                self.output.push_str("); const h = await crypto.subtle.digest('SHA-256', d); return Array.from(new Uint8Array(h)).map(b => b.toString(16).padStart(2, '0')).join(''); })())");
            }
            Expr::CryptoMd5(_) => {
                self.output.push_str("((() => { throw new Error('MD5 not available in browser crypto API'); })())");
            }

            // --- OS (browser alternatives) ---
            Expr::OsPlatform => self.output.push_str("'browser'"),
            Expr::OsArch => self.output.push_str("'wasm'"),
            Expr::OsHostname => self.output.push_str("location.hostname"),
            Expr::OsHomedir => self.output.push_str("'/'"),
            Expr::OsTmpdir => self.output.push_str("'/tmp'"),
            Expr::OsTotalmem => self.output.push_str("(navigator.deviceMemory ? navigator.deviceMemory * 1024 * 1024 * 1024 : 4294967296)"),
            Expr::OsFreemem => self.output.push_str("(navigator.deviceMemory ? navigator.deviceMemory * 1024 * 1024 * 1024 : 4294967296)"),
            Expr::OsUptime => self.output.push_str("(performance.now() / 1000)"),
            Expr::OsType => self.output.push_str("'Browser'"),
            Expr::OsRelease => self.output.push_str("navigator.userAgent"),
            Expr::OsCpus => self.output.push_str("(Array.from({length: navigator.hardwareConcurrency || 4}, () => ({model: 'unknown', speed: 0})))"),
            Expr::OsNetworkInterfaces => self.output.push_str("({})"),
            Expr::OsUserInfo => self.output.push_str("({username: 'browser', homedir: '/', shell: ''})"),
            Expr::OsEOL => self.output.push_str("'\\n'"),

            // --- Buffer (basic browser polyfill using Uint8Array) ---
            Expr::BufferFrom { data, encoding } => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(data);
                self.output.push(')');
                let _ = encoding; // encoding not used in simple polyfill
            }
            Expr::BufferAlloc { size, fill } => {
                self.output.push_str("new Uint8Array(");
                self.emit_expr(size);
                self.output.push(')');
                if let Some(f) = fill {
                    self.output.push_str(".fill(");
                    self.emit_expr(f);
                    self.output.push(')');
                }
            }
            Expr::BufferAllocUnsafe(size) => {
                self.output.push_str("new Uint8Array(");
                self.emit_expr(size);
                self.output.push(')');
            }
            Expr::BufferConcat(list) => {
                // Simple concat implementation
                self.output.push_str("((() => { const _arrs = ");
                self.emit_expr(list);
                self.output.push_str("; const _len = _arrs.reduce((a,b) => a + b.length, 0); const _r = new Uint8Array(_len); let _off = 0; for (const _a of _arrs) { _r.set(_a, _off); _off += _a.length; } return _r; })())");
            }
            Expr::BufferIsBuffer(obj) => {
                self.output.push('(');
                self.emit_expr(obj);
                self.output.push_str(" instanceof Uint8Array)");
            }
            Expr::BufferByteLength(s) => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(s);
                self.output.push_str(").length");
            }
            Expr::BufferToString { buffer, .. } => {
                self.output.push_str("new TextDecoder().decode(");
                self.emit_expr(buffer);
                self.output.push(')');
            }
            Expr::BufferLength(buf) => {
                self.emit_expr(buf);
                self.output.push_str(".length");
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.emit_expr(buffer);
                self.output.push_str(".slice(");
                if let Some(s) = start { self.emit_expr(s); } else { self.output.push('0'); }
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::BufferFill { buffer, value, .. } => {
                self.emit_expr(buffer);
                self.output.push_str(".fill(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
                self.emit_expr(target);
                self.output.push_str(".set(");
                self.emit_expr(source);
                if let Some(ss) = source_start {
                    self.output.push_str(".slice(");
                    self.emit_expr(ss);
                    if let Some(se) = source_end {
                        self.output.push_str(", ");
                        self.emit_expr(se);
                    }
                    self.output.push(')');
                }
                if let Some(ts) = target_start {
                    self.output.push_str(", ");
                    self.emit_expr(ts);
                }
                self.output.push(')');
            }
            Expr::BufferWrite { buffer, string, offset, .. } => {
                self.output.push_str("((() => { const _b = new TextEncoder().encode(");
                self.emit_expr(string);
                self.output.push_str("); ");
                self.emit_expr(buffer);
                self.output.push_str(".set(_b, ");
                if let Some(o) = offset { self.emit_expr(o); } else { self.output.push('0'); }
                self.output.push_str("); return _b.length; })())");
            }
            Expr::BufferEquals { buffer, other } => {
                self.output.push_str("((() => { const _a = ");
                self.emit_expr(buffer);
                self.output.push_str(", _b = ");
                self.emit_expr(other);
                self.output.push_str("; return _a.length === _b.length && _a.every((v, i) => v === _b[i]); })())");
            }
            Expr::BufferIndexGet { buffer, index } => {
                self.emit_expr(buffer);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::BufferIndexSet { buffer, index, value } => {
                self.output.push('(');
                self.emit_expr(buffer);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }

            // --- Typed arrays ---
            Expr::Uint8ArrayNew(size) => {
                self.output.push_str("new Uint8Array(");
                if let Some(s) = size { self.emit_expr(s); }
                self.output.push(')');
            }
            // NOTE: TypedArrayNew variant referenced an HIR variant that was
            // never landed; the corresponding lower.rs path was reverted.
            // Reinstating it requires landing the typedarray HIR work.
            Expr::Uint8ArrayFrom(src) => {
                self.output.push_str("Uint8Array.from(");
                self.emit_expr(src);
                self.output.push(')');
            }
            Expr::Uint8ArrayLength(arr) => {
                self.emit_expr(arr);
                self.output.push_str(".length");
            }
            Expr::Uint8ArrayGet { array, index } => {
                self.emit_expr(array);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::Uint8ArraySet { array, index, value } => {
                self.output.push('(');
                self.emit_expr(array);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }

            // --- Child process (throw stubs) ---
            Expr::ChildProcessExecSync { .. } |
            Expr::ChildProcessSpawnSync { .. } |
            Expr::ChildProcessSpawn { .. } |
            Expr::ChildProcessExec { .. } |
            Expr::ChildProcessSpawnBackground { .. } |
            Expr::ChildProcessGetProcessStatus(_) |
            Expr::ChildProcessKillProcess(_) => {
                self.output.push_str("((() => { throw new Error('child_process not available in browser'); })())");
            }

            // --- Fetch ---
            Expr::FetchWithOptions { url, method, body, headers } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output.push_str(", {method: ");
                self.emit_expr(method);
                self.output.push_str(", body: ");
                self.emit_expr(body);
                self.output.push_str(", headers: {");
                for (i, (key, val)) in headers.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.output.push_str(&self.quote_string(key));
                    self.output.push_str(": ");
                    self.emit_expr(val);
                }
                self.output.push_str("}})");
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output.push_str(", {headers: {\"Authorization\": ");
                self.emit_expr(auth_header);
                self.output.push_str("}})");
            }
            Expr::FetchPostWithAuth { url, auth_header, body } => {
                self.output.push_str("fetch(");
                self.emit_expr(url);
                self.output.push_str(", {method: \"POST\", headers: {\"Authorization\": ");
                self.emit_expr(auth_header);
                self.output.push_str(", \"Content-Type\": \"application/json\"}, body: ");
                self.emit_expr(body);
                self.output.push_str("})");
            }

            // --- Net (throw stubs) ---
            Expr::NetCreateServer { .. } |
            Expr::NetCreateConnection { .. } |
            Expr::NetConnect { .. } => {
                self.output.push_str("((() => { throw new Error('net module not available in browser'); })())");
            }

            // --- Array methods ---
            Expr::ArrayPush { array_id, value } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.push(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayPushSpread { array_id, source } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.push(...", name);
                self.emit_expr(source);
                self.output.push(')');
            }
            Expr::ArrayPop(id) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "{}.pop()", name);
            }
            Expr::ArrayShift(id) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "{}.shift()", name);
            }
            Expr::ArrayUnshift { array_id, value } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.unshift(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayIndexOf { array, value } => {
                self.emit_expr(array);
                self.output.push_str(".indexOf(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayIncludes { array, value } => {
                self.emit_expr(array);
                self.output.push_str(".includes(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArraySlice { array, start, end } => {
                self.emit_expr(array);
                self.output.push_str(".slice(");
                self.emit_expr(start);
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::ArraySplice { array_id, start, delete_count, items } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.splice(", name);
                self.emit_expr(start);
                if let Some(dc) = delete_count {
                    self.output.push_str(", ");
                    self.emit_expr(dc);
                }
                for item in items {
                    self.output.push_str(", ");
                    self.emit_expr(item);
                }
                self.output.push(')');
            }

            // --- Array higher-order methods ---
            Expr::ArrayForEach { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".forEach(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayMap { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".map(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFilter { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".filter(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFind { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".find(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findIndex(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindLast { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findLast(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFindLastIndex { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".findLastIndex(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayAt { array, index } => {
                self.emit_expr(array);
                self.output.push_str(".at(");
                self.emit_expr(index);
                self.output.push(')');
            }
            Expr::ArraySome { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".some(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayEvery { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".every(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArrayFlatMap { array, callback } => {
                self.emit_expr(array);
                self.output.push_str(".flatMap(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::ArraySort { array, comparator } => {
                self.emit_expr(array);
                self.output.push_str(".sort(");
                self.emit_expr(comparator);
                self.output.push(')');
            }
            Expr::ArrayReduce { array, callback, initial } => {
                self.emit_expr(array);
                self.output.push_str(".reduce(");
                self.emit_expr(callback);
                if let Some(init) = initial {
                    self.output.push_str(", ");
                    self.emit_expr(init);
                }
                self.output.push(')');
            }
            Expr::ArrayJoin { array, separator } => {
                self.emit_expr(array);
                self.output.push_str(".join(");
                if let Some(sep) = separator {
                    self.emit_expr(sep);
                }
                self.output.push(')');
            }
            Expr::ArrayFlat { array } => {
                self.emit_expr(array);
                self.output.push_str(".flat()");
            }
            Expr::ArrayReduceRight { array, callback, initial } => {
                self.emit_expr(array);
                self.output.push_str(".reduceRight(");
                self.emit_expr(callback);
                if let Some(init) = initial {
                    self.output.push_str(", ");
                    self.emit_expr(init);
                }
                self.output.push(')');
            }
            Expr::ArrayToReversed { array } => {
                self.emit_expr(array);
                self.output.push_str(".toReversed()");
            }
            Expr::ArrayToSorted { array, comparator } => {
                self.emit_expr(array);
                self.output.push_str(".toSorted(");
                if let Some(cmp) = comparator {
                    self.emit_expr(cmp);
                }
                self.output.push(')');
            }
            Expr::ArrayToSpliced { array, start, delete_count, items } => {
                self.emit_expr(array);
                self.output.push_str(".toSpliced(");
                self.emit_expr(start);
                self.output.push_str(", ");
                self.emit_expr(delete_count);
                for item in items {
                    self.output.push_str(", ");
                    self.emit_expr(item);
                }
                self.output.push(')');
            }
            Expr::ArrayWith { array, index, value } => {
                self.emit_expr(array);
                self.output.push_str(".with(");
                self.emit_expr(index);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ArrayCopyWithin { array_id, target, start, end } => {
                let name = self.get_local_name(*array_id);
                let _ = write!(self.output, "{}.copyWithin(", name);
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(start);
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::ArrayEntries(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".entries())");
            }
            Expr::ArrayKeys(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".keys())");
            }
            Expr::ArrayValues(array) => {
                self.output.push_str("Array.from(");
                self.emit_expr(array);
                self.output.push_str(".values())");
            }

            // --- String methods ---
            Expr::StringSplit(string, delimiter) => {
                self.emit_expr(string);
                self.output.push_str(".split(");
                self.emit_expr(delimiter);
                self.output.push(')');
            }
            Expr::StringFromCharCode(code) => {
                self.output.push_str("String.fromCharCode(");
                self.emit_expr(code);
                self.output.push(')');
            }
            Expr::StringFromCodePoint(code) => {
                self.output.push_str("String.fromCodePoint(");
                self.emit_expr(code);
                self.output.push(')');
            }
            Expr::StringAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".at(");
                self.emit_expr(index);
                self.output.push(')');
            }
            Expr::StringCodePointAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".codePointAt(");
                self.emit_expr(index);
                self.output.push(')');
            }

            // --- Map operations ---
            Expr::MapNew => self.output.push_str("new Map()"),
            Expr::MapNewFromArray(expr) => {
                self.output.push_str("new Map(");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::MapSet { map, key, value } => {
                self.emit_expr(map);
                self.output.push_str(".set(");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::MapGet { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".get(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapHas { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".has(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapDelete { map, key } => {
                self.emit_expr(map);
                self.output.push_str(".delete(");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::MapSize(map) => {
                self.emit_expr(map);
                self.output.push_str(".size");
            }
            Expr::MapClear(map) => {
                self.emit_expr(map);
                self.output.push_str(".clear()");
            }
            Expr::MapEntries(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".entries())");
            }
            Expr::MapKeys(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".keys())");
            }
            Expr::MapValues(map) => {
                self.output.push_str("Array.from(");
                self.emit_expr(map);
                self.output.push_str(".values())");
            }

            // --- Set operations ---
            Expr::SetNew => self.output.push_str("new Set()"),
            Expr::SetNewFromArray(expr) => {
                self.output.push_str("new Set(");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::SetAdd { set_id, value } => {
                let name = self.get_local_name(*set_id);
                let _ = write!(self.output, "{}.add(", name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetHas { set, value } => {
                self.emit_expr(set);
                self.output.push_str(".has(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetDelete { set, value } => {
                self.emit_expr(set);
                self.output.push_str(".delete(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::SetSize(set) => {
                self.emit_expr(set);
                self.output.push_str(".size");
            }
            Expr::SetClear(set) => {
                self.emit_expr(set);
                self.output.push_str(".clear()");
            }
            Expr::SetValues(set) => {
                self.output.push_str("Array.from(");
                self.emit_expr(set);
                self.output.push_str(".values())");
            }

            // --- Sequence ---
            Expr::Sequence(exprs) => {
                self.output.push('(');
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(e);
                }
                self.output.push(')');
            }

            // --- Date ---
            Expr::DateNow => self.output.push_str("Date.now()"),
            Expr::DateNew(val) => {
                if let Some(v) = val {
                    self.output.push_str("new Date(");
                    self.emit_expr(v);
                    self.output.push(')');
                } else {
                    self.output.push_str("new Date()");
                }
            }
            Expr::DateGetTime(d) => { self.emit_expr(d); self.output.push_str(".getTime()"); }
            Expr::DateToISOString(d) => { self.emit_expr(d); self.output.push_str(".toISOString()"); }
            Expr::DateGetFullYear(d) => { self.emit_expr(d); self.output.push_str(".getFullYear()"); }
            Expr::DateGetMonth(d) => { self.emit_expr(d); self.output.push_str(".getMonth()"); }
            Expr::DateGetDate(d) => { self.emit_expr(d); self.output.push_str(".getDate()"); }
            Expr::DateGetHours(d) => { self.emit_expr(d); self.output.push_str(".getHours()"); }
            Expr::DateGetMinutes(d) => { self.emit_expr(d); self.output.push_str(".getMinutes()"); }
            Expr::DateGetSeconds(d) => { self.emit_expr(d); self.output.push_str(".getSeconds()"); }
            Expr::DateGetMilliseconds(d) => { self.emit_expr(d); self.output.push_str(".getMilliseconds()"); }
            Expr::DateParse(s) => { self.output.push_str("Date.parse("); self.emit_expr(s); self.output.push(')'); }
            Expr::DateUtc(args) => {
                self.output.push_str("Date.UTC(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::DateGetUtcDay(d) => { self.emit_expr(d); self.output.push_str(".getUTCDay()"); }
            Expr::DateGetUtcFullYear(d) => { self.emit_expr(d); self.output.push_str(".getUTCFullYear()"); }
            Expr::DateGetUtcMonth(d) => { self.emit_expr(d); self.output.push_str(".getUTCMonth()"); }
            Expr::DateGetUtcDate(d) => { self.emit_expr(d); self.output.push_str(".getUTCDate()"); }
            Expr::DateGetUtcHours(d) => { self.emit_expr(d); self.output.push_str(".getUTCHours()"); }
            Expr::DateGetUtcMinutes(d) => { self.emit_expr(d); self.output.push_str(".getUTCMinutes()"); }
            Expr::DateGetUtcSeconds(d) => { self.emit_expr(d); self.output.push_str(".getUTCSeconds()"); }
            Expr::DateGetUtcMilliseconds(d) => { self.emit_expr(d); self.output.push_str(".getUTCMilliseconds()"); }
            Expr::DateSetUtcFullYear { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCFullYear("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcMonth { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCMonth("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcDate { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCDate("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcHours { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCHours("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcMinutes { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCMinutes("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcSeconds { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCSeconds("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateSetUtcMilliseconds { date, value } => { self.emit_expr(date); self.output.push_str(".setUTCMilliseconds("); self.emit_expr(value); self.output.push(')'); }
            Expr::DateValueOf(d) => { self.emit_expr(d); self.output.push_str(".valueOf()"); }
            Expr::DateToDateString(d) => { self.emit_expr(d); self.output.push_str(".toDateString()"); }
            Expr::DateToTimeString(d) => { self.emit_expr(d); self.output.push_str(".toTimeString()"); }
            Expr::DateToLocaleDateString(d) => { self.emit_expr(d); self.output.push_str(".toLocaleDateString()"); }
            Expr::DateToLocaleTimeString(d) => { self.emit_expr(d); self.output.push_str(".toLocaleTimeString()"); }
            Expr::DateToLocaleString(d) => { self.emit_expr(d); self.output.push_str(".toLocaleString()"); }
            Expr::DateGetTimezoneOffset(d) => { self.emit_expr(d); self.output.push_str(".getTimezoneOffset()"); }
            Expr::DateToJSON(d) => { self.emit_expr(d); self.output.push_str(".toJSON()"); }

            // --- Error ---
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.output.push_str("new Error(");
                    self.emit_expr(m);
                    self.output.push(')');
                } else {
                    self.output.push_str("new Error()");
                }
            }
            Expr::ErrorMessage(err) => {
                self.emit_expr(err);
                self.output.push_str(".message");
            }
            Expr::ErrorNewWithCause { message, cause } => {
                self.output.push_str("new Error(");
                self.emit_expr(message);
                self.output.push_str(", { cause: ");
                self.emit_expr(cause);
                self.output.push_str(" })");
            }
            Expr::TypeErrorNew(msg) => {
                self.output.push_str("new TypeError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::RangeErrorNew(msg) => {
                self.output.push_str("new RangeError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::ReferenceErrorNew(msg) => {
                self.output.push_str("new ReferenceError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::SyntaxErrorNew(msg) => {
                self.output.push_str("new SyntaxError(");
                self.emit_expr(msg);
                self.output.push(')');
            }
            Expr::AggregateErrorNew { errors, message } => {
                self.output.push_str("new AggregateError(");
                self.emit_expr(errors);
                self.output.push_str(", ");
                self.emit_expr(message);
                self.output.push(')');
            }

            // --- URL ---
            Expr::UrlNew { url, base } => {
                self.output.push_str("new URL(");
                self.emit_expr(url);
                if let Some(b) = base {
                    self.output.push_str(", ");
                    self.emit_expr(b);
                }
                self.output.push(')');
            }
            Expr::UrlGetHref(u) => { self.emit_expr(u); self.output.push_str(".href"); }
            Expr::UrlGetPathname(u) => { self.emit_expr(u); self.output.push_str(".pathname"); }
            Expr::UrlGetProtocol(u) => { self.emit_expr(u); self.output.push_str(".protocol"); }
            Expr::UrlGetHost(u) => { self.emit_expr(u); self.output.push_str(".host"); }
            Expr::UrlGetHostname(u) => { self.emit_expr(u); self.output.push_str(".hostname"); }
            Expr::UrlGetPort(u) => { self.emit_expr(u); self.output.push_str(".port"); }
            Expr::UrlGetSearch(u) => { self.emit_expr(u); self.output.push_str(".search"); }
            Expr::UrlGetHash(u) => { self.emit_expr(u); self.output.push_str(".hash"); }
            Expr::UrlGetOrigin(u) => { self.emit_expr(u); self.output.push_str(".origin"); }
            Expr::UrlGetSearchParams(u) => { self.emit_expr(u); self.output.push_str(".searchParams"); }

            // --- URLSearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(i) = init {
                    self.output.push_str("new URLSearchParams(");
                    self.emit_expr(i);
                    self.output.push(')');
                } else {
                    self.output.push_str("new URLSearchParams()");
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".get(");
                self.emit_expr(name);
                self.output.push(')');
            }
            Expr::UrlSearchParamsHas { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".has(");
                self.emit_expr(name);
                self.output.push(')');
            }
            Expr::UrlSearchParamsSet { params, name, value } => {
                self.emit_expr(params);
                self.output.push_str(".set(");
                self.emit_expr(name);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::UrlSearchParamsAppend { params, name, value } => {
                self.emit_expr(params);
                self.output.push_str(".append(");
                self.emit_expr(name);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::UrlSearchParamsDelete { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".delete(");
                self.emit_expr(name);
                self.output.push(')');
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_expr(params);
                self.output.push_str(".toString()");
            }
            Expr::UrlSearchParamsEntries(params) => {
                self.output.push_str("Array.from(");
                self.emit_expr(params);
                self.output.push_str(".entries())");
            }
            Expr::UrlSearchParamsGetAll { params, name } => {
                self.emit_expr(params);
                self.output.push_str(".getAll(");
                self.emit_expr(name);
                self.output.push(')');
            }

            // --- Delete ---
            Expr::Delete(expr) => {
                self.output.push_str("delete ");
                self.emit_expr(expr);
            }

            // --- Closure ---
            Expr::Closure { params, body, is_async, .. } => {
                if *is_async {
                    self.output.push_str("async ");
                }
                self.output.push('(');
                self.emit_params(params);
                self.output.push_str(") => {\n");
                self.indent += 1;
                for s in body {
                    self.emit_stmt(s);
                }
                self.indent -= 1;
                self.write_indent();
                self.output.push('}');
            }

            // --- RegExp ---
            Expr::RegExp { pattern, flags } => {
                let _ = write!(self.output, "/{}/{}", pattern, flags);
            }
            Expr::RegExpTest { regex, string } => {
                self.emit_expr(regex);
                self.output.push_str(".test(");
                self.emit_expr(string);
                self.output.push(')');
            }
            Expr::StringMatch { string, regex } => {
                self.emit_expr(string);
                self.output.push_str(".match(");
                self.emit_expr(regex);
                self.output.push(')');
            }
            Expr::StringMatchAll { string, regex } => {
                self.emit_expr(string);
                self.output.push_str(".matchAll(");
                self.emit_expr(regex);
                self.output.push(')');
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.emit_expr(string);
                self.output.push_str(".replace(");
                self.emit_expr(pattern);
                self.output.push_str(", ");
                self.emit_expr(replacement);
                self.output.push(')');
            }

            // --- Object operations ---
            Expr::ObjectKeys(obj) => {
                self.output.push_str("Object.keys(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectValues(obj) => {
                self.output.push_str("Object.values(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectEntries(obj) => {
                self.output.push_str("Object.entries(");
                self.emit_expr(obj);
                self.output.push(')');
            }
            Expr::ObjectFromEntries(entries) => {
                self.output.push_str("Object.fromEntries(");
                self.emit_expr(entries);
                self.output.push(')');
            }
            Expr::ObjectIs(a, b) => {
                self.output.push_str("Object.is(");
                self.emit_expr(a);
                self.output.push(',');
                self.emit_expr(b);
                self.output.push(')');
            }
            Expr::ObjectHasOwn(obj, key) => {
                self.output.push_str("Object.hasOwn(");
                self.emit_expr(obj);
                self.output.push(',');
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ObjectRest { object, exclude_keys } => {
                self.output.push_str("(({");
                for (i, key) in exclude_keys.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.output.push_str(key);
                }
                self.output.push_str("}, ..._rest) => _rest[0])(");  // Actually use Object.keys approach
                // Better approach: use destructuring
                self.output.clear(); // Redo this
                self.output.push_str("((() => { const {");
                for (i, key) in exclude_keys.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.output.push_str(key);
                    self.output.push_str(": _");
                }
                self.output.push_str(", ...__rest} = ");
                self.emit_expr(object);
                self.output.push_str("; return __rest; })())");
            }

            // --- Array static methods ---
            Expr::ArrayIsArray(val) => {
                self.output.push_str("Array.isArray(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::ArrayFrom(val) => {
                self.output.push_str("Array.from(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IteratorToArray(val) => {
                self.output.push_str("Array.from(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::ArrayFromMapped { iterable, map_fn } => {
                self.output.push_str("Array.from(");
                self.emit_expr(iterable);
                self.output.push_str(", ");
                self.emit_expr(map_fn);
                self.output.push(')');
            }

            // --- Global built-in functions ---
            Expr::ParseInt { string, radix } => {
                self.output.push_str("parseInt(");
                self.emit_expr(string);
                if let Some(r) = radix {
                    self.output.push_str(", ");
                    self.emit_expr(r);
                }
                self.output.push(')');
            }
            Expr::ParseFloat(s) => {
                self.output.push_str("parseFloat(");
                self.emit_expr(s);
                self.output.push(')');
            }
            Expr::NumberCoerce(val) => {
                self.output.push_str("Number(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::BigIntCoerce(val) => {
                self.output.push_str("BigInt(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::StringCoerce(val) => {
                self.output.push_str("String(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::BooleanCoerce(val) => {
                self.output.push_str("Boolean(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IsNaN(val) => {
                self.output.push_str("isNaN(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::IsUndefinedOrBareNan(val) => {
                // JS fallback: (v === undefined || Number.isNaN(v))
                self.output.push_str("((");
                self.emit_expr(val);
                self.output.push_str(") === undefined || Number.isNaN(");
                self.emit_expr(val);
                self.output.push_str("))");
            }
            Expr::IsFinite(val) => {
                self.output.push_str("isFinite(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsNaN(val) => {
                self.output.push_str("Number.isNaN(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsFinite(val) => {
                self.output.push_str("Number.isFinite(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsInteger(val) => {
                self.output.push_str("Number.isInteger(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::NumberIsSafeInteger(val) => {
                self.output.push_str("Number.isSafeInteger(");
                self.emit_expr(val);
                self.output.push(')');
            }

            // --- Static plugin resolve ---
            Expr::StaticPluginResolve(_) => {
                self.output.push_str("undefined");
            }
            Expr::PerformanceNow => {
                self.output.push_str("performance.now()");
            }
            Expr::TextEncoderNew => { self.output.push_str("new TextEncoder()"); }
            Expr::TextDecoderNew => { self.output.push_str("new TextDecoder()"); }
            Expr::TextEncoderEncode(inner) => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::TextDecoderDecode(inner) => {
                self.output.push_str("new TextDecoder().decode(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::EncodeURI(inner) => {
                self.output.push_str("encodeURI(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::DecodeURI(inner) => {
                self.output.push_str("decodeURI(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::EncodeURIComponent(inner) => {
                self.output.push_str("encodeURIComponent(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::DecodeURIComponent(inner) => {
                self.output.push_str("decodeURIComponent(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::StructuredClone(inner) => {
                self.output.push_str("structuredClone(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::QueueMicrotask(inner) => {
                self.output.push_str("queueMicrotask(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::Atob(inner) => {
                self.output.push_str("atob(");
                self.emit_expr(inner);
                self.output.push(')');
            }
            Expr::Btoa(inner) => {
                self.output.push_str("btoa(");
                self.emit_expr(inner);
                self.output.push(')');
            }

            // --- V8/JS interop (passthrough in browser) ---
            Expr::JsLoadModule { path } => {
                let _ = write!(self.output, "((() => {{ throw new Error('JsLoadModule not supported in browser: {}'); }})())", path);
            }
            Expr::JsGetExport { module_handle, export_name } => {
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}", export_name);
            }
            Expr::JsCallFunction { module_handle, func_name, args } => {
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}(", func_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsCallMethod { object, method_name, args } => {
                self.emit_expr(object);
                let _ = write!(self.output, ".{}(", method_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsGetProperty { object, property_name } => {
                self.emit_expr(object);
                let _ = write!(self.output, ".{}", property_name);
            }
            Expr::JsSetProperty { object, property_name, value } => {
                self.output.push('(');
                self.emit_expr(object);
                let _ = write!(self.output, ".{} = ", property_name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::JsNew { module_handle, class_name, args } => {
                self.output.push_str("new ");
                self.emit_expr(module_handle);
                let _ = write!(self.output, ".{}(", class_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsNewFromHandle { constructor, args } => {
                self.output.push_str("new (");
                self.emit_expr(constructor);
                self.output.push_str(")(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::JsCreateCallback { closure, .. } => {
                self.emit_expr(closure);
            }

            // --- ImportMetaUrl ---
            Expr::ImportMetaUrl(path) => {
                let _ = write!(self.output, "{}", self.quote_string(path));
            }

            // --- Math.imul ---
            Expr::MathImul(a, b) => {
                let _ = write!(self.output, "Math.imul(");
                self.emit_expr(a);
                let _ = write!(self.output, ", ");
                self.emit_expr(b);
                let _ = write!(self.output, ")");
            }
            Expr::StringFromCodePoint(arg) => {
                self.output.push_str("String.fromCodePoint(");
                self.emit_expr(arg);
                self.output.push(')');
            }
            Expr::StringAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".at(");
                self.emit_expr(index);
                self.output.push(')');
            }
            Expr::StringCodePointAt { string, index } => {
                self.emit_expr(string);
                self.output.push_str(".codePointAt(");
                self.emit_expr(index);
                self.output.push(')');
            }
            // Object property descriptor stubs
            Expr::ObjectDefineProperty(obj, key, desc) => {
                self.output.push_str("Object.defineProperty("); self.emit_expr(obj);
                self.output.push_str(", "); self.emit_expr(key);
                self.output.push_str(", "); self.emit_expr(desc); self.output.push(')');
            }
            Expr::ObjectGetOwnPropertyDescriptor(obj, key) => {
                self.output.push_str("Object.getOwnPropertyDescriptor("); self.emit_expr(obj);
                self.output.push_str(", "); self.emit_expr(key); self.output.push(')');
            }
            Expr::ObjectGetOwnPropertyNames(obj) => { self.output.push_str("Object.getOwnPropertyNames("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectCreate(proto) => { self.output.push_str("Object.create("); self.emit_expr(proto); self.output.push(')'); }
            Expr::ObjectFreeze(obj) => { self.output.push_str("Object.freeze("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectSeal(obj) => { self.output.push_str("Object.seal("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectPreventExtensions(obj) => { self.output.push_str("Object.preventExtensions("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectIsFrozen(obj) => { self.output.push_str("Object.isFrozen("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectIsSealed(obj) => { self.output.push_str("Object.isSealed("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectIsExtensible(obj) => { self.output.push_str("Object.isExtensible("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectGetPrototypeOf(obj) => { self.output.push_str("Object.getPrototypeOf("); self.emit_expr(obj); self.output.push(')'); }
            Expr::ObjectGetOwnPropertySymbols(obj) => { self.output.push_str("Object.getOwnPropertySymbols("); self.emit_expr(obj); self.output.push(')'); }
            // Symbol stubs
            Expr::SymbolNew(desc) => {
                self.output.push_str("Symbol(");
                if let Some(d) = desc { self.emit_expr(d); }
                self.output.push(')');
            }
            Expr::SymbolFor(key) => { self.output.push_str("Symbol.for("); self.emit_expr(key); self.output.push(')'); }
            Expr::SymbolKeyFor(sym) => { self.output.push_str("Symbol.keyFor("); self.emit_expr(sym); self.output.push(')'); }
            Expr::SymbolDescription(sym) => { self.emit_expr(sym); self.output.push_str(".description"); }
            Expr::SymbolToString(sym) => { self.emit_expr(sym); self.output.push_str(".toString()"); }
            // RegExp stubs
            Expr::RegExpExec { regex, string } => { self.emit_expr(regex); self.output.push_str(".exec("); self.emit_expr(string); self.output.push(')'); }
            Expr::RegExpSource(re) => { self.emit_expr(re); self.output.push_str(".source"); }
            Expr::RegExpFlags(re) => { self.emit_expr(re); self.output.push_str(".flags"); }
            Expr::RegExpLastIndex(re) => { self.emit_expr(re); self.output.push_str(".lastIndex"); }
            Expr::RegExpSetLastIndex { regex, value } => { self.emit_expr(regex); self.output.push_str(".lastIndex = "); self.emit_expr(value); }
            Expr::RegExpReplaceFn { string, regex, callback } => { self.emit_expr(string); self.output.push_str(".replace("); self.emit_expr(regex); self.output.push_str(", "); self.emit_expr(callback); self.output.push(')'); }
            Expr::RegExpExecIndex => { self.output.push_str("__perry_exec_index"); }
            Expr::RegExpExecGroups => { self.output.push_str("__perry_exec_groups"); }
            // Proxy / Reflect — JS backend emits direct JS forms.
            Expr::ProxyNew { target, handler } => {
                self.output.push_str("new Proxy(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push(')');
            }
            Expr::ProxyGet { proxy, key } => {
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push(']');
            }
            Expr::ProxySet { proxy, key, value } => {
                self.output.push('(');
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push_str(", true)");
            }
            Expr::ProxyHas { proxy, key } => {
                self.output.push('(');
                self.emit_expr(key);
                self.output.push_str(" in ");
                self.emit_expr(proxy);
                self.output.push(')');
            }
            Expr::ProxyDelete { proxy, key } => {
                self.output.push_str("delete ");
                self.emit_expr(proxy);
                self.output.push('[');
                self.emit_expr(key);
                self.output.push(']');
            }
            Expr::ProxyApply { proxy, args } => {
                self.emit_expr(proxy);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::ProxyConstruct { proxy, args } => {
                self.output.push_str("new ");
                self.emit_expr(proxy);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::ProxyRevocable { target, handler } => {
                self.output.push_str("Proxy.revocable(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push(')');
            }
            Expr::ProxyRevoke(_) => {
                self.output.push_str("undefined");
            }
            Expr::ReflectGet { target, key } => {
                self.output.push_str("Reflect.get(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectSet { target, key, value } => {
                self.output.push_str("Reflect.set(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::ReflectHas { target, key } => {
                self.output.push_str("Reflect.has(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectDelete { target, key } => {
                self.output.push_str("Reflect.deleteProperty(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push(')');
            }
            Expr::ReflectOwnKeys(target) => {
                self.output.push_str("Reflect.ownKeys(");
                self.emit_expr(target);
                self.output.push(')');
            }
            Expr::ReflectApply { func, this_arg, args } => {
                self.output.push_str("Reflect.apply(");
                self.emit_expr(func);
                self.output.push_str(", ");
                self.emit_expr(this_arg);
                self.output.push_str(", ");
                self.emit_expr(args);
                self.output.push(')');
            }
            Expr::ReflectConstruct { target, args } => {
                self.output.push_str("Reflect.construct(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(args);
                self.output.push(')');
            }
            Expr::ReflectDefineProperty { target, key, descriptor } => {
                self.output.push_str("Reflect.defineProperty(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(key);
                self.output.push_str(", ");
                self.emit_expr(descriptor);
                self.output.push(')');
            }
            Expr::ReflectGetPrototypeOf(target) => {
                self.output.push_str("Reflect.getPrototypeOf(");
                self.emit_expr(target);
                self.output.push(')');
            }
            // Fallback for HIR variants the JS emitter doesn't model directly
            // (e.g. TypedArrayNew). Emit `undefined` so the emitted JS still
            // parses; these paths are unused for the LLVM-backend sweeps.
            _ => self.output.push_str("undefined"),
        }
    }

    // --- Native method call mapping ---

    fn emit_native_method_call(
        &mut self,
        module: &str,
        class_name: Option<&str>,
        object: Option<&Expr>,
        method: &str,
        args: &[Expr],
    ) {
        let normalized_module = module.strip_prefix("node:").unwrap_or(module);

        match normalized_module {
            "perry/ui" => {
                self.emit_ui_method_call(class_name, object, method, args);
            }
            "perry/system" => {
                self.emit_system_method_call(method, args);
            }
            "console" => {
                self.emit_console_call(method, args);
            }
            // --- Timer functions ---
            _ if method == "setTimeout" => {
                self.output.push_str("setTimeout(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "setInterval" => {
                self.output.push_str("setInterval(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "clearTimeout" => {
                self.output.push_str("clearTimeout(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "clearInterval" => {
                self.output.push_str("clearInterval(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            // --- File System (fs module — serve from web file cache) ---
            "fs" => {
                match method {
                    "readFileSync" => {
                        self.output.push_str("__perry.fs_readFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "readdirSync" => {
                        self.output.push_str("__perry.fs_readdirSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "isDirectory" => {
                        self.output.push_str("__perry.fs_isDirectory(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "existsSync" => {
                        self.output.push_str("__perry.fs_existsSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "writeFileSync" => {
                        self.output.push_str("__perry.fs_writeFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "mkdirSync" => {
                        self.output.push_str("__perry.fs_mkdirSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "unlinkSync" => {
                        self.output.push_str("__perry.fs_unlinkSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "appendFileSync" => {
                        self.output.push_str("__perry.fs_appendFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    _ => {
                        // Graceful fallback — log warning instead of throwing
                        let _ = write!(
                            self.output,
                            "(console.warn('fs.{} not available in browser'), \"\")",
                            method
                        );
                    }
                }
            }
            // --- child_process (stub in browser) ---
            "child_process" => match method {
                "execSync" => {
                    self.output.push_str("__perry.child_process_execSync(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                _ => {
                    let _ = write!(
                        self.output,
                        "(console.warn('child_process.{} not available in browser'), \"\")",
                        method
                    );
                }
            },
            // --- node-fetch (Perry native SSE streaming → Fetch API on web) ---
            "node-fetch" => match method {
                "streamStart" => {
                    self.output.push_str("__perry.stream_start(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamPoll" => {
                    self.output.push_str("__perry.stream_poll(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamStatus" => {
                    self.output.push_str("__perry.stream_status(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamClose" => {
                    self.output.push_str("__perry.stream_close(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                _ => {
                    let _ = write!(
                        self.output,
                        "(console.warn('node-fetch.{} not available in browser'), \"\")",
                        method
                    );
                }
            },
            // --- child_process: spawnBackground (stub) ---
            _ if method == "spawnBackground" => {
                self.output
                    .push_str("(console.warn('spawnBackground not available in browser'), 0)");
            }
            // --- Fastify/HTTP (throw in browser) ---
            "fastify" | "ws" | "mysql2" | "mysql2/promise" | "pg" | "net" | "worker_threads" => {
                let _ = write!(
                    self.output,
                    "((() => {{ throw new Error('{} not available in browser'); }})())",
                    normalized_module
                );
            }
            // --- Events module ---
            "events"
                if method == "on"
                    || method == "addEventListener"
                    || method == "emit"
                    || method == "removeListener" =>
            {
                if let Some(obj) = object {
                    self.emit_expr(obj);
                    let _ = write!(self.output, ".{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                } else {
                    self.output.push_str("undefined");
                }
            }
            // --- Default: try to emit as method call on object ---
            _ => {
                if let Some(obj) = object {
                    self.emit_expr(obj);
                    let _ = write!(self.output, ".{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                } else {
                    // Static-style call - just emit as function call
                    let _ = write!(self.output, "{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
            }
        }
    }

    fn emit_console_call(&mut self, method: &str, args: &[Expr]) {
        let _ = write!(self.output, "console.{}(", method);
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
        }
        self.output.push(')');
    }

    fn emit_system_method_call(&mut self, method: &str, args: &[Expr]) {
        match method {
            "openURL" | "open_url" => {
                self.output.push_str("window.open(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(", '_blank')");
            }
            "isDarkMode" | "is_dark_mode" => {
                self.output.push_str("(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 1.0 : 0.0)");
            }
            "preferencesGet" | "preferences_get" => {
                self.output.push_str("(localStorage.getItem(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(") || '')");
            }
            "preferencesSet" | "preferences_set" => {
                self.output.push_str("localStorage.setItem(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(", ");
                if let Some(a) = args.get(1) {
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            "audioStart" | "audio_start" => {
                self.output.push_str("perry_system_audio_start()");
            }
            "audioStop" | "audio_stop" => {
                self.output.push_str("perry_system_audio_stop()");
            }
            "audioGetLevel" | "audio_get_level" => {
                self.output.push_str("perry_system_audio_get_level()");
            }
            "audioGetPeak" | "audio_get_peak" => {
                self.output.push_str("perry_system_audio_get_peak()");
            }
            "audioGetWaveformSamples" | "audio_get_waveform" => {
                self.output.push_str("perry_system_audio_get_waveform(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            "getDeviceModel" | "get_device_model" => {
                self.output.push_str("perry_system_get_device_model()");
            }
            "getAppVersion" | "get_app_version" => {
                let s = self.quote_string(&self.app_metadata.version.clone());
                self.output.push_str(&s);
            }
            "getAppBuildNumber" | "get_app_build_number" => {
                let _ = write!(self.output, "{}", self.app_metadata.build_number);
            }
            "getBundleId" | "get_bundle_id" => {
                let s = self.quote_string(&self.app_metadata.bundle_id.clone());
                self.output.push_str(&s);
            }
            _ => {
                let _ = write!(
                    self.output,
                    "console.warn('perry/system.{} not available in browser')",
                    method
                );
            }
        }
    }

    fn emit_ui_method_call(
        &mut self,
        class_name: Option<&str>,
        object: Option<&Expr>,
        method: &str,
        args: &[Expr],
    ) {
        // Map perry/ui methods to __perry.perry_ui_* calls
        let ui_fn = match method {
            // Widget creation
            "App" | "app_create" => "perry_ui_app_create",
            "VStack" | "vstack_create" => "perry_ui_vstack_create",
            "HStack" | "hstack_create" => "perry_ui_hstack_create",
            "ZStack" | "zstack_create" => "perry_ui_zstack_create",
            "Text" | "text_create" => "perry_ui_text_create",
            "Button" | "button_create" => "perry_ui_button_create",
            "TextField" | "textfield_create" => "perry_ui_textfield_create",
            "SecureField" | "securefield_create" => "perry_ui_securefield_create",
            "Toggle" | "toggle_create" => "perry_ui_toggle_create",
            "Slider" | "slider_create" => "perry_ui_slider_create",
            "ScrollView" | "scrollview_create" => "perry_ui_scrollview_create",
            "scrollViewSetChild" => "perry_ui_scrollview_set_child",
            "scrollViewScrollTo" => "perry_ui_scrollview_scroll_to",
            "scrollViewGetOffset" => "perry_ui_scrollview_get_offset",
            "scrollViewSetOffset" => "perry_ui_scrollview_set_offset",
            "Spacer" | "spacer_create" => "perry_ui_spacer_create",
            "Divider" | "divider_create" => "perry_ui_divider_create",
            "ProgressView" | "progressview_create" => "perry_ui_progressview_create",
            "Image" | "image_create" => "perry_ui_image_create",
            "Picker" | "picker_create" => "perry_ui_picker_create",
            // Table (issue #192)
            "Table" | "table_create" => "perry_ui_table_create",
            "tableSetColumnHeader" => "perry_ui_table_set_column_header",
            "tableSetColumnWidth" => "perry_ui_table_set_column_width",
            "tableUpdateRowCount" => "perry_ui_table_update_row_count",
            "tableSetOnRowSelect" => "perry_ui_table_set_on_row_select",
            "tableGetSelectedRow" => "perry_ui_table_get_selected_row",
            // Camera (issue #191) — browser stubs in wasm_runtime.js return
            // 0 / -1 since there's no DOM equivalent for live capture; the
            // dispatch entries here exist so the JS-target compile resolves
            // the names rather than emitting `perry_ui_unknown`.
            "CameraView" | "camera_create" => "perry_ui_camera_create",
            "cameraStart" => "perry_ui_camera_start",
            "cameraStop" => "perry_ui_camera_stop",
            "cameraFreeze" => "perry_ui_camera_freeze",
            "cameraUnfreeze" => "perry_ui_camera_unfreeze",
            "cameraSampleColor" => "perry_ui_camera_sample_color",
            "cameraSetOnTap" => "perry_ui_camera_set_on_tap",
            "Form" | "form_create" => "perry_ui_form_create",
            "Section" | "section_create" => "perry_ui_section_create",
            "NavigationStack" | "navigationstack_create" => "perry_ui_navigationstack_create",
            "Canvas" | "canvas_create" => "perry_ui_canvas_create",
            // Child management
            "addChild" | "widget_add_child" => "perry_ui_widget_add_child",
            "removeAllChildren" | "widget_remove_all_children" => {
                "perry_ui_widget_remove_all_children"
            }
            // Styling
            "setBackground" | "set_background" => "perry_ui_set_background",
            "setForeground" | "set_foreground" => "perry_ui_set_foreground",
            "setFontSize" | "set_font_size" => "perry_ui_set_font_size",
            "setFontWeight" | "set_font_weight" => "perry_ui_set_font_weight",
            "setFontFamily" | "set_font_family" => "perry_ui_set_font_family",
            "setPadding" | "set_padding" => "perry_ui_set_padding",
            "setFrame" | "set_frame" => "perry_ui_set_frame",
            "setCornerRadius" | "set_corner_radius" => "perry_ui_set_corner_radius",
            "setBorder" | "set_border" => "perry_ui_set_border",
            "setOpacity" | "set_opacity" => "perry_ui_set_opacity",
            "setEnabled" | "set_enabled" => "perry_ui_set_enabled",
            "setTooltip" | "set_tooltip" => "perry_ui_set_tooltip",
            "setControlSize" | "set_control_size" => "perry_ui_set_control_size",
            // Animations
            "animateOpacity" | "animate_opacity" => "perry_ui_animate_opacity",
            "animatePosition" | "animate_position" => "perry_ui_animate_position",
            // widget-prefixed free-function forms (used by HIR reactive desugar)
            "widgetAnimateOpacity" => "perry_ui_animate_opacity",
            "widgetAnimatePosition" => "perry_ui_animate_position",
            // Events
            "setOnClick" | "set_on_click" => "perry_ui_set_on_click",
            "setOnHover" | "set_on_hover" => "perry_ui_set_on_hover",
            "setOnDoubleClick" | "set_on_double_click" => "perry_ui_set_on_double_click",
            // State
            "createState" | "state_create" => {
                self.output.push_str("__perry.stateCreate(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
                return;
            }
            "get" if class_name == Some("State") => {
                self.output.push_str("__perry.stateGet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                self.output.push(')');
                return;
            }
            "set" if class_name == Some("State") => {
                self.output.push_str("__perry.stateSet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                for arg in args {
                    self.output.push_str(", ");
                    self.emit_expr(arg);
                }
                self.output.push(')');
                return;
            }
            "value" if class_name == Some("State") => {
                self.output.push_str("__perry.stateGet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                self.output.push(')');
                return;
            }
            // State bindings
            "bindText" | "state_bind_text" => "perry_ui_state_bind_text",
            "bindTextNumeric" | "state_bind_text_numeric" => "perry_ui_state_bind_text_numeric",
            "bindSlider" | "state_bind_slider" => "perry_ui_state_bind_slider",
            "bindToggle" | "state_bind_toggle" => "perry_ui_state_bind_toggle",
            "bindVisibility" | "state_bind_visibility" => "perry_ui_state_bind_visibility",
            "bindForEach" | "state_bind_foreach" => "perry_ui_state_bind_foreach",
            "onChange" | "state_on_change" => "perry_ui_state_on_change",
            // Canvas
            "fillRect" | "canvas_fill_rect" => "perry_ui_canvas_fill_rect",
            "strokeRect" | "canvas_stroke_rect" => "perry_ui_canvas_stroke_rect",
            "clearRect" | "canvas_clear_rect" => "perry_ui_canvas_clear_rect",
            "setFillColor" | "canvas_set_fill_color" => "perry_ui_canvas_set_fill_color",
            "setStrokeColor" | "canvas_set_stroke_color" => "perry_ui_canvas_set_stroke_color",
            "beginPath" | "canvas_begin_path" => "perry_ui_canvas_begin_path",
            "moveTo" | "canvas_move_to" => "perry_ui_canvas_move_to",
            "lineTo" | "canvas_line_to" => "perry_ui_canvas_line_to",
            "arc" | "canvas_arc" => "perry_ui_canvas_arc",
            "closePath" | "canvas_close_path" => "perry_ui_canvas_close_path",
            "fill" | "canvas_fill" => "perry_ui_canvas_fill",
            "stroke" | "canvas_stroke" => "perry_ui_canvas_stroke",
            "setLineWidth" | "canvas_set_line_width" => "perry_ui_canvas_set_line_width",
            "fillText" | "canvas_fill_text" => "perry_ui_canvas_fill_text",
            "setFont" | "canvas_set_font" => "perry_ui_canvas_set_font",
            // Hone IDE camelCase free-function imports
            "textSetColor" => "perry_ui_set_foreground",
            "textSetFontSize" => "perry_ui_set_font_size",
            "textSetFontWeight" => "perry_ui_set_font_weight",
            "textSetFontFamily" => "perry_ui_set_font_family",
            "textSetString" => "perry_ui_text_set_string",
            "buttonSetBordered" => "perry_ui_button_set_bordered",
            "buttonSetTextColor" => "perry_ui_button_set_text_color",
            "buttonSetTitle" => "perry_ui_button_set_title",
            "buttonSetImage" => "perry_ui_button_set_image",
            "buttonSetContentTintColor" => "perry_ui_button_set_content_tint_color",
            "widgetSetBackgroundColor" => "perry_ui_set_background",
            "widgetAddChild" => "perry_ui_widget_add_child",
            "widgetRemoveChild" => "perry_ui_widget_remove_child",
            "widgetReorderChild" => "perry_ui_widget_reorder_child",
            "widgetClearChildren" => "perry_ui_widget_remove_all_children",
            "widgetSetWidth" => "perry_ui_widget_set_width",
            "widgetSetHeight" => "perry_ui_widget_set_height",
            "widgetSetHugging" => "perry_ui_widget_set_hugging",
            "widgetSetHidden" => "perry_ui_set_widget_hidden",
            "widgetMatchParentHeight" => "perry_ui_widget_match_parent_height",
            "widgetMatchParentWidth" => "perry_ui_widget_match_parent_width",
            "widgetAddOverlay" => "perry_ui_widget_add_overlay",
            "widgetSetOverlayFrame" => "perry_ui_widget_set_overlay_frame",
            "widgetSetEdgeInsets" => "perry_ui_widget_set_edge_insets",
            "widgetSetContextMenu" => "perry_ui_widget_set_context_menu",
            "stackSetDetachesHidden" => "perry_ui_stack_set_detaches_hidden",
            "stackSetDistribution" => "perry_ui_stack_set_distribution",
            "buttonSetImagePosition" => "perry_ui_button_set_image_position",
            "textSetColor" => "perry_ui_text_set_color",
            "textSetWraps" => "perry_ui_text_set_wraps",
            "textfieldSetString" => "perry_ui_textfield_set_string",
            "textfieldGetString" => "perry_ui_textfield_get_string",
            "textfieldFocus" => "perry_ui_textfield_focus",
            "textfieldBlurAll" => "perry_ui_textfield_blur_all",
            "textfieldSetOnSubmit" => "perry_ui_textfield_set_on_submit",
            "textfieldSetOnFocus" => "perry_ui_textfield_set_on_focus",
            "pollOpenFile" => "perry_ui_poll_open_file",
            "frameSplitCreate" => "perry_ui_frame_split_create",
            "frameSplitAddChild" => "perry_ui_frame_split_add_child",
            "saveFileDialog" => "perry_ui_save_file_dialog",
            "VStackWithInsets" => "perry_ui_vstack_create_with_insets",
            "HStackWithInsets" => "perry_ui_hstack_create_with_insets",
            "embedNSView" => "perry_ui_embed_ns_view",
            "openFolderDialog" => "perry_ui_open_folder_dialog",
            "openFileDialog" => "perry_ui_open_file_dialog",
            // App lifecycle
            "run" | "app_run" => "perry_ui_app_run",
            // Menu
            "menuCreate" | "menu_create" => "perry_ui_menu_create",
            "menuAddItem" | "menu_add_item" => "perry_ui_menu_add_item",
            "menuAddStandardAction" | "menu_add_standard_action" => {
                "perry_ui_menu_add_standard_action"
            }
            "menuClear" | "menu_clear" => "perry_ui_menu_clear",
            "menuAddSeparator" | "menu_add_separator" => "perry_ui_menu_add_separator",
            "menuAddSubmenu" | "menu_add_submenu" => "perry_ui_menu_add_submenu",
            "menuBarCreate" | "menubar_create" => "perry_ui_menubar_create",
            "menuBarAddMenu" | "menubar_add_menu" => "perry_ui_menubar_add_menu",
            "menuBarAttach" | "menubar_attach" => "perry_ui_menubar_attach",
            // Default — try the centralised perry-dispatch tables first
            // (Tier 1.3, v0.5.332). PERRY_UI_TABLE / PERRY_UI_INSTANCE_TABLE
            // / PERRY_SYSTEM_TABLE are the canonical source of truth, so
            // any new perry/ui or perry/system method added there resolves
            // on `--target web` without a parallel edit here. The static
            // arms above are kept for legacy snake_case aliases (`app_create`,
            // `vstack_create`, …) that aren't in PERRY_UI_TABLE because
            // the LLVM backend only expects the canonical camelCase names.
            _ => {
                if let Some(rt) = perry_dispatch::ui_method_to_runtime(method) {
                    rt
                } else {
                    // Last-resort fallback: emit as __perry.perry_ui_<method>(...).
                    // The browser-side runtime will throw "function not found"
                    // at call time if the symbol doesn't exist — this preserves
                    // the pre-1.3 best-effort behavior for unknown methods.
                    let _ = write!(self.output, "__perry.perry_ui_{}(", method);
                    if let Some(obj) = object {
                        self.emit_expr(obj);
                        if !args.is_empty() {
                            self.output.push_str(", ");
                        }
                    }
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                    return;
                }
            }
        };

        // Emit the __perry.fn_name(object?, args...) call
        let _ = write!(self.output, "__perry.{}(", ui_fn);
        let mut first = true;
        if let Some(obj) = object {
            self.emit_expr(obj);
            first = false;
        }
        for arg in args {
            if !first {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
            first = false;
        }
        self.output.push(')');
    }

    // --- Helpers ---

    fn emit_math_unary(&mut self, func: &str, arg: &Expr) {
        self.output.push_str(func);
        self.output.push('(');
        self.emit_expr(arg);
        self.output.push(')');
    }

    fn emit_math_variadic(&mut self, func: &str, args: &[Expr]) {
        self.output.push_str(func);
        self.output.push('(');
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
        }
        self.output.push(')');
    }

    fn quote_string(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len() + 2);
        result.push('"');
        for ch in s.chars() {
            match ch {
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\0' => result.push_str("\\0"),
                c if c < ' ' => {
                    let _ = write!(result, "\\x{:02x}", c as u32);
                }
                c => result.push(c),
            }
        }
        result.push('"');
        result
    }
}

/// Check if a string is a valid JavaScript identifier
fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Generate a short variable name from a counter value.
///
/// Produces: a, b, ..., z, A, ..., Z, aa, ab, ..., az, aA, ..., aZ, ba, ...
/// Uses bijective base-52 encoding (a-z, A-Z).
fn gen_short_name(n: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let base = CHARS.len(); // 52
    let mut result = Vec::new();
    let mut val = n;
    loop {
        result.push(CHARS[val % base] as char);
        if val < base {
            break;
        }
        val = val / base - 1;
    }
    result.reverse();
    result.into_iter().collect()
}

/// Check if a string is a JavaScript reserved word.
fn is_js_reserved(s: &str) -> bool {
    matches!(
        s,
        "do" | "if"
            | "in"
            | "for"
            | "let"
            | "new"
            | "try"
            | "var"
            | "case"
            | "else"
            | "enum"
            | "null"
            | "this"
            | "true"
            | "void"
            | "with"
            | "break"
            | "catch"
            | "class"
            | "const"
            | "false"
            | "super"
            | "throw"
            | "while"
            | "yield"
            | "delete"
            | "export"
            | "import"
            | "return"
            | "switch"
            | "typeof"
            | "default"
            | "extends"
            | "finally"
            | "continue"
            | "debugger"
            | "function"
            | "arguments"
            | "instanceof"
            | "of"
    )
}
