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

        let mut body = String::new();
        // Map param names to local IDs for the JS emitter
        let mut local_names: BTreeMap<u32, String> = BTreeMap::new();
        for (i, param) in func.params.iter().enumerate() {
            local_names.insert(param.id, format!("__p{}", i));
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
        let pad = "  ".repeat(indent);
        match stmt {
            Stmt::Let { id, init, .. } => {
                let name = format!("__l{}", id);
                locals.insert(*id, name.clone());
                if let Some(init_expr) = init {
                    let val = self.emit_js_expr(init_expr, locals);
                    out.push_str(&format!("{pad}    let {name} = {val};\n"));
                } else {
                    out.push_str(&format!("{pad}    let {name} = u64ToF64(TAG_UNDEFINED);\n"));
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
                out.push_str(&format!("{pad}    while (toJsValue({cond})) {{\n"));
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
                out.push_str(&format!("{pad}      while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 2);
                }
                if let Some(upd) = update {
                    let u = self.emit_js_expr(upd, locals);
                    out.push_str(&format!("{pad}        {u};\n"));
                }
                out.push_str(&format!("{pad}      }}\n"));
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Try {
                body,
                catch,
                finally,
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
                        out.push_str(&format!("{pad}      let {name} = fromJsValue(__e);\n"));
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
                out.push_str(&format!("{pad}    do {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    }} while (isTruthy({cond}));\n"));
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
            // Issue #569: PreallocateBoxes is a perry-codegen-only directive
            // — JS hoisting handles forward refs natively, so the wasm/JS
            // backend has no equivalent to emit.
            Stmt::PreallocateBoxes(_) => {}
        }
    }

    pub(super) fn emit_js_expr(&self, expr: &Expr, locals: &BTreeMap<u32, String>) -> String {
        match expr {
            Expr::Number(n) => format!("{}", n),
            Expr::Integer(i) => format!("{}", *i as f64),
            Expr::Bool(true) => "u64ToF64(TAG_TRUE)".to_string(),
            Expr::Bool(false) => "u64ToF64(TAG_FALSE)".to_string(),
            Expr::Undefined => "u64ToF64(TAG_UNDEFINED)".to_string(),
            Expr::Null => "u64ToF64(TAG_NULL)".to_string(),
            Expr::String(s) => {
                let escaped = s
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r");
                format!("fromJsValue(\"{}\")", escaped)
            }
            Expr::LocalGet(id) => locals
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("__l{}", id)),
            Expr::LocalSet(id, val) => {
                let name = locals
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("__l{}", id));
                let v = self.emit_js_expr(val, locals);
                format!("({} = {})", name, v)
            }
            Expr::GlobalGet(id) => format!("__g{}", id),
            Expr::GlobalSet(id, val) => {
                let v = self.emit_js_expr(val, locals);
                format!("(__g{} = {})", id, v)
            }
            Expr::Binary { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                match op {
                    BinaryOp::Add => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                    BinaryOp::Sub => format!("({} - {})", l, r),
                    BinaryOp::Mul => format!("({} * {})", l, r),
                    BinaryOp::Div => format!("({} / {})", l, r),
                    BinaryOp::Mod => format!("({} % {})", l, r),
                    _ => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                }
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
                match op {
                    LogicalOp::And => format!("(toJsValue({l}) ? {r} : {l})"),
                    LogicalOp::Or => format!("(toJsValue({l}) ? {l} : {r})"),
                    LogicalOp::Coalesce => format!("(isNull({l}) || isUndefined({l}) ? {r} : {l})"),
                }
            }
            Expr::Unary { op, operand } => {
                let o = self.emit_js_expr(operand, locals);
                match op {
                    UnaryOp::Neg => format!("(-{})", o),
                    UnaryOp::Not => format!(
                        "(toJsValue({}) ? u64ToF64(TAG_FALSE) : u64ToF64(TAG_TRUE))",
                        o
                    ),
                    _ => o,
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
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&func_idx) = self.func_map.get(id) {
                            // Call exported WASM function. WASM funcs use i64 params/result.
                            let args_i64: Vec<String> =
                                args_js.iter().map(|a| format!("f64ToU64({})", a)).collect();
                            format!(
                                "u64ToF64(wasmInstance.exports.__wasm_func_{}({}))",
                                func_idx,
                                args_i64.join(", ")
                            )
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::ExternFuncRef { name, .. } => {
                        if let Some(&func_idx) = self.func_name_map.get(name) {
                            let args_i64: Vec<String> =
                                args_js.iter().map(|a| format!("f64ToU64({})", a)).collect();
                            format!(
                                "u64ToF64(wasmInstance.exports.__wasm_func_{}({}))",
                                func_idx,
                                args_i64.join(", ")
                            )
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::PropertyGet { object, property } => {
                        let obj = self.emit_js_expr(object, locals);
                        let _args_str = args_js.join(", ");
                        format!(
                            "fromJsValue(toJsValue({}).{}({}))",
                            obj,
                            property,
                            args.iter()
                                .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                    _ => {
                        let callee_js = self.emit_js_expr(callee, locals);
                        format!("{}({})", callee_js, args_js.join(", "))
                    }
                }
            }
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
                headers_dynamic,
                signal: _,
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
                        opts.push_str(&format!("'{}': getString({})", key, v));
                    }
                    opts.push('}');
                }
                opts.push('}');
                format!("fromJsValue(await fetch(getString({}), {}))", url_js, opts)
            }
            Expr::PropertyGet { object, property } => {
                let obj = self.emit_js_expr(object, locals);
                format!("fromJsValue(toJsValue({}).{})", obj, property)
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                let obj = self.emit_js_expr(object, locals);
                let val = self.emit_js_expr(value, locals);
                format!(
                    "(toJsValue({}).{} = toJsValue({}), {})",
                    obj, property, val, val
                )
            }
            Expr::Object(fields) => {
                let mut parts = Vec::new();
                for (key, val) in fields {
                    let v = self.emit_js_expr(val, locals);
                    parts.push(format!("'{}': toJsValue({})", key, v));
                }
                format!("fromJsValue({{{}}})", parts.join(", "))
            }
            Expr::Array(elements) => {
                let elems: Vec<String> = elements
                    .iter()
                    .map(|e| format!("toJsValue({})", self.emit_js_expr(e, locals)))
                    .collect();
                format!("fromJsValue([{}])", elems.join(", "))
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
            Expr::This => "__this".to_string(),
            Expr::IndexGet { object, index } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                format!("fromJsValue(toJsValue({})[toJsValue({})])", obj, idx)
            }
            Expr::IndexSet {
                object,
                index,
                value,
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
                format!(
                    "(toJsValue({})[toJsValue({})] = toJsValue({}), {})",
                    obj, k, val, val
                )
            }
            Expr::ArrayPush { array_id, value } => {
                let arr = locals
                    .get(array_id)
                    .cloned()
                    .unwrap_or_else(|| format!("__l{}", array_id));
                let val = self.emit_js_expr(value, locals);
                format!("fromJsValue(toJsValue({}).push(toJsValue({})))", arr, val)
            }
            Expr::StringCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(String(toJsValue({})))", v)
            }
            Expr::ObjectCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(Object(toJsValue({})))", v)
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
            Expr::MathRandom => "Math.random()".to_string(),
            Expr::DateNow => "Date.now()".to_string(),
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
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.func_map.get(id) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            Expr::New {
                class_name, args, ..
            } => {
                let args_js: Vec<String> = args
                    .iter()
                    .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                    .collect();
                format!(
                    "fromJsValue(new (toJsValue(fromJsValue('{}')))({}))",
                    class_name,
                    args_js.join(", ")
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
                format!("fromJsValue(typeof toJsValue({}))", o)
            }
            Expr::Void(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("({}, u64ToF64(TAG_UNDEFINED))", v)
            }
            Expr::Delete(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("(delete toJsValue({}), u64ToF64(TAG_TRUE))", v)
            }
            _ => {
                // Fallback: return undefined for unhandled expressions
                "u64ToF64(TAG_UNDEFINED)".to_string()
            }
        }
    }
}
