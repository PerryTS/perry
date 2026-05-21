//! Phase-2 transformation: rewrite a producer function's body to use
//! the synthetic out-parameter in place of the local accumulator.

use super::*;

/// Phase 2 — rewrite a producer's body to use the new out-param.
/// Removes the `let out = []` line, replaces every `LocalGet(out)`
/// that survives the analyzer (push targets, array reads on the
/// accumulator) with `LocalGet(out_param)`, drops the trailing
/// `return out`, and rewrites recursive calls within the consume
/// pattern to pass the param through.
pub fn rewrite_producer_body(
    func: &mut Function,
    info: &ProducerInfo,
    out_param: LocalId,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
) {
    // 1. Insert the synthetic param at the END of the param list.
    func.params.push(perry_hir::Param {
        id: out_param,
        name: "__deforest_out".to_string(),
        ty: Type::Array(Box::new(info.elem_ty.clone())),
        default: None,
        decorators: Vec::new(),
        is_rest: false,
    });

    // 2. Drop the trailing `return out`.
    if matches!(func.body.last(), Some(Stmt::Return(_))) {
        func.body.pop();
    }

    // 3. Drop the leading `let out = []` (or any position where the
    //    out-local is bound).
    func.body
        .retain(|s| !matches!(s, Stmt::Let { id, .. } if *id == info.out_local_id));

    // 4. Substitute every reference to `out_local_id` with
    //    `out_param`. Same shape walk as the analyzer; this time we
    //    mutate.
    let mut subst = SubstituteLocal {
        from: info.out_local_id,
        to: out_param,
    };
    for s in &mut func.body {
        subst.visit_stmt(s);
    }

    // 5. Rewrite call sites inside the producer body — both
    //    consumer-pattern call sites (fuse into pass-through) and
    //    bare recursive calls (pass `out_param` through directly).
    let mut next_local = max_local_id_for_func(func) + 1;
    rewrite_call_sites_in_stmts_with_local_pass(
        &mut func.body,
        producers,
        out_param_ids,
        &mut next_local,
        Some(out_param),
    );
}

/// Mutating equivalent of `OutUsageAnalyzer`'s walker — substitutes
/// every `LocalGet(from)` and every `LocalSet(from, ...)` with `to`.
/// Doesn't touch other local references.
pub struct SubstituteLocal {
    pub from: LocalId,
    pub to: LocalId,
}

impl SubstituteLocal {
    pub fn visit_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &mut c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(t) = &mut c.test {
                        self.visit_expr(t);
                    }
                    for s in &mut c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            _ => {}
        }
    }

    pub fn visit_expr(&mut self, e: &mut Expr) {
        // Direct local-id field rewrites. These variants have a
        // `LocalId` (not `Box<Expr>`) field referencing the array
        // being mutated; the generic walker doesn't visit those, so
        // they need explicit handling here.
        match e {
            Expr::LocalGet(id) if *id == self.from => {
                *id = self.to;
                return;
            }
            Expr::LocalSet(id, val) if *id == self.from => {
                *id = self.to;
                self.visit_expr(val);
                return;
            }
            Expr::Update { id, .. } if *id == self.from => {
                *id = self.to;
                return;
            }
            Expr::ArrayPush { array_id, value } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(value);
                return;
            }
            Expr::ArrayPushSpread { array_id, source } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(source);
                return;
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                if *id == self.from {
                    *id = self.to;
                }
                return;
            }
            Expr::ArrayUnshift { array_id, value } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(value);
                return;
            }
            _ => {}
        }
        walk_expr_children_mut(e, &mut |child| self.visit_expr(child));
    }
}
