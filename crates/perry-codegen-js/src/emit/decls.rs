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
        self.emit_params(&func.params);
        self.output.push_str(") {\n");
        self.indent += 1;
        for stmt in &func.body {
            self.emit_stmt(stmt);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    pub(super) fn emit_params(&mut self, params: &[Param]) {
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
}
