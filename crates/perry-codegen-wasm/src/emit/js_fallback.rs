//! JavaScript-fallback emission extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move of `WasmModuleEmitter::{emit_js_async_function, emit_js_stmt,
//! emit_js_expr}` onto a dedicated inherent `impl WasmModuleEmitter` block.
//! Used when a construct cannot be represented directly in WASM.

use super::*;

impl WasmModuleEmitter {
    /// Generate JavaScript code for an async function body.
    /// The generated function uses NaN-boxed f64 values and bridge helper functions.
    pub(super) fn emit_js_async_function(&self, func: &perry_hir::ir::Function) -> String {
        let params: Vec<String> = func
            .params
            .iter()
            .enumerate()
            .map(|(i, _)| format!("__p{}", i))
            .collect();
        let params_str = params.join(", ");

        self.js_preallocated.borrow_mut().clear();
        let mut body = String::new();
        let scoped_names = scoped_js::scoped_js_names(&func.body);
        if !scoped_names.is_empty() {
            body.push_str(&format!(
                "    let {};\n",
                scoped_names.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        // Map param names to local IDs for the JS emitter
        let mut local_names: BTreeMap<u32, String> = BTreeMap::new();
        for (i, param) in func.params.iter().enumerate() {
            local_names.insert(param.id, format!("__p{}", i));
            if self.js_boxed(param.id) {
                body.push_str(&format!("    __p{i} = {{ value: toJsValue(__p{i}) }};\n"));
            }
        }

        for stmt in &func.body {
            self.emit_js_stmt(&mut body, stmt, &mut local_names, 2);
        }

        format!(
            "  __async_{name}: ({params}) => {{\n\
             \x20   const __p = (async () => {{\n\
             {body}\
             \x20     return {undef};\n\
             \x20   }})();\n\
             \x20   return nanboxPointer(allocHandle(__p));\n\
             \x20 }},",
            name = func.name,
            params = params_str,
            body = body,
            undef = "u64ToF64(TAG_UNDEFINED)",
        )
    }

    pub(super) fn emit_js_stmt(
        &self,
        out: &mut String,
        stmt: &Stmt,
        locals: &mut BTreeMap<u32, String>,
        indent: usize,
    ) {
        self.emit_js_labeled_stmt(out, stmt, locals, indent, "");
    }

    /// `label` is `"name: "` on a loop whose label a `continue` may name; JS accepts
    /// `continue name` only when `name` labels the iteration statement itself.
    fn emit_js_labeled_stmt(
        &self,
        out: &mut String,
        stmt: &Stmt,
        locals: &mut BTreeMap<u32, String>,
        indent: usize,
        label: &str,
    ) {
        let pad = "  ".repeat(indent);
        match stmt {
            // A HIR local lives for the whole function, as a WASM local does: the
            // lowering of `try`/`finally` and `for...of` reads a local from the `catch` or
            // `finally` beside the block that declared it. `var` gives the JS binding that
            // scope; a block-scoped `let` is undeclared there.
            Stmt::Let { id, init, .. } => {
                let name = format!("__l{}", id);
                locals.insert(*id, name.clone());
                let val = match init {
                    Some(init_expr) => self.emit_js_expr(init_expr, locals),
                    None => "u64ToF64(TAG_UNDEFINED)".to_string(),
                };
                if self.js_preallocated.borrow().contains(id) {
                    out.push_str(&format!("{pad}    {name}.value = toJsValue({val});\n"));
                } else if self.js_boxed(*id) {
                    out.push_str(&format!(
                        "{pad}    var {name} = {{ value: toJsValue({val}) }};\n"
                    ));
                } else {
                    out.push_str(&format!("{pad}    var {name} = {val};\n"));
                }
            }
            Stmt::Expr(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    {val};\n"));
            }
            Stmt::Return(Some(e)) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    return {val};\n"));
            }
            Stmt::Return(None) => {
                out.push_str(&format!("{pad}    return u64ToF64(TAG_UNDEFINED);\n"));
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    if (toJsValue({cond})) {{\n"));
                for s in then_branch {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(eb) = else_branch {
                    out.push_str(&format!("{pad}    }} else {{\n"));
                    for s in eb {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::While { condition, body } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    {label}while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                out.push_str(&format!("{pad}    {{\n"));
                if let Some(init_stmt) = init {
                    self.emit_js_stmt(out, init_stmt, locals, indent + 1);
                }
                let cond = condition
                    .as_ref()
                    .map(|c| self.emit_js_expr(c, locals))
                    .unwrap_or_else(|| "1".to_string());
                // A JS `for` header, so a `continue` still runs the update.
                let upd = update
                    .as_ref()
                    .map(|u| self.emit_js_expr(u, locals))
                    .unwrap_or_default();
                let release = |threw| {
                    condition
                        .as_ref()
                        .and_then(|c| super::stmt::collection_cursor_release(c, threw))
                };
                let release =
                    release(true)
                        .zip(release(false))
                        .map(|((_, on_throw), (_, on_exit))| {
                            (
                                self.emit_js_expr(&on_throw, locals),
                                self.emit_js_expr(&on_exit, locals),
                            )
                        });
                if release.is_some() {
                    out.push_str(&format!("{pad}      try {{\n"));
                }
                out.push_str(&format!(
                    "{pad}      {label}for (; toJsValue({cond}); {upd}) {{\n"
                ));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 2);
                }
                out.push_str(&format!("{pad}      }}\n"));
                if let Some((on_throw, on_exit)) = release {
                    out.push_str(&format!(
                        "{pad}      }} catch (__e) {{ {on_throw}; throw __e; }} finally {{ {on_exit}; }}\n"
                    ));
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                out.push_str(&format!("{pad}    try {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(c) = catch {
                    if let Some((param_id, _)) = &c.param {
                        let name = format!("__l{}", param_id);
                        locals.insert(*param_id, name.clone());
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                        let value = if self.js_boxed(*param_id) {
                            "{ value: __e }"
                        } else {
                            "fromJsValue(__e)"
                        };
                        out.push_str(&format!("{pad}      let {name} = {value};\n"));
                    } else {
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                    }
                    for s in &c.body {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                if let Some(f) = finally {
                    out.push_str(&format!("{pad}    }} finally {{\n"));
                    for s in f {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Throw(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    throw toJsValue({val});\n"));
            }
            Stmt::Break => {
                out.push_str(&format!("{pad}    break;\n"));
            }
            Stmt::Continue => {
                out.push_str(&format!("{pad}    continue;\n"));
            }
            Stmt::LabeledBreak(label) => {
                out.push_str(&format!("{pad}    break {};\n", label));
            }
            Stmt::LabeledContinue(label) => {
                out.push_str(&format!("{pad}    continue {};\n", label));
            }
            Stmt::DoWhile { body, condition } => {
                out.push_str(&format!("{pad}    {label}do {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    }} while (toJsValue({cond}));\n"));
            }
            Stmt::Labeled { label: name, body } if labels_a_loop(body) => {
                self.emit_js_labeled_stmt(out, body, locals, indent, &format!("{label}{name}: "));
            }
            Stmt::Labeled { label, body } => {
                out.push_str(&format!("{pad}    {}: {{\n", label));
                self.emit_js_stmt(out, body, locals, indent + 1);
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                let disc = self.emit_js_expr(discriminant, locals);
                out.push_str(&format!("{pad}    switch (toJsValue({disc})) {{\n"));
                for case in cases {
                    if let Some(test) = &case.test {
                        let t = self.emit_js_expr(test, locals);
                        out.push_str(&format!("{pad}      case toJsValue({t}):\n"));
                    } else {
                        out.push_str(&format!("{pad}      default:\n"));
                    }
                    for s in &case.body {
                        self.emit_js_stmt(out, s, locals, indent + 2);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            // A closure created before its captured binding's `Let` shares the cell made
            // here, as in the WASM arm; a TDZ cell starts as `undefined` there too.
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                for id in ids.iter().filter(|id| self.js_boxed(**id)) {
                    let name = format!("__l{}", id);
                    locals.insert(*id, name.clone());
                    out.push_str(&format!("{pad}    var {name} = {{ value: undefined }};\n"));
                    self.js_preallocated.borrow_mut().insert(*id);
                }
            }
            Stmt::ReleaseBoxes(_) => {}
        }
    }

    pub(super) fn emit_js_expr(&self, expr: &Expr, locals: &BTreeMap<u32, String>) -> String {
        match expr {
            // Rust spells an infinity `inf`, which is not a JS literal.
            Expr::Number(n) if n.is_infinite() => {
                if *n > 0.0 { "Infinity" } else { "(-Infinity)" }.to_string()
            }
            Expr::Number(n) => format!("{}", n),
            Expr::Integer(i) => format!("{}", *i as f64),
            Expr::Bool(true) => "u64ToF64(TAG_TRUE)".to_string(),
            Expr::Bool(false) => "u64ToF64(TAG_FALSE)".to_string(),
            Expr::Undefined => "u64ToF64(TAG_UNDEFINED)".to_string(),
            Expr::Null => "u64ToF64(TAG_NULL)".to_string(),
            Expr::String(s) => format!("fromJsValue({})", js_string_literal(s)),
            Expr::ScopedTemp { id, value, body } => {
                let prefix = format!("__scoped_v_{id}_");
                let depth = locals
                    .get(id)
                    .and_then(|name| name.strip_prefix(&prefix))
                    .and_then(|depth| depth.parse::<u32>().ok())
                    .map_or(0, |depth| depth + 1);
                let value_name = format!("{prefix}{depth}");
                let result_name = format!("__scoped_r_{id}_{depth}");
                let init = self.emit_js_expr(value, locals);
                let mut inner = locals.clone();
                inner.insert(*id, value_name.clone());
                let body = self.emit_js_expr(body, &inner);
                format!("({value_name} = {init}, {result_name} = {body}, {value_name} = undefined, {result_name})")
            }
            // A local the body does not bind is a module-level `let`, promoted to an
            // exported WASM global (`module_let_globals`); no JS binding exists for it.
            Expr::LocalGet(id) => match locals.get(id) {
                Some(name) if self.js_boxed(*id) => format!("fromJsValue({name}.value)"),
                Some(name) => name.clone(),
                None => match self.module_let_globals.get(&(self.current_mod_idx, *id)) {
                    Some(&gidx) => {
                        format!(
                            "u64ToF64(wasmInstance.exports.__wasm_global_{}.value)",
                            gidx
                        )
                    }
                    None => format!("__l{}", id),
                },
            },
            Expr::LocalSet(id, val) => {
                let v = self.emit_js_expr(val, locals);
                self.emit_js_local_set(*id, &v, locals)
            }
            // `x++` / `--x`: the local is read once, as a number; a postfix update yields
            // the value it read.
            Expr::Update { id, op, prefix } => {
                let old = self.emit_js_expr(&Expr::LocalGet(*id), locals);
                let sign = match op {
                    UpdateOp::Increment => "+",
                    UpdateOp::Decrement => "-",
                };
                let set = self.emit_js_local_set(*id, &format!("(__u {sign} 1)"), locals);
                let result = if *prefix {
                    set
                } else {
                    format!("({set}, __u)")
                };
                format!("((__u) => {result})(+toJsValue({old}))")
            }
            // An id with no WASM global is the HIR's unresolved-global sentinel (the receiver
            // of `Promise.resolve`, `fetch`, `JSON.parse`); in JS that receiver is globalThis.
            // The WASM arm reads the same id as undefined, but here a bare `__g{id}` would be
            // an undeclared name and throw ReferenceError on entry to the async body.
            Expr::GlobalGet(id) if !self.global_map.contains_key(id) => {
                "globalThisValue()".to_string()
            }
            Expr::GlobalThisExpr => "globalThisValue()".to_string(),
            // A declared global lives in its WASM global, which the module exports by
            // index; no JS binding `__g{id}` exists.
            Expr::GlobalGet(id) => format!(
                "u64ToF64(wasmInstance.exports.__wasm_global_{}.value)",
                self.global_map[id]
            ),
            Expr::GlobalSet(id, val) if self.global_map.contains_key(id) => {
                let v = self.emit_js_expr(val, locals);
                format!(
                    "((__v) => (wasmInstance.exports.__wasm_global_{}.value = f64ToU64(__v), __v))({})",
                    self.global_map[id], v
                )
            }
            Expr::GlobalSet(_, val) => self.emit_js_expr(val, locals),
            Expr::Binary { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                // The operator runs on the decoded JS values, which applies ToNumber (or
                // string concatenation for `+`) as the sync path does.
                let js_op = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Pow => "**",
                    BinaryOp::BitAnd => "&",
                    BinaryOp::BitOr => "|",
                    BinaryOp::BitXor => "^",
                    BinaryOp::Shl => "<<",
                    BinaryOp::Shr => ">>",
                    BinaryOp::UShr => ">>>",
                };
                format!("fromJsValue(toJsValue({l}) {js_op} toJsValue({r}))")
            }
            // The same bridge as the WASM arm, so `in` agrees between sync and async bodies.
            Expr::In { property, object } => {
                let p = self.emit_js_expr(property, locals);
                let o = self.emit_js_expr(object, locals);
                // The property is evaluated before the object; both stay outside the arrow
                // so an `await` in either remains inside the async body.
                format!(
                    "((__p, __o) => __memDispatch.object_has_property(toJsValue(__o), toJsValue(__p)) ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))({p}, {o})"
                )
            }
            Expr::Compare { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                let js_op = match op {
                    CompareOp::Eq => "===",
                    CompareOp::Ne => "!==",
                    CompareOp::LooseEq => "==",
                    CompareOp::LooseNe => "!=",
                    CompareOp::Lt => "<",
                    CompareOp::Le => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Ge => ">=",
                };
                format!(
                    "(toJsValue({}) {} toJsValue({}) ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))",
                    l, js_op, r
                )
            }
            Expr::Logical { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                // The left operand runs once: the arrow yields `[left]` when it is the
                // result, else null. The right operand stays outside the arrow so an
                // `await` in it remains inside the async body.
                let keep_left = match op {
                    LogicalOp::And => "!toJsValue(__t)",
                    LogicalOp::Or => "toJsValue(__t)",
                    LogicalOp::Coalesce => "!(isNull(__t) || isUndefined(__t))",
                };
                format!("((((__t) => ({keep_left} ? [__t] : null))({l}) ?? [{r}])[0])")
            }
            Expr::Unary { op, operand } => {
                let o = self.emit_js_expr(operand, locals);
                match op {
                    UnaryOp::Neg => format!("(-{})", o),
                    UnaryOp::Not => format!(
                        "(toJsValue({}) ? u64ToF64(TAG_FALSE) : u64ToF64(TAG_TRUE))",
                        o
                    ),
                    UnaryOp::BitNot => format!("fromJsValue(~toJsValue({}))", o),
                    UnaryOp::Pos => format!("fromJsValue(+toJsValue({}))", o),
                }
            }
            Expr::Await(inner) => {
                let v = self.emit_js_expr(inner, locals);
                // In JS async context, we can truly await
                format!("fromJsValue(await toJsValue({}))", v)
            }
            Expr::Call { callee, args, .. } => {
                let args_js: Vec<String> =
                    args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                let call_direct = |func_idx: u32| -> String {
                    // An async function is its JS implementation here: WASM reaches it
                    // through an import, which is not exported.
                    if let Some((name, _, param_count)) = self
                        .async_func_imports
                        .iter()
                        .find(|(_, idx, _)| *idx == func_idx)
                    {
                        let mut args: Vec<String> = self
                            .with_rest_packed(Some(func_idx), args)
                            .iter()
                            .map(|a| self.emit_js_expr(a, locals))
                            .collect();
                        args.resize(
                            args.len().max(*param_count),
                            "u64ToF64(TAG_UNDEFINED)".to_string(),
                        );
                        return format!("__asyncFuncImpls.__async_{name}({})", args.join(", "));
                    }
                    // Call exported WASM function. WASM funcs use i64 params/result; one
                    // with no result returns `undefined`. The callback boundary packs a
                    // rest parameter's arguments, pads missing ones and rethrows an
                    // exception left pending under an enclosing WASM `try`.
                    let args_i64: Vec<String> =
                        args_js.iter().map(|a| format!("f64ToU64({})", a)).collect();
                    format!(
                        "u64ToF64(__invokeWasmCallback({}) ?? TAG_UNDEFINED)",
                        std::iter::once(format!("wasmInstance.exports.__wasm_func_{func_idx}"))
                            .chain(args_i64)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&func_idx) = self.func_map.get(id) {
                            call_direct(func_idx)
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::ExternFuncRef { name, .. }
                        if name == perry_hir::tdz_check::TDZ_CHECK_THEN =>
                    {
                        // #11826: a module-level TDZ `check_then` yields its value.
                        args_js
                            .get(2)
                            .cloned()
                            .unwrap_or_else(|| "u64ToF64(TAG_UNDEFINED)".to_string())
                    }
                    // The Set/Map `for...of` cursor calls the bridges the WASM arm calls.
                    Expr::ExternFuncRef { name, .. }
                        if matches!(
                            name.as_str(),
                            "js_set_cursor_next"
                                | "js_map_cursor_next"
                                | "js_set_compaction_epoch"
                                | "js_map_compaction_epoch"
                                | "js_collection_cursor_release"
                        ) =>
                    {
                        let bridge = if name.ends_with("cursor_next") {
                            "collection_cursor_next"
                        } else if name.ends_with("cursor_release") {
                            "collection_cursor_release"
                        } else {
                            "collection_cursor_epoch"
                        };
                        format!(
                            "fromJsValue(__memDispatch.{}({}))",
                            bridge,
                            host_args(&args_js)
                        )
                    }
                    Expr::ExternFuncRef { name, .. } => {
                        if let Some(&func_idx) = self.func_name_map.get(name) {
                            call_direct(func_idx)
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    // `console.log(x)` lowers to `GlobalGet(0).log(x)`: a known global
                    // in member position is the bare sentinel and codegen routes by the
                    // method name, as the WASM arm does. Without this the call reads
                    // `globalThis.log`, which is not a function.
                    Expr::PropertyGet {
                        object, property, ..
                    } if matches!(object.as_ref(), Expr::GlobalGet(id) if !self.global_map.contains_key(id))
                        && matches!(property.as_str(), "log" | "warn" | "error") =>
                    {
                        format!(
                            "(console.{}({}), u64ToF64(TAG_UNDEFINED))",
                            property,
                            host_args(&args_js)
                        )
                    }
                    Expr::PropertyGet {
                        object, property, ..
                    } => {
                        let obj = self.emit_js_expr(object, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke(__jsMethodGet(toJsValue({}), {}), [{}]))",
                            obj,
                            js_string_literal(property),
                            host_args(&args_js)
                        )
                    }
                    Expr::IndexGet { object, index, .. } => {
                        let obj = self.emit_js_expr(object, locals);
                        let key = self.emit_js_expr(index, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke(__jsMethodGet(toJsValue({}), toJsValue({})), [{}]))",
                            obj,
                            key,
                            host_args(&args_js)
                        )
                    }
                    // A NaN-boxed callee: `__jsMethodInvoke` runs a Perry closure or a host function.
                    _ => {
                        let callee_js = self.emit_js_expr(callee, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke({{ r: undefined, m: 'value', f: toJsValue({}) }}, [{}]))",
                            callee_js,
                            host_args(&args_js)
                        )
                    }
                }
            }
            // `f(a, ...xs)` / `o.m(a, ...xs)`: a JS spread over the host values, then the
            // same method lookup and invocation as `Expr::Call`.
            Expr::CallSpread { callee, args, .. } => {
                let list = args
                    .iter()
                    .map(|arg| match arg {
                        CallArg::Expr(e) => format!("toJsValue({})", self.emit_js_expr(e, locals)),
                        CallArg::Spread(e) => {
                            format!("...toJsValue({})", self.emit_js_expr(e, locals))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let direct = match callee.as_ref() {
                    Expr::FuncRef(id) => self.func_map.get(id).copied(),
                    Expr::ExternFuncRef { name, .. } => self.func_name_map.get(name).copied(),
                    _ => None,
                };
                // A compiled function takes NaN-boxed i64 arguments, one per parameter;
                // a void export returns `undefined`.
                if let Some(func_idx) = direct {
                    return format!(
                        "((__a) => u64ToF64(__invokeWasmCallback(wasmInstance.exports.__wasm_func_{func_idx}, ...__a.map((__v) => f64ToU64(fromJsValue(__v)))) ?? TAG_UNDEFINED))([{list}])"
                    );
                }
                match callee.as_ref() {
                    Expr::PropertyGet {
                        object, property, ..
                    } if matches!(object.as_ref(), Expr::GlobalGet(id) if !self.global_map.contains_key(id))
                        && matches!(
                            property.as_str(),
                            "log" | "info" | "warn" | "error" | "debug"
                        ) =>
                    {
                        format!("(console.{property}({list}), u64ToF64(TAG_UNDEFINED))")
                    }
                    Expr::PropertyGet {
                        object, property, ..
                    } => {
                        let obj = self.emit_js_expr(object, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke(__jsMethodGet(toJsValue({}), {}), [{}]))",
                            obj,
                            js_string_literal(property),
                            list
                        )
                    }
                    Expr::IndexGet { object, index, .. } => {
                        let obj = self.emit_js_expr(object, locals);
                        let key = self.emit_js_expr(index, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke(__jsMethodGet(toJsValue({obj}), toJsValue({key})), [{list}]))"
                        )
                    }
                    _ => {
                        let callee_js = self.emit_js_expr(callee, locals);
                        format!(
                            "fromJsValue(__jsMethodInvoke({{ r: undefined, m: 'value', f: toJsValue({callee_js}) }}, [{list}]))"
                        )
                    }
                }
            }
            // The bridge the WASM arms call, so both send `fetch(url, { method, body,
            // headers: { Authorization } })`. Arguments evaluate in source order (url,
            // auth header, body); the result is the request's promise, for the caller
            // to await or chain.
            Expr::FetchGetWithAuth { url, auth_header } => {
                let u = self.emit_js_expr(url, locals);
                let a = self.emit_js_expr(auth_header, locals);
                format!(
                    "((__u, __a) => fromJsValue(__memDispatch.fetch_with_options(toJsValue(__u), undefined, undefined, {{ Authorization: toJsValue(__a) }})))({}, {})",
                    u, a
                )
            }
            Expr::FetchPostWithAuth {
                url,
                auth_header,
                body,
            } => {
                let u = self.emit_js_expr(url, locals);
                let a = self.emit_js_expr(auth_header, locals);
                let b = self.emit_js_expr(body, locals);
                format!(
                    "((__u, __a, __b) => fromJsValue(__memDispatch.fetch_with_options(toJsValue(__u), \"POST\", toJsValue(__b), {{ Authorization: toJsValue(__a) }})))({}, {}, {})",
                    u, a, b
                )
            }
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
                headers_dynamic,
                signal: _,
                redirect,
            } => {
                let url_js = self.emit_js_expr(url, locals);
                let method_js = self.emit_js_expr(method, locals);
                let body_js = self.emit_js_expr(body, locals);
                // In async JS context, we can do a real fetch
                let mut opts = format!("{{ method: getString({}) || 'GET'", method_js);
                if !matches!(body.as_ref(), Expr::Undefined) {
                    opts.push_str(&format!(", body: getString({})", body_js));
                }
                if let Some(hexpr) = headers_dynamic {
                    // Dynamically-built headers: pass the JS object through so
                    // fetch enumerates every property (#4932).
                    let h = self.emit_js_expr(hexpr, locals);
                    opts.push_str(&format!(", headers: toJsValue({})", h));
                } else if !headers.is_empty() {
                    opts.push_str(", headers: {");
                    for (i, (key, val)) in headers.iter().enumerate() {
                        if i > 0 {
                            opts.push_str(", ");
                        }
                        let v = self.emit_js_expr(val, locals);
                        opts.push_str(&format!("{}: getString({})", js_string_literal(key), v));
                    }
                    opts.push('}');
                }
                if let Some(mode) = redirect {
                    let redirect_js = self.emit_js_expr(mode, locals);
                    opts.push_str(&format!(", redirect: getString({})", redirect_js));
                }
                opts.push('}');
                format!("fromJsValue(await fetch(getString({}), {}))", url_js, opts)
            }
            Expr::PropertyGet {
                object, property, ..
            } => {
                let obj = self.emit_js_expr(object, locals);
                format!(
                    "fromJsValue(toJsValue({})[{}])",
                    obj,
                    js_string_literal(property)
                )
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } => {
                let obj = self.emit_js_expr(object, locals);
                let val = self.emit_js_expr(value, locals);
                // Object, then value, each evaluated once: the value is both stored and
                // the expression's result, and may have side effects.
                format!(
                    "((__o, __v) => (toJsValue(__o)[{}] = toJsValue(__v), __v))({}, {})",
                    js_string_literal(property),
                    obj,
                    val
                )
            }
            Expr::Object(fields) => {
                let mut parts = Vec::new();
                for (key, val) in fields {
                    let v = self.emit_js_expr(val, locals);
                    parts.push(format!("{}: toJsValue({})", js_string_literal(key), v));
                }
                format!("fromJsValue({{{}}})", parts.join(", "))
            }
            Expr::Array(elements, _) => {
                let elems: Vec<String> = elements
                    .iter()
                    .map(|e| format!("toJsValue({})", self.emit_js_expr(e, locals)))
                    .collect();
                format!("fromJsValue([{}])", elems.join(", "))
            }
            // An array literal with a spread is a JS array literal: a spread element expands
            // any iterable, and a hole stays a hole. Each element ends in a comma, so a
            // trailing hole still counts toward the length.
            Expr::ArraySpread(elements) => {
                let elems: String = elements
                    .iter()
                    .map(|el| match el {
                        ArrayElement::Expr(e) => {
                            format!("toJsValue({}),", self.emit_js_expr(e, locals))
                        }
                        ArrayElement::Spread(e) => {
                            format!("...toJsValue({}),", self.emit_js_expr(e, locals))
                        }
                        ArrayElement::Hole => ",".to_string(),
                    })
                    .collect();
                format!("fromJsValue([{}])", elems)
            }
            // `Array.from` calls the bridges its WASM arms call (`emit/expr/arrays.rs`).
            Expr::ArrayFrom(value, _) => self.emit_js_bridge_call("array_from", &[value], locals),
            Expr::ArrayFromMapped {
                iterable,
                map_fn,
                this_arg,
            } => {
                let this_arg = this_arg.as_deref().unwrap_or(&Expr::Undefined);
                self.emit_js_bridge_call("array_from_mapped", &[iterable, map_fn, this_arg], locals)
            }
            Expr::ArrayKeys(a) | Expr::ArrayValues(a) | Expr::ArrayEntries(a) => {
                let method = match expr {
                    Expr::ArrayKeys(_) => "keys",
                    Expr::ArrayValues(_) => "values",
                    _ => "entries",
                };
                let v = self.emit_js_expr(a, locals);
                format!("fromJsValue(toJsValue({}).{}())", v, method)
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let c = self.emit_js_expr(condition, locals);
                let t = self.emit_js_expr(then_expr, locals);
                let e = self.emit_js_expr(else_expr, locals);
                format!("(toJsValue({}) ? {} : {})", c, t, e)
            }
            Expr::NativeMethodCall {
                module,
                method,
                object,
                args,
                class_name,
            } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "__perry_borrowed_proof" => "u64ToF64(TAG_FALSE)".to_string(),
                    "__perry_runtime" => self.emit_js_runtime_intrinsic(method, args, locals),
                    "console" => {
                        let args_js: Vec<String> = args
                            .iter()
                            .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                            .collect();
                        match method.as_str() {
                            "log" => format!(
                                "(console.log({}), u64ToF64(TAG_UNDEFINED))",
                                args_js.join(", ")
                            ),
                            "warn" => format!(
                                "(console.warn({}), u64ToF64(TAG_UNDEFINED))",
                                args_js.join(", ")
                            ),
                            "error" => format!(
                                "(console.error({}), u64ToF64(TAG_UNDEFINED))",
                                args_js.join(", ")
                            ),
                            _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                        }
                    }
                    "JSON" => match method.as_str() {
                        "parse" if !args.is_empty() => {
                            let a = self.emit_js_expr(&args[0], locals);
                            format!("fromJsValue(JSON.parse(getString({})))", a)
                        }
                        "stringify" if !args.is_empty() => {
                            let a = self.emit_js_expr(&args[0], locals);
                            format!("fromJsValue(JSON.stringify(toJsValue({})))", a)
                        }
                        _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                    },
                    "perry/ui" | "perry/system" => {
                        let bridge_name = map_ui_method(method, class_name.as_deref());
                        let args_js: Vec<String> =
                            args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                        if let Some(obj) = object {
                            let obj_js = self.emit_js_expr(obj, locals);
                            let mut all_args = vec![obj_js];
                            all_args.extend(args_js);
                            format!("__perryUi.{}({})", bridge_name, all_args.join(", "))
                        } else {
                            format!("__perryUi.{}({})", bridge_name, args_js.join(", "))
                        }
                    }
                    _ => {
                        if let Some(obj) = object {
                            let obj_js = self.emit_js_expr(obj, locals);
                            // The HIR lowers a bare read of a host getter (`res.status`)
                            // to this call with no arguments, the same shape as
                            // `res.text()`; the member's value tells them apart.
                            if args.is_empty() {
                                return format!(
                                    "fromJsValue(__jsNativeMember(toJsValue({}), {}))",
                                    obj_js,
                                    js_string_literal(method)
                                );
                            }
                            let args_js: Vec<String> = args
                                .iter()
                                .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                                .collect();
                            format!(
                                "fromJsValue(toJsValue({}).{}({}))",
                                obj_js,
                                method,
                                args_js.join(", ")
                            )
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                }
            }
            Expr::SetValueAt { set: coll, idx }
            | Expr::MapEntryKeyAt { map: coll, idx }
            | Expr::MapEntryValueAt { map: coll, idx } => {
                let bridge = match expr {
                    Expr::SetValueAt { .. } => "set_value_at",
                    Expr::MapEntryKeyAt { .. } => "map_entry_key_at",
                    _ => "map_entry_value_at",
                };
                format!(
                    "fromJsValue(__memDispatch.{}(toJsValue({}), toJsValue({})))",
                    bridge,
                    self.emit_js_expr(coll, locals),
                    self.emit_js_expr(idx, locals)
                )
            }
            Expr::ArrayJoin { array, separator } => {
                let a = self.emit_js_expr(array, locals);
                let sep = separator
                    .as_ref()
                    .map(|sep| format!("toJsValue({})", self.emit_js_expr(sep, locals)))
                    .unwrap_or_default();
                format!("fromJsValue(toJsValue({}).join({}))", a, sep)
            }
            Expr::ObjectGetPrototypeOf(o) => {
                let o = self.emit_js_expr(o, locals);
                format!(
                    "fromJsValue(__memDispatch.object_get_prototype_of(toJsValue({})))",
                    o
                )
            }
            Expr::ReflectGetPrototypeOf(o) => {
                let o = self.emit_js_expr(o, locals);
                format!(
                    "fromJsValue(__memDispatch.reflect_get_prototype_of(toJsValue({})))",
                    o
                )
            }
            Expr::ObjectIs(a, b) => {
                let a = self.emit_js_expr(a, locals);
                let b = self.emit_js_expr(b, locals);
                format!(
                    "fromJsValue(__memDispatch.object_is(toJsValue({}), toJsValue({})))",
                    a, b
                )
            }
            Expr::ObjectKeys(o) => {
                let o = self.emit_js_expr(o, locals);
                format!("fromJsValue(Object.keys(toJsValue({})))", o)
            }
            Expr::ObjectValues(o) => {
                let o = self.emit_js_expr(o, locals);
                format!("fromJsValue(Object.values(toJsValue({})))", o)
            }
            Expr::ObjectCreate(proto, props) => {
                let proto = self.emit_js_expr(proto, locals);
                let props = props.as_ref().map_or_else(
                    || "undefined".to_string(),
                    |p| format!("toJsValue({})", self.emit_js_expr(p, locals)),
                );
                format!("fromJsValue(__memDispatch.object_create(toJsValue({proto}), {props}))")
            }
            Expr::ObjectDefineProperty(obj, key, desc) => {
                let (obj, key, desc) = (
                    self.emit_js_expr(obj, locals),
                    self.emit_js_expr(key, locals),
                    self.emit_js_expr(desc, locals),
                );
                format!("fromJsValue(__memDispatch.object_define_property(toJsValue({obj}), toJsValue({key}), toJsValue({desc})))")
            }
            Expr::ObjectDefineProperties(obj, props) => {
                let (obj, props) = (self.emit_js_expr(obj, locals), self.emit_js_expr(props, locals));
                format!("fromJsValue(__memDispatch.object_define_properties(toJsValue({obj}), toJsValue({props})))")
            }
            Expr::ObjectGetOwnPropertyDescriptor(obj, key) => {
                let (obj, key) = (self.emit_js_expr(obj, locals), self.emit_js_expr(key, locals));
                format!("fromJsValue(__memDispatch.object_get_own_property_descriptor(toJsValue({obj}), toJsValue({key})))")
            }
            Expr::ObjectEntries(o) => {
                let o = self.emit_js_expr(o, locals);
                format!("fromJsValue(Object.entries(toJsValue({})))", o)
            }
            // The same bridge as the WASM arm, so `for...in` agrees between sync and async bodies.
            Expr::ForInKeys(o) => {
                let o = self.emit_js_expr(o, locals);
                format!("fromJsValue(__memDispatch.for_in_keys(toJsValue({o})))")
            }
            Expr::ObjectRest {
                object,
                exclude_keys,
            } => {
                let keys: Vec<String> = exclude_keys.iter().map(|k| js_string_literal(k)).collect();
                format!(
                    "fromJsValue(__memDispatch.object_rest(toJsValue({}), [{}]))",
                    self.emit_js_expr(object, locals),
                    keys.join(", ")
                )
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    let m_js = self.emit_js_expr(m, locals);
                    format!("fromJsValue(new Error(getString({})))", m_js)
                } else {
                    "fromJsValue(new Error())".to_string()
                }
            }
            Expr::ErrorMessage(err) => {
                let e = self.emit_js_expr(err, locals);
                format!("fromJsValue(toJsValue({}).message)", e)
            }
            Expr::ErrorNewWithCause { message, cause } => {
                let m_js = self.emit_js_expr(message, locals);
                let c_js = self.emit_js_expr(cause, locals);
                format!(
                    "fromJsValue(new Error(getString({}), {{ cause: toJsValue({}) }}))",
                    m_js, c_js
                )
            }
            Expr::ErrorNewWithOptions {
                kind,
                message,
                options,
            } => {
                let ctor = match kind {
                    1 => "TypeError",
                    2 => "RangeError",
                    3 => "ReferenceError",
                    4 => "SyntaxError",
                    _ => "Error",
                };
                let m_js = self.emit_js_expr(message, locals);
                let o_js = self.emit_js_expr(options, locals);
                format!(
                    "fromJsValue(new {}(getString({}), toJsValue({})))",
                    ctor, m_js, o_js
                )
            }
            Expr::TypeErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new TypeError(getString({})))", m_js)
            }
            Expr::RangeErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new RangeError(getString({})))", m_js)
            }
            Expr::ReferenceErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new ReferenceError(getString({})))", m_js)
            }
            Expr::SyntaxErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new SyntaxError(getString({})))", m_js)
            }
            Expr::AggregateErrorNew {
                errors,
                message,
                options,
            } => {
                let e_js = self.emit_js_expr(errors, locals);
                let m_js = self.emit_js_expr(message, locals);
                match options {
                    Some(o) => {
                        let o_js = self.emit_js_expr(o, locals);
                        format!(
                            "fromJsValue(new AggregateError(toJsValue({}), getString({}), toJsValue({})))",
                            e_js, m_js, o_js
                        )
                    }
                    None => format!(
                        "fromJsValue(new AggregateError(toJsValue({}), getString({})))",
                        e_js, m_js
                    ),
                }
            }
            Expr::JsonParse(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.parse(getString({})))", v)
            }
            Expr::JsonStringify(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.stringify(toJsValue({})))", v)
            }
            // The HIR lowers every `JSON.stringify` call to this form. The bridge
            // is the one WASM code calls, so a closure replacer runs the same way.
            Expr::JsonStringifyFull(val, replacer, space) => {
                let v = self.emit_js_expr(val, locals);
                let r = self.emit_js_expr(replacer, locals);
                let s = self.emit_js_expr(space, locals);
                format!(
                    "fromJsValue(__memDispatch.json_stringify(toJsValue({}), toJsValue({}), toJsValue({})))",
                    v, r, s
                )
            }
            Expr::This => "__this".to_string(),
            Expr::IndexGet { object, index, .. } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                format!("fromJsValue(toJsValue({})[toJsValue({})])", obj, idx)
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                let val = self.emit_js_expr(value, locals);
                format!(
                    "(toJsValue({})[toJsValue({})] = toJsValue({}), {})",
                    obj, idx, val, val
                )
            }
            // #5016: `obj.prop = v` / `obj[i] = v` lower to PutValueSet. In the
            // JS fallback both are plain `obj[key] = val`; return the RHS value.
            Expr::PutValueSet {
                target, key, value, ..
            } => {
                let obj = self.emit_js_expr(target, locals);
                let k = self.emit_js_expr(key, locals);
                let val = self.emit_js_expr(value, locals);
                // Target, key, then value, each evaluated once (see PropertySet).
                format!(
                    "((__o, __k, __v) => (toJsValue(__o)[toJsValue(__k)] = toJsValue(__v), __v))({}, {}, {})",
                    obj, k, val
                )
            }
            Expr::ArrayPush {
                array_id, value, ..
            } => {
                let arr = self.emit_js_expr(&Expr::LocalGet(*array_id), locals);
                let val = self.emit_js_expr(value, locals);
                format!("fromJsValue(toJsValue({}).push(toJsValue({})))", arr, val)
            }
            Expr::StringCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(String(toJsValue({})))", v)
            }
            Expr::TemplateStringCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(`${{toJsValue({})}}`)", v)
            }
            Expr::ObjectCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(Object(toJsValue({})))", v)
            }
            // `Math.max` / `Math.min` convert every argument with ToNumber after evaluating
            // them all, as the JS builtins do.
            Expr::MathMax(args) | Expr::MathMin(args) => {
                let name = if matches!(expr, Expr::MathMax(_)) {
                    "max"
                } else {
                    "min"
                };
                let args_js: Vec<String> = args
                    .iter()
                    .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                    .collect();
                format!("Math.{}({})", name, args_js.join(", "))
            }
            Expr::MathFloor(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.floor({})", v)
            }
            Expr::MathCeil(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.ceil({})", v)
            }
            Expr::MathRound(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.round({})", v)
            }
            Expr::MathTrunc(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.trunc({})", v)
            }
            Expr::MathSign(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.sign({})", v)
            }
            Expr::MathAbs(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.abs({})", v)
            }
            Expr::MathMinSpread(src) => {
                let v = self.emit_js_expr(src, locals);
                format!("Math.min(...toJsValue({}))", v)
            }
            Expr::MathMaxSpread(src) => {
                let v = self.emit_js_expr(src, locals);
                format!("Math.max(...toJsValue({}))", v)
            }
            Expr::MathRandom => "Math.random()".to_string(),
            Expr::DateNow => "Date.now()".to_string(),
            Expr::DateNew(args) => {
                let operands: Vec<&Expr> = args.iter().collect();
                self.emit_js_bridge_call("date_new_val", &operands, locals)
            }
            Expr::DateToUTCString(d) => {
                self.emit_js_bridge_call("date_to_utc_string", &[d], locals)
            }
            // `new` on a constructor value (`new Map<K, V>([...])` among others) and the Map and
            // Set nodes call the bridges their WASM arms call (`emit/expr/classes.rs`,
            // `emit/expr/map_set.rs`).
            Expr::NewDynamic { callee, args, .. } => {
                let operands: Vec<&Expr> = std::iter::once(&**callee).chain(args).collect();
                self.emit_js_bridge_call("new_dynamic", &operands, locals)
            }
            Expr::MapNew => self.emit_js_bridge_call("map_new", &[], locals),
            Expr::SetNew => self.emit_js_bridge_call("set_new", &[], locals),
            Expr::SetNewFromArray(a) => self.emit_js_bridge_call("set_new_from_array", &[a], locals),
            Expr::MapSize(a) => self.emit_js_bridge_call("map_size", &[a], locals),
            Expr::SetSize(a) => self.emit_js_bridge_call("set_size", &[a], locals),
            Expr::MapEntries(a) => self.emit_js_bridge_call("map_entries", &[a], locals),
            Expr::MapKeys(a) => self.emit_js_bridge_call("map_keys", &[a], locals),
            Expr::MapValues(a) => self.emit_js_bridge_call("map_values", &[a], locals),
            Expr::SetValues(a) => self.emit_js_bridge_call("set_values", &[a], locals),
            Expr::MapClear(a) => self.emit_js_bridge_call("map_clear", &[a], locals),
            Expr::SetClear(a) => self.emit_js_bridge_call("set_clear", &[a], locals),
            // The WeakRef and FinalizationRegistry nodes call the bridges their WASM arms
            // call (`emit/expr/url_process_path.rs`).
            Expr::WeakRefNew(a) => self.emit_js_bridge_call("weakref_new", &[a], locals),
            Expr::WeakRefDeref(a) => self.emit_js_bridge_call("weakref_deref", &[a], locals),
            Expr::FinalizationRegistryNew(a) => {
                self.emit_js_bridge_call("finreg_new", &[a], locals)
            }
            Expr::FinalizationRegistryRegister {
                registry,
                target,
                held,
                token,
            } => {
                let mut operands: Vec<&Expr> = vec![registry, target, held];
                operands.extend(token.as_deref());
                self.emit_js_bridge_call("finreg_register", &operands, locals)
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.emit_js_bridge_call("finreg_unregister", &[registry, token], locals)
            }
            Expr::MapGet { map, key } => self.emit_js_bridge_call("map_get", &[map, key], locals),
            // The `has` and `delete` bridges answer 1/0 and nothing, for their WASM callers;
            // JS wants the boolean, and `delete` says whether the entry was there.
            Expr::MapHas { map: c, key: k } | Expr::SetHas { set: c, value: k } => format!(
                "fromJsValue(((c, k) => (c instanceof Map || c instanceof Set) && c.has(k))(toJsValue({}), toJsValue({})))",
                self.emit_js_expr(c, locals),
                self.emit_js_expr(k, locals)
            ),
            Expr::MapDelete { map: c, key: k } | Expr::SetDelete { set: c, value: k } => format!(
                "fromJsValue(((c, k) => (c instanceof Map || c instanceof Set) && c.delete(k))(toJsValue({}), toJsValue({})))",
                self.emit_js_expr(c, locals),
                self.emit_js_expr(k, locals)
            ),
            // `set` and `add` yield the collection. The operands are the arrow's arguments,
            // so each is evaluated once, in source order, and an `await` stays in the async body.
            Expr::MapSet { map, key, value } => format!(
                "((m, k, v) => (__memDispatch.map_set(toJsValue(m), toJsValue(k), toJsValue(v)), m))({}, {}, {})",
                self.emit_js_expr(map, locals),
                self.emit_js_expr(key, locals),
                self.emit_js_expr(value, locals)
            ),
            Expr::SetAdd { set_id, value } => format!(
                "((s, v) => (__memDispatch.set_add(toJsValue(s), toJsValue(v)), s))({}, {})",
                self.emit_js_expr(&Expr::LocalGet(*set_id), locals),
                self.emit_js_expr(value, locals)
            ),
            Expr::Sequence(exprs) => {
                if exprs.is_empty() {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                } else {
                    let parts: Vec<String> =
                        exprs.iter().map(|e| self.emit_js_expr(e, locals)).collect();
                    format!("({})", parts.join(", "))
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // Issue #1071: prefer imported-variable global over a like-named
                // function. In JS context the WASM exports include a getter for
                // each global as `wasmInstance.exports.__wasm_global_<idx>` (the
                // WASM emitter exports every global by index — see ExportSection
                // population below). Reading the global returns the i64 NaN-box
                // bits matching the source module's let slot.
                let mod_key = (self.current_mod_idx, name.clone());
                if let Some(&gidx) = self.imported_var_globals.get(&mod_key) {
                    format!(
                        "u64ToF64(wasmInstance.exports.__wasm_global_{}.value)",
                        gidx
                    )
                } else if let Some(&func_idx) = self.func_name_map.get(name) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            // The same `closure_new` / `closure_set_capture` sequence as the WASM arm in
            // `emit/expr/classes.rs`. The body is a module function, so a captured `this`
            // is undefined.
            Expr::Closure {
                func_id,
                body,
                captures,
                mutable_captures,
                captures_this,
                is_arrow,
                ..
            } => {
                let Some(&func_idx) = self.func_map.get(func_id) else {
                    return "u64ToF64(TAG_UNDEFINED)".to_string();
                };
                let table_idx = self
                    .func_to_table_idx
                    .get(&func_idx)
                    .copied()
                    .unwrap_or(func_idx);
                let this = closure_this(*captures_this, *is_arrow, body);
                let mut closure = format!(
                    "__memDispatch.closure_new({}, {}, {}, {})",
                    table_idx,
                    captures.len(),
                    u8::from(this == ClosureThis::Receiver),
                    self.fn_proto_code(func_idx)
                );
                let mut values: Vec<String> = captures
                    .iter()
                    .chain(mutable_captures.iter())
                    .map(|id| match locals.get(id) {
                        Some(cell) if self.js_boxed(*id) => cell.clone(),
                        _ => format!("toJsValue({})", self.emit_js_expr(&Expr::LocalGet(*id), locals)),
                    })
                    .collect();
                if this == ClosureThis::Captured {
                    values.push("undefined".to_string());
                }
                for (i, value) in values.iter().enumerate() {
                    closure = format!("__memDispatch.closure_set_capture({closure}, {i}, {value})");
                }
                format!("fromJsValue({closure})")
            }
            // The function's one value, as the WASM arm in `emit/expr/classes.rs` makes it:
            // the raw export takes i64 parameters, so a JS caller cannot pass it numbers.
            Expr::FuncRef(id) => {
                let Some(&func_idx) = self.func_map.get(id) else {
                    return "u64ToF64(TAG_UNDEFINED)".to_string();
                };
                let table_idx = self
                    .func_to_table_idx
                    .get(&func_idx)
                    .copied()
                    .unwrap_or(func_idx);
                format!(
                    "fromJsValue(__memDispatch.function_value({}, {}))",
                    table_idx,
                    self.fn_proto_code(func_idx)
                )
            }
            Expr::New {
                class_name, args, ..
            } => {
                let args_js: Vec<String> =
                    args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                if !self.user_classes.contains(class_name.as_str()) {
                    // A host constructor (`Map`, `Error`, ...): the class name is a global.
                    let host_args: Vec<String> = args_js
                        .iter()
                        .map(|a| format!("toJsValue({})", a))
                        .collect();
                    return format!(
                        "fromJsValue(new globalThis[{}]({}))",
                        js_string_literal(class_name),
                        host_args.join(", ")
                    );
                }
                // A user class, including the `__AnonShape_*` class of an object literal,
                // has no JS constructor. Birth it as the WASM arm's `class_new` does
                // (`__classInstance`) and run the compiled constructor, which returns `this`.
                let instance = format!(
                    "fromJsValue(__classInstance({}))",
                    js_string_literal(class_name)
                );
                let Some(&ctor_idx) = self.class_ctor_map.get(class_name.as_str()) else {
                    return instance;
                };
                let mut call_args = vec![format!("f64ToU64({})", instance)];
                call_args.extend(args_js.iter().map(|a| format!("f64ToU64({})", a)));
                // Through the callback boundary, which packs a rest parameter's arguments
                // and pads missing ones to the constructor's arity (excess ones are still
                // evaluated, and the export ignores them): an exception the constructor raised
                // under an enclosing WASM `try` is left pending, and this rethrows it
                // here instead of handing back `this` as if it had succeeded.
                format!(
                    "u64ToF64(__invokeWasmCallback(wasmInstance.exports.__wasm_func_{}, {}))",
                    ctor_idx,
                    call_args.join(", ")
                )
            }
            Expr::InstanceOf { expr, ty, .. } => {
                let e = self.emit_js_expr(expr, locals);
                // Use the bridge instanceof check
                format!(
                    "(toJsValue({}) instanceof {} ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))",
                    e, ty
                )
            }
            Expr::TypeOf(operand) => {
                let o = self.emit_js_expr(operand, locals);
                format!("fromJsValue(__typeOf(toJsValue({})))", o)
            }
            Expr::Void(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("({}, u64ToF64(TAG_UNDEFINED))", v)
            }
            // `delete` reaches the property reference, not the value read through it, so a
            // nullish receiver throws; any other operand is still evaluated.
            Expr::Delete(e) => match e.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } => format!(
                    "fromJsValue(delete toJsValue({})[{}])",
                    self.emit_js_expr(object, locals),
                    js_string_literal(property)
                ),
                Expr::IndexGet { object, index, .. } => format!(
                    "fromJsValue(delete toJsValue({})[toJsValue({})])",
                    self.emit_js_expr(object, locals),
                    self.emit_js_expr(index, locals)
                ),
                other => format!("({}, u64ToF64(TAG_TRUE))", self.emit_js_expr(other, locals)),
            },
            // The iteration nodes beside the iterator-record intrinsics call the bridges
            // their WASM arms in `emit/expr/iterator_records.rs` call.
            Expr::GetIterator(value) => format!(
                "fromJsValue(__memDispatch.rt_getIterator(toJsValue({})))",
                self.emit_js_expr(value, locals)
            ),
            Expr::IteratorToArray(value) | Expr::ForOfToArray(value) => format!(
                "fromJsValue(__memDispatch.rt_iterableToArray(toJsValue({})))",
                self.emit_js_expr(value, locals)
            ),
            Expr::ArrayIterationPatched => {
                "fromJsValue(__memDispatch.rt_arrayIterationPatched())".to_string()
            }
            _ => {
                // A folded builtin-method node (`a.map(f)`, `m.get(k)`, ...) is the call
                // `receiver.method(...args)`, made as an unfolded method call is.
                if let Some(call) = perry_hir::folded_call::folded_call(expr) {
                    let receiver = match call.receiver {
                        perry_hir::folded_call::Receiver::Expr(e) => self.emit_js_expr(e, locals),
                        perry_hir::folded_call::Receiver::Local(id) => {
                            self.emit_js_expr(&Expr::LocalGet(id), locals)
                        }
                    };
                    let args_js: Vec<String> =
                        call.args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                    return format!(
                        "fromJsValue(__jsMethodInvoke(__jsMethodGet(toJsValue({}), {}), [{}]))",
                        receiver,
                        js_string_literal(call.method),
                        host_args(&args_js)
                    );
                }
                // Fallback: return undefined for unhandled expressions
                "u64ToF64(TAG_UNDEFINED)".to_string()
            }
        }
    }

    /// JS for a `__perry_runtime` intrinsic: the same contract as the WASM arms in
    /// `emit/expr/iterator_records.rs`. The mode-selected forms evaluate only the arm their
    /// mode picks, and every other intrinsic is one call to the bridge the WASM arm calls.
    /// An intrinsic without an arm throws, because an undefined result would run on with
    /// wrong values.
    fn emit_js_runtime_intrinsic(
        &self,
        method: &str,
        args: &[Expr],
        locals: &BTreeMap<u32, String>,
    ) -> String {
        let js = |e: &Expr| self.emit_js_expr(e, locals);
        let select = |mode: &Expr, then_js: String, else_js: String| {
            format!("(toJsValue({}) ? {} : {})", js(mode), then_js, else_js)
        };
        let bridge = |name: &str, args: &[&Expr]| {
            let args_js: Vec<String> = args.iter().map(|a| js(a)).collect();
            format!(
                "fromJsValue(__memDispatch.rt_{}({}))",
                name,
                host_args(&args_js)
            )
        };
        match (method, args) {
            ("arrayRecordForValue", [mode, proto_value, elem]) => {
                select(mode, js(proto_value), js(elem))
            }
            ("arrayRecordForUpdate", [mode, update]) => select(mode, "0".to_string(), js(update)),
            ("arrayRecordForBound", [mode, len, idx, step_call, state_set]) => format!(
                "({}, {})",
                js(state_set),
                select(
                    mode,
                    select(step_call, js(idx), "Infinity".to_string()),
                    js(len)
                )
            ),
            ("arrayRecordIndex", [value]) => js(value),
            // `(source, payload := ..., source := undefined)`: the payload write is the
            // value, and the clear runs after it.
            ("arrayRecordPayload", [_source, payload, clear]) => {
                format!("[{}, {}][0]", js(payload), js(clear))
            }
            // The step runs before its output operand, which reads what it produced.
            ("iteratorStep", [iter, next, output, read @ ..]) => {
                let read_true = Expr::Bool(true);
                let read = read.first().unwrap_or(&read_true);
                format!(
                    "[{}, {}][0]",
                    bridge("iteratorStep", &[iter, next, read]),
                    js(output)
                )
            }
            // The second operand is a source offset for native `--debug-symbols` only.
            ("requireObjectCoercible", [value, ..]) => bridge("requireObjectCoercible", &[value]),
            (name, args) if expr::ITERATOR_RECORD_EAGER.contains(&name) => {
                bridge(name, &args.iter().collect::<Vec<_>>())
            }
            _ => format!(
                "(() => {{ throw new TypeError({}); }})()",
                js_string_literal(&format!(
                    "{method} is not supported in an async function body on the web target"
                ))
            ),
        }
    }

    /// JS that calls `__memDispatch.<bridge>` on `operands` and NaN-boxes its result.
    fn emit_js_bridge_call(
        &self,
        bridge: &str,
        operands: &[&Expr],
        locals: &BTreeMap<u32, String>,
    ) -> String {
        let args: Vec<String> = operands
            .iter()
            .map(|e| self.emit_js_expr(e, locals))
            .collect();
        format!("fromJsValue(__memDispatch.{bridge}({}))", host_args(&args))
    }

    /// A local a closure assigns lives in a `{ value }` cell, the object `box_new` makes
    /// for the WASM arm, so the closure body's `box_get` / `box_set` share it.
    fn js_boxed(&self, id: LocalId) -> bool {
        self.boxed_locals.contains(&(self.current_mod_idx, id))
    }

    /// JS that stores `value` in local `id` and yields it. A local the body does not bind
    /// is a module-level `let` held in its exported WASM global.
    fn emit_js_local_set(
        &self,
        id: LocalId,
        value: &str,
        locals: &BTreeMap<u32, String>,
    ) -> String {
        match locals.get(&id) {
            Some(name) if self.js_boxed(id) => {
                format!("((__v) => ({name}.value = toJsValue(__v), __v))({value})")
            }
            Some(name) => format!("({} = {})", name, value),
            None => match self.module_let_globals.get(&(self.current_mod_idx, id)) {
                Some(&gidx) => format!(
                    "((__v) => (wasmInstance.exports.__wasm_global_{}.value = f64ToU64(__v), __v))({})",
                    gidx, value
                ),
                None => format!("(__l{} = {})", id, value),
            },
        }
    }
}

/// NaN-boxed call arguments as a comma-separated list of JS values.
fn host_args(args_js: &[String]) -> String {
    args_js
        .iter()
        .map(|a| format!("toJsValue({})", a))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A double-quoted JS string literal holding exactly `s`.
fn js_string_literal(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{}\"", escaped)
}

/// Whether a label chain ending in `stmt` labels an iteration statement.
fn labels_a_loop(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => true,
        Stmt::Labeled { body, .. } => labels_a_loop(body),
        _ => false,
    }
}
