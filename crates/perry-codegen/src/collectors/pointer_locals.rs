use perry_hir::{infer_expr_type, BinaryOp, Expr, HirTypeFacts};
use perry_types::{FunctionType, Type};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
enum LocalWrite {
    Expr(Expr),
    NonPointer,
}

thread_local! {
    /// #6219 perf: cache each closure body's collected local-writes, keyed by
    /// the body slice's identity `(ptr, len)`, so a closure subtree is walked
    /// ONCE total instead of once per enclosing frame. Writes are keyed by
    /// GLOBAL local id, so the same cached set is correct for every ancestor
    /// frame AND the closure's own frame — the collection is compositional.
    ///
    /// Without this, emitting a shadow frame per closure/method (#6219) made
    /// each frame's `collect_pointer_typed_locals` re-descend the full nested
    /// closure subtree, so a body nested `D` deep was walked `O(D)` times —
    /// `O(nesting²)` overall, which never finished on deeply-nested bundles
    /// (the Next.js standalone server, thousands of closures 5–10 deep).
    ///
    /// Safe because within a single `perry compile` the HIR is alive for the
    /// whole (parallel) codegen — no body is freed, so no `(ptr, len)` is ever
    /// reused for a different body; and each closure body is reachable from
    /// exactly one enclosing tree, so its writes don't depend on the caller.
    static CLOSURE_WRITES_MEMO: std::cell::RefCell<
        HashMap<(usize, usize), std::rc::Rc<HashMap<u32, Vec<LocalWrite>>>>,
    > = std::cell::RefCell::new(HashMap::new());
}

const MAX_POINTER_ANALYSIS_TYPE_DEPTH: usize = 4;

fn pointer_analysis_type(ty: &Type) -> Type {
    pointer_analysis_type_inner(ty, 0)
}

fn pointer_analysis_type_inner(ty: &Type, depth: usize) -> Type {
    if depth >= MAX_POINTER_ANALYSIS_TYPE_DEPTH {
        return match ty {
            Type::Array(_) => Type::Array(Box::new(Type::Any)),
            Type::Tuple(_) => Type::Tuple(vec![Type::Any]),
            Type::Object(_) => Type::Object(Default::default()),
            Type::Function(ft) => Type::Function(FunctionType {
                params: Vec::new(),
                return_type: Box::new(Type::Any),
                is_async: ft.is_async,
                is_generator: ft.is_generator,
            }),
            Type::Union(_) => Type::Any,
            Type::Promise(_) => Type::Promise(Box::new(Type::Any)),
            Type::Generic { base, .. } => Type::Generic {
                base: base.clone(),
                type_args: Vec::new(),
            },
            other => other.clone(),
        };
    }

    match ty {
        Type::Array(elem) => {
            if depth + 1 >= MAX_POINTER_ANALYSIS_TYPE_DEPTH {
                Type::Array(Box::new(Type::Any))
            } else {
                Type::Array(Box::new(pointer_analysis_type_inner(elem, depth + 1)))
            }
        }
        Type::Tuple(elems) => Type::Tuple(
            elems
                .iter()
                .map(|elem| pointer_analysis_type_inner(elem, depth + 1))
                .collect(),
        ),
        Type::Object(_) => Type::Object(Default::default()),
        Type::Function(ft) => Type::Function(FunctionType {
            params: Vec::new(),
            return_type: Box::new(Type::Any),
            is_async: ft.is_async,
            is_generator: ft.is_generator,
        }),
        Type::Union(variants) => Type::Union(
            variants
                .iter()
                .map(|variant| pointer_analysis_type_inner(variant, depth + 1))
                .collect(),
        ),
        Type::Promise(inner) => {
            Type::Promise(Box::new(pointer_analysis_type_inner(inner, depth + 1)))
        }
        Type::Generic { base, type_args } => Type::Generic {
            base: base.clone(),
            type_args: type_args
                .iter()
                .map(|arg| pointer_analysis_type_inner(arg, depth + 1))
                .collect(),
        },
        other => other.clone(),
    }
}

fn pointer_analysis_array_type(elem: Type) -> Type {
    Type::Array(Box::new(pointer_analysis_type_inner(&elem, 1)))
}

