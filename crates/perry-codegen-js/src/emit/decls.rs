use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
    pub(super) fn emit_enum(&mut self, en: &Enum) {
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

    pub(super) fn emit_global(&mut self, global: &Global) {
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

    pub(super) fn emit_class(&mut self, class: &Class) {
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
            let default_params = self.emit_params(&ctor.params, true);
            self.output.push_str(") {\n");
            self.indent += 1;
            let temp_scope = self.begin_scoped_temp_scope();
            self.emit_parameter_defaults(&default_params);

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
            self.finish_parameter_defaults(&default_params);
            self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
            self.indent -= 1;
            self.writeln("}");
        } else if !class.fields.is_empty() {
            // Auto-generate constructor with field initializers
            self.write_indent();
            self.output.push_str("constructor() {\n");
            self.indent += 1;
            let temp_scope = self.begin_scoped_temp_scope();
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
            self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
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
            let temp_scope = self.begin_scoped_temp_scope();
            for stmt in &func.body {
                self.emit_stmt(stmt);
            }
            self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
            self.indent -= 1;
            self.writeln("}");
        }

        // Setters
        for (prop_name, func) in &class.setters {
            self.write_indent();
            let _ = write!(self.output, "set {}(", prop_name);
            let default_params = self.emit_params(&func.params, !func.is_generator);
            self.output.push_str(") {\n");
            self.indent += 1;
            let temp_scope = self.begin_scoped_temp_scope();
            self.emit_parameter_defaults(&default_params);
            for stmt in &func.body {
                self.emit_stmt(stmt);
            }
            self.finish_parameter_defaults(&default_params);
            self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
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
            let default_params = self.emit_params(&method.params, !method.is_generator);
            self.output.push_str(") {\n");
            self.indent += 1;
            let temp_scope = self.begin_scoped_temp_scope();
            self.emit_parameter_defaults(&default_params);
            for stmt in &method.body {
                self.emit_stmt(stmt);
            }
            self.finish_parameter_defaults(&default_params);
            self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
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
                self.clear_scoped_temps();
            }
        }
    }

    pub(super) fn emit_method(&mut self, method: &Function) {
        self.write_indent();
        if method.is_async {
            self.output.push_str("async ");
        }
        if method.is_generator {
            let _ = write!(self.output, "*{}(", method.name);
        } else {
            let _ = write!(self.output, "{}(", method.name);
        }
        let default_params = self.emit_params(&method.params, !method.is_generator);
        self.output.push_str(") {\n");
        self.indent += 1;
        let temp_scope = self.begin_scoped_temp_scope();
        self.emit_parameter_defaults(&default_params);
        for stmt in &method.body {
            self.emit_stmt(stmt);
        }
        self.finish_parameter_defaults(&default_params);
        self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
        self.indent -= 1;
        self.writeln("}");
    }

    // --- Function emission ---

    pub(super) fn emit_function(&mut self, func: &Function) {
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
        let default_params = self.emit_params(&func.params, !func.is_generator);
        self.output.push_str(") {\n");
        self.indent += 1;
        let temp_scope = self.begin_scoped_temp_scope();
        self.emit_parameter_defaults(&default_params);
        for stmt in &func.body {
            self.emit_stmt(stmt);
        }
        self.finish_parameter_defaults(&default_params);
        self.finish_scoped_temp_scope(temp_scope.0, temp_scope.1, temp_scope.2);
        self.indent -= 1;
        self.writeln("}");
    }

    /// Default expressions with scoped captures need body-local declarations.
    /// Argument aliases preserve the signature's arity; sequential lexical
    /// initialization preserves earlier-parameter reads and later-parameter TDZ.
    /// Generators retain parameter evaluation at iterator creation time.
    pub(super) fn emit_params(
        &mut self,
        params: &[Param],
        lower_scoped_defaults: bool,
    ) -> Vec<(Param, String)> {
        fn has_capture(expr: &Expr) -> bool {
            if matches!(expr, Expr::ScopedTemp { .. }) {
                return true;
            }
            if matches!(expr, Expr::Closure { .. }) {
                return false;
            }
            let mut found = false;
            perry_hir::walker::walk_expr_children(expr, &mut |child| found |= has_capture(child));
            found
        }
        let lower_defaults = lower_scoped_defaults
            && params
                .iter()
                .any(|param| param.default.as_ref().is_some_and(has_capture));
        if lower_defaults {
            for param in params {
                self.make_local_name(&param.name, param.id);
            }
        }
        let mut defaults = Vec::new();
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            if param.is_rest {
                self.output.push_str("...");
            }
            let name = if lower_defaults {
                let mut alias = format!("__perry_arg_{}", param.id);
                while self.used_names.contains(&alias) {
                    alias.push('_');
                }
                self.used_names.insert(alias.clone());
                defaults.push((param.clone(), alias.clone()));
                alias
            } else {
                self.make_local_name(&param.name, param.id)
            };
            self.output.push_str(&name);
            if let Some(default) = &param.default {
                self.output.push_str(" = ");
                if lower_defaults {
                    self.output.push_str("undefined");
                } else {
                    let previous = self.in_parameter_default;
                    self.in_parameter_default = true;
                    self.emit_expr(default);
                    self.in_parameter_default = previous;
                }
            }
        }
        defaults
    }

    pub(super) fn emit_parameter_defaults(&mut self, defaults: &[(Param, String)]) {
        for (param, alias) in defaults {
            let name = self.get_local_name(param.id);
            self.write_indent();
            let _ = write!(self.output, "let {name} = ");
            if let Some(default) = &param.default {
                let _ = write!(self.output, "{alias} === undefined ? ");
                self.emit_expr(default);
                let _ = write!(self.output, " : {alias}");
            } else {
                self.output.push_str(alias);
            }
            self.output.push_str(";\n");
        }
        if !defaults.is_empty() {
            // Body declarations live in the original body environment, outside
            // the environment in which parameter initializers resolve names.
            self.writeln("{");
            self.indent += 1;
        }
    }

    pub(super) fn finish_parameter_defaults(&mut self, defaults: &[(Param, String)]) {
        if !defaults.is_empty() {
            self.indent -= 1;
            self.writeln("}");
        }
    }
}
