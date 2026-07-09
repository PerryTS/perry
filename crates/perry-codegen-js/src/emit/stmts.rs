use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
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
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}
