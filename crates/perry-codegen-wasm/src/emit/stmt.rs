//! Statement emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `FuncEmitCtx::emit_stmt`, `FuncEmitCtx::expr_has_value`,
//! and the free `has_return` helper. Methods stay on the same struct via
//! a separate `impl<'a> FuncEmitCtx<'a>` block (idiomatic Rust).

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn emit_stmt(&mut self, func: &mut Function, stmt: &Stmt, in_returning_func: bool) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                if let Some(init_expr) = init {
                    self.emit_expr(func, init_expr);
                } else {
                    // Default: undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                // Check module_let_globals FIRST (top-level Let → WASM global), then local_map
                if let Some(&gidx) = self
                    .emitter
                    .module_let_globals
                    .get(&(self.emitter.current_mod_idx, *id))
                {
                    func.instruction(&Instruction::GlobalSet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalSet(idx));
                } else {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Expr(expr) => {
                self.emit_expr(func, expr);
                // Drop the result (expression statement)
                // Check if expr produces a value
                if self.expr_has_value(expr) {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.emit_expr(func, e);
                    // Issue #1081: some Expr arms (notably `console.log/warn/error`
                    // and other void-returning calls in `Expr::Call` -> PropertyGet)
                    // intentionally do not push a result onto the operand stack.
                    // When such an expression is the body of `return e` (e.g. the
                    // expression-bodied arrow `v => console.log(v)`), the WASM
                    // function signature still expects exactly one i64 result, so
                    // the bare `return` would fail validation with
                    // "expected 1 elements on the stack for return, found 0".
                    // Push `undefined` in that case to mirror JS semantics
                    // (a void call returns `undefined`).
                    if in_returning_func && !self.expr_has_value(e) {
                        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    }
                } else if in_returning_func {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                func.instruction(&Instruction::Return);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // Convert to i32 boolean via is_truthy
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                for s in then_branch {
                    self.emit_stmt(func, s, in_returning_func);
                }
                if let Some(else_stmts) = else_branch {
                    func.instruction(&Instruction::Else);
                    for s in else_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::While { condition, body } => {
                // block $break
                //   loop $continue
                //     <condition>
                //     is_truthy
                //     i32.eqz
                //     br_if $break (1)
                //     <body>
                //     br $continue (0)
                //   end
                // end
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_depth = self.block_depth;
                self.break_depth.push(break_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_depth = self.block_depth;
                self.loop_depth.push(continue_depth);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_depth, continue_depth));
                    true
                } else {
                    false
                };

                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::BrIf(1)); // break to outer block

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                func.instruction(&Instruction::Br(0)); // continue (loop back)
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end loop

                if label_pushed {
                    self.label_stack.pop();
                }
                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end block
            }
            Stmt::DoWhile { body, condition } => {
                // block $break
                //   loop $continue
                //     <body>
                //     <condition>
                //     is_truthy
                //     br_if $continue (0) — loop back if truthy
                //   end
                // end
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_depth = self.block_depth;
                self.break_depth.push(break_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_depth = self.block_depth;
                self.loop_depth.push(continue_depth);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_depth, continue_depth));
                    true
                } else {
                    false
                };

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::BrIf(0)); // loop back if truthy

                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end loop

                if label_pushed {
                    self.label_stack.pop();
                }
                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end block
            }
            Stmt::Labeled { label, body } => {
                // Set the pending label and compile the body statement.
                // The inner loop will consume it and register in label_stack.
                self.pending_label = Some(label.clone());
                self.emit_stmt(func, body, in_returning_func);
                // If the body wasn't a loop, the pending label is stale; drop it.
                self.pending_label = None;
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                // <init>
                // block $break
                //   loop $loop_top
                //     <condition>
                //     is_truthy ; i32.eqz ; br_if $break   (rel=1, loop is directly inside block)
                //     block $body_end                       ← continue targets this block's exit
                //       <body>                             ← continue: br(rel) exits $body_end
                //     end                                  ← fall through to update
                //     <update> ; drop
                //     br 0                                 ← restart $loop_top (rel=0)
                //   end
                // end
                //
                // Wrapping the body in $body_end ensures `continue` falls through to
                // the update expression before restarting, fixing the iterator-stuck
                // bug when `continue` fires inside an if/else chain (issue #137).
                if let Some(init_stmt) = init {
                    self.emit_stmt(func, init_stmt, in_returning_func);
                }

                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_d = self.block_depth;
                self.break_depth.push(break_d);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                // block_depth is now the loop's depth; Br(0) here restarts the loop.

                if let Some(cond) = condition {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, cond);
                    self.emit_memcall_i32(func, "is_truthy", 1);
                    func.instruction(&Instruction::I32Eqz);
                    // loop is directly inside block, so break is always 1 level up
                    func.instruction(&Instruction::BrIf(1));
                }

                // Inner block: continue targets this block's exit, then update runs.
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_d = self.block_depth;
                self.loop_depth.push(continue_d);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_d, continue_d));
                    true
                } else {
                    false
                };

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                // Close inner body block; continue lands here, then falls to update.
                if label_pushed {
                    self.label_stack.pop();
                }
                self.loop_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);

                if let Some(upd) = update {
                    self.emit_expr(func, upd);
                    if self.expr_has_value(upd) {
                        func.instruction(&Instruction::Drop);
                    }
                }

                // Restart loop (block_depth == loop's depth, so rel=0).
                func.instruction(&Instruction::Br(0));
                self.block_depth -= 1;
                func.instruction(&Instruction::End);

                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::Break => {
                // Branch out to the enclosing loop's break block. Must compute
                // relative depth from break_depth/block_depth — a hardcoded Br(1)
                // miscounts when the break is nested inside an if/switch/try
                // (each pushes block_depth without a matching break_depth), which
                // in the worst case turns `break` into `continue` and hangs.
                if let Some(&target) = self.break_depth.last() {
                    let rel = self.block_depth.saturating_sub(target);
                    func.instruction(&Instruction::Br(rel));
                } else {
                    func.instruction(&Instruction::Br(1));
                }
            }
            Stmt::Continue => {
                if let Some(&target) = self.loop_depth.last() {
                    let rel = self.block_depth.saturating_sub(target);
                    func.instruction(&Instruction::Br(rel));
                } else {
                    func.instruction(&Instruction::Br(0));
                }
            }
            Stmt::LabeledBreak(label) => {
                // Find the label in the stack and compute the relative depth.
                if let Some(&(_, break_d, _)) =
                    self.label_stack.iter().rev().find(|(l, _, _)| l == label)
                {
                    let rel = self.block_depth - break_d;
                    func.instruction(&Instruction::Br(rel));
                }
            }
            Stmt::LabeledContinue(label) => {
                if let Some(&(_, _, continue_d)) =
                    self.label_stack.iter().rev().find(|(l, _, _)| l == label)
                {
                    let rel = self.block_depth - continue_d;
                    func.instruction(&Instruction::Br(rel));
                }
            }
            Stmt::Throw(expr) => {
                // Set exception in bridge and return
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, expr);
                self.emit_memcall_void(func, "throw_value", 1);
                if in_returning_func {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::Return);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                // Bridge-based exception handling:
                // try_start(); <try body>; try_end();
                // if has_exception(): <bind catch param>; <catch body>
                // <finally body>
                self.emit_frame_begin(func, 0);
                self.emit_memcall_void(func, "try_start", 0);

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                self.emit_frame_begin(func, 0);
                self.emit_memcall_void(func, "try_end", 0);

                // Check for exception and execute catch block
                if let Some(catch_clause) = catch {
                    self.emit_frame_begin(func, 0);
                    self.emit_memcall_i32(func, "has_exception", 0);
                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                    self.block_depth += 1;

                    // Bind catch parameter
                    if let Some((param_id, _)) = &catch_clause.param {
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "get_exception", 0);
                        if let Some(&local_idx) = self.local_map.get(param_id) {
                            func.instruction(&Instruction::LocalSet(local_idx));
                        } else {
                            func.instruction(&Instruction::Drop);
                        }
                    } else {
                        // No param, just clear the exception
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "get_exception", 0);
                        func.instruction(&Instruction::Drop);
                    }

                    for s in &catch_clause.body {
                        self.emit_stmt(func, s, in_returning_func);
                    }

                    self.block_depth -= 1;
                    func.instruction(&Instruction::End);
                }

                // Finally block (unconditional)
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                // Compile switch as cascading if/else blocks
                // Strategy: store discriminant in a local-like pattern, compare each case
                // Since we can't easily allocate a local here, we use nested blocks + br_table approach
                // Simpler approach: nested if/else with js_strict_eq

                // Outer block for break
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.break_depth.push(self.block_depth);

                // We need to evaluate discriminant once. Without scratch locals,
                // we'll re-evaluate it for each case (works if it's a simple expression).
                // For complex discriminants, this could cause issues but handles most cases.

                let mut has_matched = false;
                for case in cases {
                    if let Some(test) = &case.test {
                        // case <test>:
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, discriminant);
                        self.emit_store_arg(func, 1, test);
                        self.emit_memcall_i32(func, "js_strict_eq", 2);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                        self.block_depth += 1;
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        self.block_depth -= 1;
                        func.instruction(&Instruction::End);
                    } else {
                        // default:
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        has_matched = true;
                    }
                }
                let _ = has_matched;

                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            // Issue #569: Wasm backend has no equivalent of LLVM's
            // alloca'd box slot — JS hoisting in the host runtime handles
            // forward refs natively. Emit nothing.
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    /// Check if an expression produces a value on the stack
    pub(super) fn expr_has_value(&self, expr: &Expr) -> bool {
        match expr {
            Expr::NativeMethodCall { module, method, .. } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                if normalized == "console" {
                    return false;
                }
                // void-returning array methods via NativeMethodCall
                if matches!(method.as_str(), "forEach") {
                    return false;
                }
                true
            }
            // console.log/warn/error via Call + PropertyGet pattern
            Expr::Call { callee, .. } => {
                if let Expr::PropertyGet {
                    object, property, ..
                } = callee.as_ref()
                {
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        if matches!(property.as_str(), "log" | "warn" | "error") {
                            return false;
                        }
                    }
                }
                true
            }
            // ArrayForEach returns undefined but we emit it explicitly
            _ => true,
        }
    }
}

/// Check if a statement or its children contain a return statement
pub(super) fn has_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(has_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(has_return))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().any(has_return),
        Stmt::For { init, body, .. } => {
            init.as_ref().is_some_and(|b| has_return(b.as_ref())) || body.iter().any(has_return)
        }
        Stmt::Labeled { body, .. } => has_return(body.as_ref()),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(has_return)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(has_return))
                || finally.as_ref().is_some_and(|f| f.iter().any(has_return))
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| c.body.iter().any(has_return)),
        _ => false,
    }
}