struct PointerAnalysisFacts<'a> {
    local_types: &'a HashMap<u32, Type>,
    local_value_types: &'a HashMap<u32, Type>,
}

impl HirTypeFacts for PointerAnalysisFacts<'_> {
    fn local_type(&self, id: u32) -> Option<&Type> {
        self.local_value_types
            .get(&id)
            .or_else(|| self.local_types.get(&id))
    }

    fn global_type(&self, _id: u32) -> Option<&Type> {
        None
    }

    fn function_return_type(&self, _id: u32) -> Option<&Type> {
        None
    }
}

pub fn collect_pointer_typed_locals(
    params: &[perry_hir::Param],
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
) -> std::collections::HashMap<u32, u32> {
    use perry_hir::Stmt;
    fn is_ptr_typed(ty: &Type) -> bool {
        matches!(
            ty,
            Type::String
                | Type::Array(_)
                | Type::Tuple(_)
                | Type::Object(_)
                | Type::Named(_)
                | Type::Promise(_)
                | Type::Function(_)
                | Type::BigInt
                | Type::Any
                | Type::Unknown
        ) || matches!(ty, Type::Union(variants) if variants.iter().any(is_ptr_typed))
    }

    fn is_definitely_non_pointer_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Number
                | Type::Int32
                | Type::Boolean
                | Type::Null
                | Type::Void
                | Type::Never
                | Type::Symbol
        ) || matches!(ty, Type::Union(variants) if variants.iter().all(is_definitely_non_pointer_type))
    }

    fn expr_value_type(
        expr: &Expr,
        local_types: &HashMap<u32, Type>,
        local_value_types: &HashMap<u32, Type>,
        non_pointer_locals: &HashSet<u32>,
    ) -> Option<Type> {
        match expr {
            Expr::Undefined => Some(Type::Void),
            Expr::Null => Some(Type::Null),
            Expr::Bool(_) | Expr::Compare { .. } => Some(Type::Boolean),
            Expr::Number(_)
            | Expr::Integer(_)
            | Expr::Uint8ArrayLength(_)
            | Expr::Uint8ArrayGet { .. }
            | Expr::BufferLength(_)
            | Expr::BufferIndexGet { .. }
            | Expr::MathFloor(_)
            | Expr::MathCeil(_)
            | Expr::MathRound(_)
            | Expr::MathTrunc(_)
            | Expr::MathSign(_)
            | Expr::MathAbs(_)
            | Expr::MathSqrt(_)
            | Expr::MathLog(_)
            | Expr::MathLog2(_)
            | Expr::MathLog10(_)
            | Expr::MathPow(_, _)
            | Expr::MathMin(_)
            | Expr::MathMax(_)
            | Expr::MathImul(_, _)
            | Expr::MathRandom
            | Expr::MathSin(_)
            | Expr::MathCos(_)
            | Expr::MathTan(_)
            | Expr::MathAsin(_)
            | Expr::MathAcos(_)
            | Expr::MathAtan(_)
            | Expr::MathAtan2(_, _)
            | Expr::MathCbrt(_)
            | Expr::MathHypot(_)
            | Expr::MathFround(_)
            | Expr::MathF16round(_)
            | Expr::MathClz32(_)
            | Expr::MathExpm1(_)
            | Expr::MathLog1p(_)
            | Expr::MathSinh(_)
            | Expr::MathCosh(_)
            | Expr::MathTanh(_)
            | Expr::MathAsinh(_)
            | Expr::MathAcosh(_)
            | Expr::MathAtanh(_)
            | Expr::MathExp(_)
            | Expr::PerformanceNow => Some(Type::Number),
            Expr::String(_)
            | Expr::WtfString(_)
            | Expr::I18nString { .. }
            | Expr::TypeOf(_)
            | Expr::JsonStringify(_)
            | Expr::JsonStringifyPretty { .. }
            | Expr::JsonStringifyFull(..) => Some(Type::String),
            Expr::LocalGet(id) => local_value_types
                .get(id)
                .or_else(|| local_types.get(id))
                .map(pointer_analysis_type)
                .or_else(|| {
                    if non_pointer_locals.contains(id) {
                        Some(Type::Number)
                    } else {
                        None
                    }
                }),
            Expr::Unary { op, .. } => Some(match op {
                perry_hir::UnaryOp::Not => Type::Boolean,
                perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot => {
                    Type::Number
                }
            }),
            Expr::Binary { op, left, right } => {
                if matches!(op, BinaryOp::Add) {
                    if expr_is_known_non_pointer(
                        left,
                        local_types,
                        local_value_types,
                        non_pointer_locals,
                    ) && expr_is_known_non_pointer(
                        right,
                        local_types,
                        local_value_types,
                        non_pointer_locals,
                    ) {
                        Some(Type::Number)
                    } else {
                        None
                    }
                } else {
                    Some(Type::Number)
                }
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let then_ty = expr_value_type(
                    then_expr,
                    local_types,
                    local_value_types,
                    non_pointer_locals,
                )?;
                let else_ty = expr_value_type(
                    else_expr,
                    local_types,
                    local_value_types,
                    non_pointer_locals,
                )?;
                if then_ty == else_ty {
                    Some(then_ty)
                } else {
                    None
                }
            }
            Expr::Sequence(exprs) => exprs.last().and_then(|last| {
                expr_value_type(last, local_types, local_value_types, non_pointer_locals)
            }),
            Expr::Array(elements) => {
                let mut elem_ty: Option<Type> = None;
                for elem in elements {
                    let Some(ty) =
                        expr_value_type(elem, local_types, local_value_types, non_pointer_locals)
                    else {
                        return Some(Type::Array(Box::new(Type::Any)));
                    };
                    match &elem_ty {
                        None => elem_ty = Some(ty),
                        Some(existing) if existing == &ty => {}
                        Some(_) => return Some(pointer_analysis_array_type(Type::Any)),
                    }
                }
                Some(pointer_analysis_array_type(elem_ty.unwrap_or(Type::Any)))
            }
            Expr::IndexGet { object, .. } => {
                match expr_value_type(object, local_types, local_value_types, non_pointer_locals)? {
                    Type::Array(elem) => Some(*elem),
                    Type::String => Some(Type::String),
                    _ => None,
                }
            }
            Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(_)
            | Expr::BufferFrom { .. }
            | Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferConcat(_)
            | Expr::BufferConcatWithLength { .. }
            | Expr::Uint8ArrayNew(_)
            | Expr::Uint8ArrayFrom(_)
            | Expr::TextEncoderEncode(_) => Some(Type::Named("Uint8Array".into())),
            Expr::TextEncoderEncodeInto { .. } => Some(Type::Object(Default::default())),
            Expr::NativeMethodCall {
                module,
                method,
                object: None,
                ..
            } if module == "buffer" && method == "copyBytesFrom" => {
                Some(Type::Named("Uint8Array".into()))
            }
            Expr::Void(_) => Some(Type::Void),
            _ => {
                let facts = PointerAnalysisFacts {
                    local_types,
                    local_value_types,
                };
                match infer_expr_type(expr, &facts) {
                    Type::Any | Type::Unknown => None,
                    ty => Some(pointer_analysis_type(&ty)),
                }
            }
        }
    }

    fn expr_is_known_non_pointer(
        expr: &Expr,
        local_types: &HashMap<u32, Type>,
        local_value_types: &HashMap<u32, Type>,
        non_pointer_locals: &HashSet<u32>,
    ) -> bool {
        expr_value_type(expr, local_types, local_value_types, non_pointer_locals)
            .is_some_and(|ty| is_definitely_non_pointer_type(&ty))
    }

    fn collect_expr_writes_in_closure_stmts(
        stmts: &[Stmt],
        writes: &mut HashMap<u32, Vec<LocalWrite>>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { init, .. } => {
                    if let Some(init) = init {
                        collect_expr_writes(init, writes);
                    }
                }
                Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                    collect_expr_writes(expr, writes);
                }
                Stmt::Return(None)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    collect_expr_writes(condition, writes);
                    collect_expr_writes_in_closure_stmts(then_branch, writes);
                    if let Some(else_branch) = else_branch {
                        collect_expr_writes_in_closure_stmts(else_branch, writes);
                    }
                }
                Stmt::While { condition, body } => {
                    collect_expr_writes(condition, writes);
                    collect_expr_writes_in_closure_stmts(body, writes);
                }
                Stmt::DoWhile { body, condition } => {
                    collect_expr_writes_in_closure_stmts(body, writes);
                    collect_expr_writes(condition, writes);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        collect_expr_writes_in_closure_stmts(
                            std::slice::from_ref(init.as_ref()),
                            writes,
                        );
                    }
                    if let Some(condition) = condition {
                        collect_expr_writes(condition, writes);
                    }
                    if let Some(update) = update {
                        collect_expr_writes(update, writes);
                    }
                    collect_expr_writes_in_closure_stmts(body, writes);
                }
                Stmt::Labeled { body, .. } => {
                    collect_expr_writes_in_closure_stmts(
                        std::slice::from_ref(body.as_ref()),
                        writes,
                    );
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_expr_writes_in_closure_stmts(body, writes);
                    if let Some(catch) = catch {
                        collect_expr_writes_in_closure_stmts(&catch.body, writes);
                    }
                    if let Some(finally) = finally {
                        collect_expr_writes_in_closure_stmts(finally, writes);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    collect_expr_writes(discriminant, writes);
                    for case in cases {
                        if let Some(test) = &case.test {
                            collect_expr_writes(test, writes);
                        }
                        collect_expr_writes_in_closure_stmts(&case.body, writes);
                    }
                }
            }
        }
    }

    /// Shallow: ids DECLARED directly in `stmts` (`let` bindings), recursing
    /// through control-flow blocks but NOT into nested closures. A nested
    /// closure's locals belong to its own scope, and its own memo entry already
    /// excludes them, so we stop at the closure boundary.
    ///
    /// Used only to decide which of a closure's collected writes are "free"
    /// (i.e. target a captured outer local) and therefore worth propagating to
    /// ancestor frames. Approximation is safe in BOTH directions: HIR local ids
    /// are globally unique, so under-counting here at worst propagates a write
    /// to an id no ancestor declares (pruned harmlessly), and over-counting at
    /// worst makes an ancestor slightly more conservative (an extra safe root).
    fn collect_direct_let_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_direct_let_ids(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        collect_direct_let_ids(else_branch, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    collect_direct_let_ids(body, out);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        collect_direct_let_ids(std::slice::from_ref(init.as_ref()), out);
                    }
                    collect_direct_let_ids(body, out);
                }
                Stmt::Labeled { body, .. } => {
                    collect_direct_let_ids(std::slice::from_ref(body.as_ref()), out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_direct_let_ids(body, out);
                    if let Some(catch) = catch {
                        collect_direct_let_ids(&catch.body, out);
                    }
                    if let Some(finally) = finally {
                        collect_direct_let_ids(finally, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_direct_let_ids(&case.body, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_expr_writes(expr: &Expr, writes: &mut HashMap<u32, Vec<LocalWrite>>) {
        match expr {
            Expr::LocalSet(id, rhs) => {
                writes
                    .entry(*id)
                    .or_default()
                    .push(LocalWrite::Expr((**rhs).clone()));
                collect_expr_writes(rhs, writes);
            }
            Expr::Update { id, .. } => {
                writes.entry(*id).or_default().push(LocalWrite::NonPointer);
            }
            Expr::Closure { params, body, .. } => {
                for param in params {
                    if let Some(default) = &param.default {
                        collect_expr_writes(default, writes);
                    }
                }
                // #6219 perf: collect this closure body's writes at most once
                // (memoized by body identity) and merge them in, rather than
                // re-descending the subtree for every enclosing frame. The
                // recursive `collect_expr_writes_in_closure_stmts` below itself
                // routes deeper closures through this same arm, so the whole
                // subtree is computed once and every ancestor reuses the cache.
                //
                // The cached set holds only the closure's FREE writes — targets
                // NOT declared anywhere in this subtree (its params + direct
                // `let`s; deeper closures' locals are already excluded by their
                // own cached entries). Those are precisely the writes an ancestor
                // frame can care about (a captured outer local written from
                // inside the closure). A write to one of the closure's OWN locals
                // is resolved by that closure's own analysis, so propagating it up
                // was pure O(nesting²)-clone waste — the second half of the #6219
                // codegen blowup, after the redundant re-walk fixed by the memo.
                let key = (body.as_ptr() as usize, body.len());
                let cached = CLOSURE_WRITES_MEMO.with(|m| m.borrow().get(&key).cloned());
                let sub = match cached {
                    Some(rc) => rc,
                    None => {
                        let mut sub_writes: HashMap<u32, Vec<LocalWrite>> = HashMap::new();
                        collect_expr_writes_in_closure_stmts(body, &mut sub_writes);
                        let mut own_ids: HashSet<u32> = params.iter().map(|p| p.id).collect();
                        collect_direct_let_ids(body, &mut own_ids);
                        sub_writes.retain(|id, _| !own_ids.contains(id));
                        let rc = std::rc::Rc::new(sub_writes);
                        CLOSURE_WRITES_MEMO.with(|m| m.borrow_mut().insert(key, rc.clone()));
                        rc
                    }
                };
                for (id, ws) in sub.iter() {
                    writes.entry(*id).or_default().extend(ws.iter().cloned());
                }
            }
            _ => {
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    collect_expr_writes(child, writes)
                });
            }
        }
    }

    fn collect_facts(
        stmts: &[Stmt],
        local_types: &mut HashMap<u32, Type>,
        writes: &mut HashMap<u32, Vec<LocalWrite>>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, ty, init, .. } => {
                    local_types.insert(*id, ty.clone());
                    if let Some(init) = init {
                        writes
                            .entry(*id)
                            .or_default()
                            .push(LocalWrite::Expr(init.clone()));
                        collect_expr_writes(init, writes);
                    } else {
                        writes.entry(*id).or_default().push(LocalWrite::NonPointer);
                    }
                }
                Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                    collect_expr_writes(expr, writes);
                }
                Stmt::Return(None)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    collect_expr_writes(condition, writes);
                    collect_facts(then_branch, local_types, writes);
                    if let Some(else_branch) = else_branch {
                        collect_facts(else_branch, local_types, writes);
                    }
                }
                Stmt::While { condition, body } => {
                    collect_expr_writes(condition, writes);
                    collect_facts(body, local_types, writes);
                }
                Stmt::DoWhile { body, condition } => {
                    collect_facts(body, local_types, writes);
                    collect_expr_writes(condition, writes);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        collect_facts(std::slice::from_ref(init.as_ref()), local_types, writes);
                    }
                    if let Some(condition) = condition {
                        collect_expr_writes(condition, writes);
                    }
                    if let Some(update) = update {
                        collect_expr_writes(update, writes);
                    }
                    collect_facts(body, local_types, writes);
                }
                Stmt::Labeled { body, .. } => {
                    collect_facts(std::slice::from_ref(body.as_ref()), local_types, writes);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_facts(body, local_types, writes);
                    if let Some(catch) = catch {
                        if let Some((id, _)) = &catch.param {
                            local_types.insert(*id, Type::Any);
                        }
                        collect_facts(&catch.body, local_types, writes);
                    }
                    if let Some(finally) = finally {
                        collect_facts(finally, local_types, writes);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    collect_expr_writes(discriminant, writes);
                    for case in cases {
                        if let Some(test) = &case.test {
                            collect_expr_writes(test, writes);
                        }
                        collect_facts(&case.body, local_types, writes);
                    }
                }
            }
        }
    }

    let mut local_types: HashMap<u32, Type> = HashMap::new();
    let mut writes: HashMap<u32, Vec<LocalWrite>> = HashMap::new();
    let mut flat_row_alias_ids: HashSet<u32> = HashSet::new();
    for p in params {
        local_types.insert(p.id, pointer_analysis_type(&p.ty));
    }
    collect_facts(stmts, &mut local_types, &mut writes);
    super::integer_locals::collect_flat_row_aliases(stmts, flat_const_ids, &mut flat_row_alias_ids);
    // #6219 perf: the type-inference fixpoint below infers each frame local's
    // type from its writes. Writes to ids NOT declared in THIS frame (a nested
    // closure's own locals, pulled in while walking the subtree for captured-
    // local writes) are resolved by that closure's OWN analysis, so processing
    // them here is pure redundant work — O(nesting) per closure frame, the
    // second half of the #6219 codegen blowup (the first being the walk itself,
    // fixed by CLOSURE_WRITES_MEMO). Prune to this frame's locals (params +
    // `let`s, all present in `local_types`). This is at worst OVER-inclusive —
    // a frame local whose write references a pruned id sees that id as unknown
    // and conservatively keeps its slot — so it never DROPS a shadow slot: no
    // under-rooting / use-after-free risk, only (rarely) one extra safe root.
    writes.retain(|id, _| local_types.contains_key(id));

    let mut local_value_types: HashMap<u32, Type> = local_types
        .iter()
        .filter_map(|(id, ty)| {
            if !matches!(ty, Type::Any | Type::Unknown) {
                Some((*id, pointer_analysis_type(ty)))
            } else {
                None
            }
        })
        .collect();
    let mut non_pointer_locals: HashSet<u32> = local_types
        .iter()
        .filter_map(|(id, ty)| {
            if is_definitely_non_pointer_type(ty) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();

    // #6219 perf: BOUND the refinement fixpoint.
    //
    // This loop exists only to PROVE additional locals non-pointer (it ONLY
    // ever GROWS `non_pointer_locals`, below), which lets `walk`/param slots
    // DROP them from the shadow frame. Every slot decision gates on
    // `is_ptr_typed(declared_ty) && !non_pointer_locals.contains(id)`, so
    // curtailing this loop is strictly conservative: a local that would have
    // been proven non-pointer instead keeps a safe extra root. It never removes
    // a needed slot — no under-rooting / use-after-free is possible.
    //
    // Unbounded (`while changed`) the loop is O(locals × iterations): a long
    // def-use chain (`a = b; b = c; …`) needs one pass per link. Next.js/webpack
    // emit whole chunks as a single closure with thousands of locals — pre-#6219
    // closure bodies were never shadow-analyzed, so this cost is new, and
    // uncapped (with SipHash-keyed lookups) it never converges: 25 min+ on the
    // standalone server, still short of LLVM emission. A fixed iteration cap
    // bounds it while keeping full precision for normal frames (which converge
    // in a handful of passes); a hard size gate skips refinement entirely on the
    // pathologically huge frames where even a few passes are wasted work.
    const MAX_FIXPOINT_ITERS: usize = 16;
    const MAX_FIXPOINT_LOCALS: usize = 8192;
    let mut iters = 0usize;
    let mut changed = writes.len() <= MAX_FIXPOINT_LOCALS;
    while changed && iters < MAX_FIXPOINT_ITERS {
        iters += 1;
        changed = false;
        for (id, local_writes) in &writes {
            let mut inferred_ty: Option<Type> = None;
            let mut precise_inference = true;
            let mut all_non_pointer = !local_writes.is_empty();
            for write in local_writes {
                let write_ty = match write {
                    LocalWrite::NonPointer => Some(Type::Number),
                    LocalWrite::Expr(expr) => {
                        expr_value_type(expr, &local_types, &local_value_types, &non_pointer_locals)
                            .map(|ty| pointer_analysis_type(&ty))
                    }
                };
                match write_ty {
                    Some(Type::Any | Type::Unknown) => {
                        all_non_pointer = false;
                        inferred_ty = None;
                        precise_inference = false;
                    }
                    Some(ty) => {
                        all_non_pointer &=
                            is_definitely_non_pointer_type(&ty) || non_pointer_locals.contains(id);
                        if precise_inference {
                            match &inferred_ty {
                                None => inferred_ty = Some(ty),
                                Some(existing) if existing == &ty => {}
                                Some(_) => {
                                    inferred_ty = None;
                                    precise_inference = false;
                                }
                            }
                        }
                    }
                    None => {
                        all_non_pointer = false;
                        inferred_ty = None;
                        precise_inference = false;
                    }
                }
            }
            if matches!(local_types.get(id), Some(Type::Any | Type::Unknown)) {
                if precise_inference {
                    if let Some(ty) = inferred_ty {
                        if local_value_types.get(id) != Some(&ty) {
                            local_value_types.insert(*id, ty);
                            changed = true;
                        }
                    } else if local_value_types.remove(id).is_some() {
                        changed = true;
                    }
                } else if local_value_types.remove(id).is_some() {
                    changed = true;
                }
            }
            if all_non_pointer && non_pointer_locals.insert(*id) {
                changed = true;
            }
        }
    }

    let mut out = std::collections::HashMap::new();
    let mut next_slot: u32 = 0;
    for p in params {
        if is_ptr_typed(&p.ty) && !non_pointer_locals.contains(&p.id) {
            out.insert(p.id, next_slot);
            next_slot += 1;
        }
    }
    fn walk(
        stmts: &[Stmt],
        out: &mut std::collections::HashMap<u32, u32>,
        next_slot: &mut u32,
        non_pointer_locals: &HashSet<u32>,
        flat_row_alias_ids: &HashSet<u32>,
    ) {
        for s in stmts {
            match s {
                Stmt::Let { id, ty, .. }
                    if is_ptr_typed(ty)
                        && !non_pointer_locals.contains(id)
                        && !flat_row_alias_ids.contains(id) =>
                {
                    out.insert(*id, *next_slot);
                    *next_slot += 1;
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(
                        then_branch,
                        out,
                        next_slot,
                        non_pointer_locals,
                        flat_row_alias_ids,
                    );
                    if let Some(eb) = else_branch {
                        walk(eb, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        walk(
                            std::slice::from_ref(i.as_ref()),
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    if let Some(c) = catch {
                        if let Some((id, _)) = &c.param {
                            // Catch parameter is implicitly bound;
                            // treat as Any (pointer-possible).
                            if !non_pointer_locals.contains(id) {
                                out.insert(*id, *next_slot);
                                *next_slot += 1;
                            }
                        }
                        walk(
                            &c.body,
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                    if let Some(fb) = finally {
                        walk(fb, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(
                            &c.body,
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                }
                Stmt::Labeled { body, .. } => walk(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    next_slot,
                    non_pointer_locals,
                    flat_row_alias_ids,
                ),
                _ => {}
            }
        }
    }
    walk(
        stmts,
        &mut out,
        &mut next_slot,
        &non_pointer_locals,
        &flat_row_alias_ids,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{Function, Param, Stmt};

    fn return_array_of_type(depth: usize, leaf: Type) -> Type {
        (0..depth).fold(leaf, |ty, _| Type::Array(Box::new(ty)))
    }

    #[test]
    fn pointer_analysis_caps_deep_array_types() {
        let ty = return_array_of_type(64, Type::Number);
        let normalized = pointer_analysis_type(&ty);
        let mut depth = 0;
        let mut current = &normalized;
        while let Type::Array(inner) = current {
            depth += 1;
            current = inner;
        }
        assert!(depth <= MAX_POINTER_ANALYSIS_TYPE_DEPTH);
        assert!(matches!(current, Type::Any));
    }

    #[test]
    fn recursive_array_map_shape_collects_without_deep_type_growth() {
        let patch_param = Param {
            id: 1,
            name: "patch".to_string(),
            ty: return_array_of_type(64, Type::Any),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        };
        let callback_param = Param {
            id: 2,
            name: "p".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        };
        let callback = Expr::Closure {
            func_id: 2,
            params: vec![callback_param],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![Expr::LocalGet(2)],
                type_args: Vec::new(),
                byte_offset: 0,
            }))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        let function = Function {
            id: 1,
            name: "unixToWin".to_string(),
            type_params: Vec::new(),
            params: vec![patch_param],
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 3,
                    name: "mapped".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::ArrayMap {
                        array: Box::new(Expr::LocalGet(1)),
                        callback: Box::new(callback),
                    }),
                },
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        };

        let slots = collect_pointer_typed_locals(&function.params, &function.body, &HashSet::new());
        assert!(slots.contains_key(&1));
        assert!(slots.contains_key(&3));
    }

    #[test]
    fn pointer_analysis_reuses_shared_hir_scalar_facts() {
        let stmts = vec![
            Stmt::Let {
                id: 1,
                name: "pid".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::ProcessPid),
            },
            Stmt::Let {
                id: 2,
                name: "date".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::DateNew(vec![])),
            },
        ];

        let slots = collect_pointer_typed_locals(&[], &stmts, &HashSet::new());
        assert!(!slots.contains_key(&1));
        assert!(slots.contains_key(&2));
    }
}
