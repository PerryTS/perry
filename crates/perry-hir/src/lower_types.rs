//! Type extraction and inference utilities for HIR lowering.
//!
//! Contains functions for inferring types from expressions, extracting
//! TypeScript type annotations, and parsing function parameter types.

use perry_types::{Type, TypeParam};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::LoweringContext;
use crate::lower_patterns::{get_pat_name, lower_lit};

pub(crate) fn infer_type_from_expr(expr: &ast::Expr, ctx: &LoweringContext) -> Type {
    match expr {
        // Literals
        ast::Expr::Lit(lit) => match lit {
            ast::Lit::Num(_) => Type::Number,
            ast::Lit::Str(_) => Type::String,
            ast::Lit::Bool(_) => Type::Boolean,
            ast::Lit::BigInt(_) => Type::BigInt,
            ast::Lit::Null(_) => Type::Null,
            ast::Lit::Regex(_) => Type::Named("RegExp".to_string()),
            _ => Type::Any,
        },

        // Template literals are always strings
        ast::Expr::Tpl(_) => Type::String,

        // Array literals → infer element type from first element
        ast::Expr::Array(arr) => {
            let elem_ty = arr
                .elems
                .iter()
                .find_map(|e| e.as_ref().map(|elem| infer_type_from_expr(&elem.expr, ctx)))
                .unwrap_or(Type::Any);
            Type::Array(Box::new(elem_ty))
        }

        // Variable reference → look up known type
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            ctx.lookup_local_type(name).cloned().unwrap_or(Type::Any)
        }

        // Binary operators
        ast::Expr::Bin(bin) => {
            use ast::BinaryOp::*;
            match bin.op {
                // Comparison/equality operators always return boolean
                EqEq | NotEq | EqEqEq | NotEqEq | Lt | LtEq | Gt | GtEq | In | InstanceOf => {
                    Type::Boolean
                }

                // Addition: string if either side is string, else number if both number
                Add => {
                    let left = infer_type_from_expr(&bin.left, ctx);
                    let right = infer_type_from_expr(&bin.right, ctx);
                    if matches!(left, Type::String) || matches!(right, Type::String) {
                        Type::String
                    } else if matches!(left, Type::Number) && matches!(right, Type::Number) {
                        Type::Number
                    } else {
                        Type::Any
                    }
                }

                // Arithmetic operators → Number if both sides Number
                Sub | Mul | Div | Mod | Exp => {
                    let left = infer_type_from_expr(&bin.left, ctx);
                    let right = infer_type_from_expr(&bin.right, ctx);
                    if matches!(left, Type::Number | Type::Int32)
                        && matches!(right, Type::Number | Type::Int32)
                    {
                        Type::Number
                    } else {
                        Type::Any
                    }
                }

                // Bitwise operators → Number
                BitAnd | BitOr | BitXor | LShift | RShift | ZeroFillRShift => Type::Number,

                // Logical operators → type of operands (simplified)
                LogicalAnd | LogicalOr => {
                    let right = infer_type_from_expr(&bin.right, ctx);
                    if !matches!(right, Type::Any) {
                        right
                    } else {
                        infer_type_from_expr(&bin.left, ctx)
                    }
                }
                NullishCoalescing => {
                    let left = infer_type_from_expr(&bin.left, ctx);
                    if !matches!(left, Type::Any) {
                        left
                    } else {
                        infer_type_from_expr(&bin.right, ctx)
                    }
                }
            }
        }

        // Unary operators
        ast::Expr::Unary(unary) => match unary.op {
            ast::UnaryOp::TypeOf => Type::String,
            ast::UnaryOp::Void => Type::Void,
            ast::UnaryOp::Bang => Type::Boolean,
            ast::UnaryOp::Minus | ast::UnaryOp::Plus | ast::UnaryOp::Tilde => Type::Number,
            _ => Type::Any,
        },

        // Update expressions (++, --) → Number
        ast::Expr::Update(_) => Type::Number,

        // typeof always returns string
        // Conditional (ternary) → try both branches
        ast::Expr::Cond(cond) => {
            let cons = infer_type_from_expr(&cond.cons, ctx);
            let alt = infer_type_from_expr(&cond.alt, ctx);
            if cons == alt {
                cons
            } else {
                Type::Any
            }
        }

        // Parenthesized expression
        ast::Expr::Paren(paren) => infer_type_from_expr(&paren.expr, ctx),

        // Type assertion (x as T) → extract the asserted type
        ast::Expr::TsAs(ts_as) => extract_ts_type(&ts_as.type_ann),

        // Non-null assertion (x!) → infer inner type
        ast::Expr::TsNonNull(non_null) => infer_type_from_expr(&non_null.expr, ctx),

        // Await expression → unwrap Promise
        ast::Expr::Await(await_expr) => {
            let inner = infer_type_from_expr(&await_expr.arg, ctx);
            match inner {
                Type::Promise(inner_ty) => *inner_ty,
                other => other,
            }
        }

        // Function calls → look up known return types
        ast::Expr::Call(call) => {
            if let ast::Callee::Expr(callee) = &call.callee {
                infer_call_return_type(callee, ctx)
            } else {
                Type::Any
            }
        }

        // Method calls on known types
        ast::Expr::Member(member) => {
            // Property access on known types (e.g., arr.length → Number)
            if let ast::MemberProp::Ident(prop) = &member.prop {
                let prop_name = prop.sym.as_ref();
                // Closes #305: `this.<field>` inside a class method must
                // consult the class_field_types registry (set up in v0.5.388
                // for #302 to resolve for-of element types) so a `const m =
                // this.map` Let binding inherits the declared `Map<K, V>`
                // type. Pre-fix the Let's RHS-inferred type was Any (the
                // `_ => Type::Any` catch-all below), so `m`'s for-of fell
                // off the Map fast path and produced 0 iterations + raw
                // pointer-bits keys.
                if matches!(member.obj.as_ref(), ast::Expr::This(_)) {
                    if let Some(class_name) = &ctx.current_class {
                        if let Some(ty) = ctx.lookup_class_field_type(class_name, prop_name) {
                            return ty.clone();
                        }
                    }
                }
                let obj_ty = infer_type_from_expr(&member.obj, ctx);
                match (&obj_ty, prop_name) {
                    (Type::Array(_), "length") => Type::Number,
                    (Type::String, "length") => Type::Number,
                    _ => Type::Any,
                }
            } else {
                Type::Any
            }
        }

        // Assignments return the assigned value type
        ast::Expr::Assign(assign) => infer_type_from_expr(&assign.right, ctx),

        // `new C(...)` → `Type::Named(C)` for plain classes, `Type::Generic`
        // when type args are present (`new Map<K, V>()` must stay Generic so
        // `is_map_expr` / `is_set_expr` etc. match — they check `base == "Map"`
        // on the Generic variant). Phase 4.1 lets `new C().method()` flow
        // through the method-call inference path above.
        //
        // Builtin collection types (Map/Set/WeakMap/WeakSet/Array/Promise) are
        // intrinsically generic — `new Set([1,2,3])` without explicit `<number>`
        // must still return `Type::Generic { base: "Set" }` so downstream
        // `is_set_expr` matches and dispatches through the Set fast path
        // (otherwise `s.has(...)` falls back to dynamic-method lookup and
        // returns `undefined`).
        ast::Expr::New(new_expr) => {
            if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                let name = ident.sym.to_string();
                if let Some(type_args) = new_expr.type_args.as_ref() {
                    if !type_args.params.is_empty() {
                        let args: Vec<Type> = type_args
                            .params
                            .iter()
                            .map(|t| extract_ts_type(t))
                            .collect();
                        return Type::Generic {
                            base: name,
                            type_args: args,
                        };
                    }
                }
                match name.as_str() {
                    // Issue #533: walk the entries arg of `new Map([...])` /
                    // `new WeakMap([...])` so K/V are populated when no explicit
                    // <K, V> is given. Without this, downstream `m.get(k)`
                    // returns Type::Any and `for-of` over the result falls off
                    // the Map fast path, silently producing zero iterations.
                    "Map" | "WeakMap" => {
                        let inferred = infer_map_entries_type(new_expr, ctx);
                        Type::Generic {
                            base: name,
                            type_args: inferred,
                        }
                    }
                    "Set" | "WeakSet" => {
                        let inferred = infer_set_elements_type(new_expr, ctx);
                        Type::Generic {
                            base: name,
                            type_args: inferred,
                        }
                    }
                    "Array" | "Promise" => Type::Generic {
                        base: name,
                        type_args: Vec::new(),
                    },
                    _ => Type::Named(name),
                }
            } else {
                Type::Any
            }
        }

        // new Array(), new Map(), etc. handled separately in var decl lowering
        // Object literals — infer the structural shape so downstream code (direct-GEP
        // property access, scalar replacement shape checks) can specialize. Bails to
        // Type::Any on anything that makes the shape non-closed: spread, computed
        // keys, methods/getters/setters, bigint keys.
        ast::Expr::Object(obj) => {
            let mut properties: std::collections::HashMap<String, perry_types::PropertyInfo> =
                std::collections::HashMap::new();
            let mut open_shape = false;
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Spread(_) => {
                        open_shape = true;
                        break;
                    }
                    ast::PropOrSpread::Prop(p) => match p.as_ref() {
                        ast::Prop::Shorthand(ident) => {
                            let name = ident.sym.to_string();
                            let ty = ctx.lookup_local_type(&name).cloned().unwrap_or(Type::Any);
                            properties.insert(
                                name,
                                perry_types::PropertyInfo {
                                    ty,
                                    optional: false,
                                    readonly: false,
                                },
                            );
                        }
                        ast::Prop::KeyValue(kv) => {
                            let key = match &kv.key {
                                ast::PropName::Ident(i) => i.sym.to_string(),
                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                ast::PropName::Num(n) => n.value.to_string(),
                                _ => {
                                    open_shape = true;
                                    break;
                                }
                            };
                            let ty = infer_type_from_expr(&kv.value, ctx);
                            properties.insert(
                                key,
                                perry_types::PropertyInfo {
                                    ty,
                                    optional: false,
                                    readonly: false,
                                },
                            );
                        }
                        _ => {
                            open_shape = true;
                            break;
                        }
                    },
                }
            }
            if open_shape {
                Type::Any
            } else {
                Type::Object(perry_types::ObjectType {
                    name: None,
                    properties,
                    index_signature: None,
                })
            }
        }

        // `this` inside a class method → Type::Named(<current class>) so
        // sibling-method calls (`this.foo()`) and field access (`this.x`)
        // can resolve through the Named-receiver paths in
        // `infer_call_return_type` and the Member arm above. Falls back to
        // Type::Any outside a class context (top-level / arrow with no
        // enclosing method — already legal under the existing catch-all).
        ast::Expr::This(_) => ctx
            .current_class
            .as_ref()
            .map(|c| Type::Named(c.clone()))
            .unwrap_or(Type::Any),

        // Arrow/function expressions
        ast::Expr::Arrow(arrow) => {
            // Phase 4 (expansion): when the arrow has no explicit return
            // annotation, infer from the body. Expression bodies (`(x) => x+1`)
            // infer via `infer_type_from_expr` directly; block bodies walk
            // return statements via `infer_body_return_type`. Generators
            // skipped (Generator<T> shape is out of scope). Async wraps in
            // Promise<T>.
            let has_explicit_return_annotation = arrow.return_type.is_some();
            let annotated = arrow
                .return_type
                .as_ref()
                .map(|rt| extract_ts_type(&rt.type_ann))
                .unwrap_or(Type::Any);
            let return_type = if !has_explicit_return_annotation
                && matches!(annotated, Type::Any)
                && !arrow.is_generator
            {
                let inferred = match arrow.body.as_ref() {
                    ast::BlockStmtOrExpr::Expr(expr) => {
                        let t = infer_type_from_expr(expr, ctx);
                        if matches!(t, Type::Any) {
                            None
                        } else {
                            Some(t)
                        }
                    }
                    ast::BlockStmtOrExpr::BlockStmt(block) => {
                        infer_body_return_type(&block.stmts, ctx)
                    }
                };
                match inferred {
                    Some(t) if arrow.is_async => Type::Promise(Box::new(t)),
                    Some(t) => t,
                    None => Type::Any,
                }
            } else {
                annotated
            };
            Type::Function(perry_types::FunctionType {
                params: arrow
                    .params
                    .iter()
                    .map(|p| {
                        let name = get_pat_name(p).unwrap_or_default();
                        let ty = extract_param_type_with_ctx(p, None);
                        (name, ty, false)
                    })
                    .collect(),
                return_type: Box::new(return_type),
                is_async: arrow.is_async,
                is_generator: arrow.is_generator,
            })
        }

        _ => Type::Any,
    }
}

/// Infer a function's return type from its body's return statements, for use when
/// the function has no explicit return annotation. Returns `None` on ambiguity
/// (mixed return types, any Type::Any return) so the caller can fall back.
///
/// Walks control-flow statements but does NOT descend into nested functions,
/// arrows, or class bodies — their return statements belong to the inner scope.
pub(crate) fn infer_body_return_type(stmts: &[ast::Stmt], ctx: &LoweringContext) -> Option<Type> {
    let mut returns: Vec<Type> = Vec::new();
    collect_return_types(stmts, ctx, &mut returns);
    if returns.is_empty() {
        return Some(Type::Void);
    }
    // All returns must agree and none may be Any — otherwise bail.
    let first = returns[0].clone();
    if matches!(first, Type::Any) {
        return None;
    }
    if returns.iter().all(|t| *t == first) {
        Some(first)
    } else {
        None
    }
}

fn collect_return_types(stmts: &[ast::Stmt], ctx: &LoweringContext, out: &mut Vec<Type>) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Return(ret) => {
                let ty = match &ret.arg {
                    Some(expr) => infer_type_from_expr(expr, ctx),
                    None => Type::Void,
                };
                out.push(ty);
            }
            ast::Stmt::Block(b) => collect_return_types(&b.stmts, ctx, out),
            ast::Stmt::If(i) => {
                collect_return_types(std::slice::from_ref(i.cons.as_ref()), ctx, out);
                if let Some(alt) = &i.alt {
                    collect_return_types(std::slice::from_ref(alt.as_ref()), ctx, out);
                }
            }
            ast::Stmt::Try(t) => {
                collect_return_types(&t.block.stmts, ctx, out);
                if let Some(catch) = &t.handler {
                    collect_return_types(&catch.body.stmts, ctx, out);
                }
                if let Some(fin) = &t.finalizer {
                    collect_return_types(&fin.stmts, ctx, out);
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    collect_return_types(&case.cons, ctx, out);
                }
            }
            ast::Stmt::While(w) => {
                collect_return_types(std::slice::from_ref(w.body.as_ref()), ctx, out)
            }
            ast::Stmt::DoWhile(d) => {
                collect_return_types(std::slice::from_ref(d.body.as_ref()), ctx, out)
            }
            ast::Stmt::For(f) => {
                collect_return_types(std::slice::from_ref(f.body.as_ref()), ctx, out)
            }
            ast::Stmt::ForIn(f) => {
                collect_return_types(std::slice::from_ref(f.body.as_ref()), ctx, out)
            }
            ast::Stmt::ForOf(f) => {
                collect_return_types(std::slice::from_ref(f.body.as_ref()), ctx, out)
            }
            ast::Stmt::Labeled(l) => {
                collect_return_types(std::slice::from_ref(l.body.as_ref()), ctx, out)
            }
            _ => {} // Decl (nested fns), Expr, Break, Continue, Throw, Debugger, Empty, With
        }
    }
}

/// Infer the return type of a function/method call expression.
/// Issue #533: walk `new Map([[k1, v1], [k2, v2], ...])` / `new WeakMap(...)`
/// to recover K, V from the literal entries. Returns an empty vec when the
/// argument isn't an array literal (e.g. dynamic `new Map(someArr)`) or when
/// no element parses as a 2-tuple — caller treats that as unknown type args.
fn infer_map_entries_type(new_expr: &ast::NewExpr, ctx: &LoweringContext) -> Vec<Type> {
    let Some(args) = new_expr.args.as_ref() else {
        return Vec::new();
    };
    let Some(first_arg) = args.first() else {
        return Vec::new();
    };
    let ast::Expr::Array(arr_lit) = first_arg.expr.as_ref() else {
        return Vec::new();
    };
    for elem_opt in &arr_lit.elems {
        let Some(elem) = elem_opt else { continue };
        let ast::Expr::Array(entry) = elem.expr.as_ref() else {
            continue;
        };
        if entry.elems.len() < 2 {
            continue;
        }
        let k = entry.elems[0]
            .as_ref()
            .map(|t| infer_type_from_expr(&t.expr, ctx))
            .unwrap_or(Type::Any);
        let v = entry.elems[1]
            .as_ref()
            .map(|t| infer_type_from_expr(&t.expr, ctx))
            .unwrap_or(Type::Any);
        return vec![k, v];
    }
    Vec::new()
}

/// Issue #533 (sibling): infer `T` from `new Set([elem1, elem2, ...])` /
/// `new WeakSet(...)` based on the first non-elided element.
fn infer_set_elements_type(new_expr: &ast::NewExpr, ctx: &LoweringContext) -> Vec<Type> {
    let Some(args) = new_expr.args.as_ref() else {
        return Vec::new();
    };
    let Some(first_arg) = args.first() else {
        return Vec::new();
    };
    let ast::Expr::Array(arr_lit) = first_arg.expr.as_ref() else {
        return Vec::new();
    };
    for elem_opt in &arr_lit.elems {
        let Some(elem) = elem_opt else { continue };
        return vec![infer_type_from_expr(&elem.expr, ctx)];
    }
    Vec::new()
}

pub(crate) fn infer_call_return_type(callee: &ast::Expr, ctx: &LoweringContext) -> Type {
    match callee {
        // Direct function call: foo()
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            // Check user-defined function return types
            if let Some(ty) = ctx.lookup_func_return_type(name) {
                return ty.clone();
            }
            // Known built-in functions
            match name {
                "parseInt" | "parseFloat" | "Number" | "Math" => Type::Number,
                "String" => Type::String,
                "Boolean" => Type::Boolean,
                "isNaN" | "isFinite" => Type::Boolean,
                "Array" => Type::Array(Box::new(Type::Any)),
                _ => Type::Any,
            }
        }
        // Method call: obj.method()
        ast::Expr::Member(member) => {
            if let ast::MemberProp::Ident(method) = &member.prop {
                let method_name = method.sym.as_ref();
                let obj_ty = infer_type_from_expr(&member.obj, ctx);

                // Phase 4.1: user class methods. When the receiver is typed
                // as `Type::Named(C)` (e.g., a local declared as `p: Point` or
                // a `new Point()` binding), look up `C.method_name`'s return
                // type in the registry. Populated for both annotated and
                // Phase-4-inferred return types. Runs BEFORE the built-in
                // String/Array/Number/Math/etc. method tables so user classes
                // can't be accidentally shadowed by built-ins that don't
                // apply (e.g., a user class with a `.slice` method wouldn't
                // hit the String table because we already checked Named).
                if let Type::Named(class_name) = &obj_ty {
                    if let Some(ty) = ctx.lookup_class_method_return_type(class_name, method_name) {
                        return ty.clone();
                    }
                    // Built-in TextEncoder / TextDecoder method return types.
                    // `new TextEncoder().encode(s)` → Uint8Array (issue #584:
                    // without this the local typed-anonymously inherits
                    // Type::Any, the codegen index path falls through to the
                    // f64-stride reader, and `bytes[i]` reads 8 packed bytes
                    // as a single f64 instead of one byte).
                    match (class_name.as_str(), method_name) {
                        ("TextEncoder", "encode") => return Type::Named("Uint8Array".into()),
                        ("TextDecoder", "decode") => return Type::String,
                        _ => {}
                    }
                }

                // Issue #533: Map<K, V> / WeakMap<K, V> / Set<T> / WeakSet<T>
                // method-return inference. `m.get(k)` returns V (not V|undef —
                // matches the pattern Array<T>.pop() uses below) so downstream
                // type-driven dispatch (for-of fast path, .size resolution,
                // formatter pretty-printing) sees the right element type
                // without forcing the user to annotate every `const c =
                // m.get(k)!` binding.
                if let Type::Generic { base, type_args } = &obj_ty {
                    match base.as_str() {
                        "Map" | "WeakMap" => {
                            return match method_name {
                                "get" => type_args.get(1).cloned().unwrap_or(Type::Any),
                                "has" | "delete" => Type::Boolean,
                                "set" => obj_ty.clone(),
                                _ => Type::Any,
                            };
                        }
                        "Set" | "WeakSet" => {
                            return match method_name {
                                "has" | "delete" => Type::Boolean,
                                "add" => obj_ty.clone(),
                                _ => Type::Any,
                            };
                        }
                        _ => {}
                    }
                }

                // String methods
                if matches!(obj_ty, Type::String) {
                    return match method_name {
                        "trim" | "trimStart" | "trimEnd" | "toLowerCase" | "toUpperCase"
                        | "slice" | "substring" | "substr" | "replace" | "replaceAll"
                        | "padStart" | "padEnd" | "repeat" | "charAt" | "concat" | "normalize"
                        | "toLocaleLowerCase" | "toLocaleUpperCase" => Type::String,
                        "indexOf" | "lastIndexOf" | "search" | "charCodeAt" | "codePointAt"
                        | "localeCompare" => Type::Number,
                        "startsWith" | "endsWith" | "includes" => Type::Boolean,
                        "split" => Type::Array(Box::new(Type::String)),
                        "match" | "matchAll" => Type::Any, // complex return types
                        _ => Type::Any,
                    };
                }

                // Array methods
                if let Type::Array(elem_ty) = &obj_ty {
                    return match method_name {
                        "push" | "unshift" | "indexOf" | "lastIndexOf" | "findIndex" => {
                            Type::Number
                        }
                        "join" => Type::String,
                        "includes" | "every" | "some" => Type::Boolean,
                        "pop" | "shift" | "find" | "at" => *elem_ty.clone(),
                        "map" | "filter" | "slice" | "concat" | "flat" | "flatMap" | "reverse"
                        | "sort" | "splice" => obj_ty.clone(),
                        "reduce" => Type::Any, // depends on accumulator
                        "fill" => obj_ty.clone(),
                        "forEach" => Type::Void,
                        "length" => Type::Number,
                        _ => Type::Any,
                    };
                }

                // Number methods
                if matches!(obj_ty, Type::Number | Type::Int32) {
                    return match method_name {
                        "toFixed" | "toPrecision" | "toExponential" | "toString" => Type::String,
                        "valueOf" => Type::Number,
                        _ => Type::Any,
                    };
                }

                // Math.* methods
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let obj_name = obj_ident.sym.as_ref();
                    if obj_name == "Math" {
                        return match method_name {
                            "floor" | "ceil" | "round" | "abs" | "sqrt" | "pow" | "min" | "max"
                            | "random" | "log" | "log2" | "log10" | "sin" | "cos" | "tan"
                            | "asin" | "acos" | "atan" | "atan2" | "exp" | "sign" | "trunc"
                            | "cbrt" | "hypot" | "fround" | "clz32" | "imul" => Type::Number,
                            _ => Type::Any,
                        };
                    }
                    if obj_name == "Number" {
                        return match method_name {
                            "parseInt" | "parseFloat" | "EPSILON" | "MAX_SAFE_INTEGER"
                            | "MIN_SAFE_INTEGER" | "MAX_VALUE" | "MIN_VALUE" => Type::Number,
                            "isNaN" | "isFinite" | "isInteger" | "isSafeInteger" => Type::Boolean,
                            _ => Type::Any,
                        };
                    }
                    if obj_name == "JSON" {
                        return match method_name {
                            // JSON.stringify USUALLY returns a string, but returns
                            // `undefined` for undefined / functions / symbols. Using
                            // Type::String would make `console.log(JSON.stringify(undefined))`
                            // print empty (string slot stayed at TAG_UNDEFINED bits).
                            // Use a String|Undefined union so callers route through
                            // dynamic dispatch instead.
                            "stringify" => Type::Union(vec![Type::String, Type::Void]),
                            _ => Type::Any, // parse returns any
                        };
                    }
                    if obj_name == "Object" {
                        return match method_name {
                            "keys" | "values" => Type::Array(Box::new(Type::Any)),
                            "entries" => Type::Array(Box::new(Type::Any)),
                            _ => Type::Any,
                        };
                    }
                    if obj_name == "Date" {
                        return match method_name {
                            "now" => Type::Number,
                            _ => Type::Any,
                        };
                    }
                    // `Buffer.from(...)`, `Buffer.alloc(...)`, etc all
                    // produce a Buffer instance — refining the local type
                    // lets `buf[i]` use the byte-indexed `Uint8ArrayGet`
                    // path and `buf.length` use the inline buffer-length
                    // load instead of falling through to the dynamic
                    // array path which reads f64 elements as JS values.
                    if obj_name == "Buffer" {
                        return match method_name {
                            "from" | "alloc" | "allocUnsafe" | "concat" => {
                                Type::Named("Uint8Array".to_string())
                            }
                            "isBuffer" => Type::Boolean,
                            "byteLength" => Type::Number,
                            "compare" => Type::Number,
                            _ => Type::Any,
                        };
                    }
                    // `crypto.randomBytes(n)` → Buffer; `crypto.randomUUID()`
                    // / `crypto.createHash(...).update(...).digest('hex')`
                    // → string. The digest chain is detected via the
                    // codegen-time chain folding instead of here, since
                    // it requires walking nested calls.
                    if obj_name == "crypto" {
                        return match method_name {
                            "randomBytes" | "scryptSync" | "pbkdf2Sync" => {
                                Type::Named("Uint8Array".to_string())
                            }
                            "randomUUID" => Type::String,
                            _ => Type::Any,
                        };
                    }
                    // console.log etc → void
                    if obj_name == "console" {
                        return Type::Void;
                    }
                }

                // Generic .toString() on any object → String
                if method_name == "toString" {
                    return Type::String;
                }
            }
            Type::Any
        }
        _ => Type::Any,
    }
}

/// Extract type parameters from SWC's TsTypeParamDecl
pub(crate) fn extract_type_params(decl: &ast::TsTypeParamDecl) -> Vec<TypeParam> {
    decl.params
        .iter()
        .map(|p| {
            let name = p.name.sym.to_string();
            let constraint = p.constraint.as_ref().map(|c| Box::new(extract_ts_type(c)));
            let default = p.default.as_ref().map(|d| Box::new(extract_ts_type(d)));
            TypeParam {
                name,
                constraint,
                default,
            }
        })
        .collect()
}

/// Extract a Type from an SWC TypeScript type annotation
/// This version doesn't have access to type parameter context
pub(crate) fn extract_ts_type(ts_type: &ast::TsType) -> Type {
    extract_ts_type_with_ctx(ts_type, None)
}

/// Extract a Type from an SWC TypeScript type annotation with type parameter context
pub(crate) fn extract_ts_type_with_ctx(
    ts_type: &ast::TsType,
    ctx: Option<&LoweringContext>,
) -> Type {
    use ast::TsKeywordTypeKind::*;
    use ast::TsType::*;

    match ts_type {
        // Keyword types (primitives)
        TsKeywordType(kw) => match kw.kind {
            TsNumberKeyword => Type::Number,
            TsStringKeyword => Type::String,
            TsBooleanKeyword => Type::Boolean,
            TsBigIntKeyword => Type::BigInt,
            TsVoidKeyword => Type::Void,
            TsNullKeyword => Type::Null,
            TsUndefinedKeyword => Type::Void,
            TsAnyKeyword => Type::Any,
            TsUnknownKeyword => Type::Unknown,
            TsNeverKeyword => Type::Never,
            TsSymbolKeyword => Type::Symbol,
            TsObjectKeyword => Type::Any, // Generic object
            TsIntrinsicKeyword => Type::Any,
        },

        // Array type: T[]
        TsArrayType(arr) => {
            let elem_type = extract_ts_type_with_ctx(&arr.elem_type, ctx);
            Type::Array(Box::new(elem_type))
        }

        // Tuple type: [T, U, V]
        TsTupleType(tuple) => {
            let elem_types: Vec<Type> = tuple
                .elem_types
                .iter()
                .map(|elem| extract_ts_type_with_ctx(&elem.ty, ctx))
                .collect();
            Type::Tuple(elem_types)
        }

        // Union type: A | B | C
        TsUnionOrIntersectionType(union_or_inter) => {
            match union_or_inter {
                ast::TsUnionOrIntersectionType::TsUnionType(union) => {
                    let types: Vec<Type> = union
                        .types
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, ctx))
                        .collect();
                    Type::Union(types)
                }
                ast::TsUnionOrIntersectionType::TsIntersectionType(_) => {
                    // Intersection types are complex - treat as Any for now
                    Type::Any
                }
            }
        }

        // Type reference: Array<T>, MyClass, T (type param), etc.
        TsTypeRef(type_ref) => {
            let name = match &type_ref.type_name {
                ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                ast::TsEntityName::TsQualifiedName(qname) => {
                    // Qualified names like Foo.Bar
                    format!("{}.{}", get_ts_entity_name(&qname.left), qname.right.sym)
                }
            };

            // First check if this is a type parameter reference (like T, K, V)
            if let Some(context) = ctx {
                if context.is_type_param(&name) {
                    return Type::TypeVar(name);
                }
            }

            // Check for built-in generic types or generic instantiations
            if let Some(type_params) = &type_ref.type_params {
                match name.as_str() {
                    "Array" if !type_params.params.is_empty() => {
                        let elem_type = extract_ts_type_with_ctx(&type_params.params[0], ctx);
                        return Type::Array(Box::new(elem_type));
                    }
                    "Promise" if !type_params.params.is_empty() => {
                        let result_type = extract_ts_type_with_ctx(&type_params.params[0], ctx);
                        return Type::Promise(Box::new(result_type));
                    }
                    _ => {
                        // Generic type instantiation (e.g., Box<number>, Map<string, number>)
                        let type_args: Vec<Type> = type_params
                            .params
                            .iter()
                            .map(|t| extract_ts_type_with_ctx(t, ctx))
                            .collect();
                        return Type::Generic {
                            base: name,
                            type_args,
                        };
                    }
                }
            }

            // Check if this is a type alias — resolve to the underlying type
            // so the codegen sees Union/String/Number instead of Named("BlockTag").
            // Without this, `type BlockTag = 'latest' | number | string` stays as
            // Named("BlockTag") which the codegen treats as I64 (object pointer),
            // causing ABI mismatch when the actual value is a NaN-boxed union.
            if let Some(context) = ctx {
                if let Some(resolved) = context.resolve_type_alias(&name) {
                    return resolved;
                }
            }

            Type::Named(name)
        }

        // Function type: (a: T, b: U) => R
        TsFnOrConstructorType(fn_type) => {
            match fn_type {
                ast::TsFnOrConstructorType::TsFnType(fn_ty) => {
                    // Extract parameter types
                    let params: Vec<(String, Type, bool)> = fn_ty
                        .params
                        .iter()
                        .map(|p| {
                            let (name, ty) = get_fn_param_name_and_type_with_ctx(p, ctx);
                            (name, ty, false) // TODO: detect optional params
                        })
                        .collect();

                    let return_type = extract_ts_type_with_ctx(&fn_ty.type_ann.type_ann, ctx);

                    Type::Function(perry_types::FunctionType {
                        params,
                        return_type: Box::new(return_type),
                        is_async: false,
                        is_generator: false,
                    })
                }
                ast::TsFnOrConstructorType::TsConstructorType(_) => {
                    // Constructor types are complex - treat as Any for now
                    Type::Any
                }
            }
        }

        // Literal types: "foo", 42, true
        TsLitType(lit) => match &lit.lit {
            ast::TsLit::Number(_) => Type::Number,
            ast::TsLit::Str(_) => Type::String,
            ast::TsLit::Bool(_) => Type::Boolean,
            ast::TsLit::BigInt(_) => Type::BigInt,
            ast::TsLit::Tpl(_) => Type::String,
        },

        // Parenthesized type: (T)
        TsParenthesizedType(paren) => extract_ts_type_with_ctx(&paren.type_ann, ctx),

        // Optional type: T?
        TsOptionalType(opt) => extract_ts_type_with_ctx(&opt.type_ann, ctx),

        // Rest type: ...T
        TsRestType(rest) => extract_ts_type_with_ctx(&rest.type_ann, ctx),

        // Type query: typeof x
        TsTypeQuery(_) => Type::Any,

        // Conditional type: T extends U ? X : Y
        TsConditionalType(_) => Type::Any,

        // Mapped type: { [K in T]: U }
        TsMappedType(_) => Type::Any,

        // Index access: T[K]
        TsIndexedAccessType(_) => Type::Any,

        // Infer type: infer T
        TsInferType(_) => Type::Any,

        // this type
        TsThisType(_) => Type::Any,

        // Type predicate: x is T
        TsTypePredicate(_) => Type::Boolean,

        // Import type: import("module").Type
        TsImportType(_) => Type::Any,

        // Type operator: keyof T, readonly T, unique symbol.
        // For `readonly T` we just return the inner type (the readonly
        // modifier is purely a type-system concept; runtime treatment is
        // identical to T). keyof and unique symbol stay as Any.
        TsTypeOperator(op) => {
            use swc_ecma_ast::TsTypeOperatorOp;
            match op.op {
                TsTypeOperatorOp::ReadOnly => extract_ts_type_with_ctx(&op.type_ann, ctx),
                TsTypeOperatorOp::KeyOf => Type::String,
                _ => Type::Any,
            }
        }

        // Type literal: { a: T, b: U }
        TsTypeLit(lit) => {
            let mut properties = std::collections::HashMap::new();
            for member in &lit.members {
                match member {
                    ast::TsTypeElement::TsPropertySignature(prop) => {
                        if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                            let field_name = ident.sym.to_string();
                            let field_type = if let Some(ann) = &prop.type_ann {
                                extract_ts_type_with_ctx(&ann.type_ann, ctx)
                            } else {
                                Type::Any
                            };
                            properties.insert(
                                field_name,
                                perry_types::PropertyInfo {
                                    ty: field_type,
                                    optional: prop.optional,
                                    readonly: prop.readonly,
                                },
                            );
                        }
                    }
                    ast::TsTypeElement::TsMethodSignature(method) => {
                        if let ast::Expr::Ident(ident) = method.key.as_ref() {
                            let method_name = ident.sym.to_string();
                            let return_type = method
                                .type_ann
                                .as_ref()
                                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                                .unwrap_or(Type::Any);
                            let params: Vec<(String, Type, bool)> = method
                                .params
                                .iter()
                                .map(|p| {
                                    let (name, ty) = get_fn_param_name_and_type_with_ctx(p, ctx);
                                    (name, ty, false)
                                })
                                .collect();
                            properties.insert(
                                method_name,
                                perry_types::PropertyInfo {
                                    ty: Type::Function(perry_types::FunctionType {
                                        params,
                                        return_type: Box::new(return_type),
                                        is_async: false,
                                        is_generator: false,
                                    }),
                                    optional: method.optional,
                                    readonly: false,
                                },
                            );
                        }
                    }
                    ast::TsTypeElement::TsIndexSignature(idx_sig) => {
                        // index signature: { [key: string]: T }
                        if let Some(ann) = &idx_sig.type_ann {
                            let val_type = extract_ts_type_with_ctx(&ann.type_ann, ctx);
                            return Type::Object(perry_types::ObjectType {
                                name: None,
                                properties,
                                index_signature: Some(Box::new(val_type)),
                            });
                        }
                    }
                    _ => {}
                }
            }
            if properties.is_empty() {
                Type::Any
            } else {
                Type::Object(perry_types::ObjectType {
                    name: None,
                    properties,
                    index_signature: None,
                })
            }
        }
    }
}

/// Helper to get name from TsEntityName
pub(crate) fn get_ts_entity_name(entity: &ast::TsEntityName) -> String {
    match entity {
        ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
        ast::TsEntityName::TsQualifiedName(qname) => {
            format!("{}.{}", get_ts_entity_name(&qname.left), qname.right.sym)
        }
    }
}

/// Helper to get parameter name and type from TsFnParam with context
pub(crate) fn get_fn_param_name_and_type_with_ctx(
    param: &ast::TsFnParam,
    ctx: Option<&LoweringContext>,
) -> (String, Type) {
    match param {
        ast::TsFnParam::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            (name, ty)
        }
        ast::TsFnParam::Array(arr) => {
            let ty = arr
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_array".to_string(), ty)
        }
        ast::TsFnParam::Rest(rest) => {
            let ty = rest
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_rest".to_string(), ty)
        }
        ast::TsFnParam::Object(obj) => {
            let ty = obj
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
                .unwrap_or(Type::Any);
            ("_obj".to_string(), ty)
        }
    }
}

/// Extract class name from a member expression (e.g., "ethers.JsonRpcProvider" -> "JsonRpcProvider")
/// This is used for extends clauses that reference external module classes
pub(crate) fn extract_member_class_name(member: &ast::MemberExpr) -> String {
    match &member.prop {
        ast::MemberProp::Ident(ident) => ident.sym.to_string(),
        ast::MemberProp::Computed(computed) => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = computed.expr.as_ref() {
                s.value.as_str().unwrap_or("UnknownClass").to_string()
            } else {
                "UnknownClass".to_string()
            }
        }
        ast::MemberProp::PrivateName(priv_name) => priv_name.name.to_string(),
    }
}

/// Extract type from a pattern (handles BindingIdent with type annotation)
/// Used for both parameter patterns and variable declaration bindings
pub(crate) fn extract_pattern_type(pat: &ast::Pat) -> Type {
    extract_pattern_type_with_ctx(pat, None)
}

/// Extract type from a pattern with type parameter context
pub(crate) fn extract_pattern_type_with_ctx(pat: &ast::Pat, ctx: Option<&LoweringContext>) -> Type {
    match pat {
        ast::Pat::Ident(ident) => ident
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Array(arr) => arr
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Rest(rest) => rest
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Object(obj) => obj
            .type_ann
            .as_ref()
            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, ctx))
            .unwrap_or(Type::Any),
        ast::Pat::Assign(assign) => {
            // For default parameters, get type from the left side
            extract_pattern_type_with_ctx(&assign.left, ctx)
        }
        ast::Pat::Invalid(_) | ast::Pat::Expr(_) => Type::Any,
    }
}

/// Alias for parameter type extraction with context
pub(crate) fn extract_param_type_with_ctx(pat: &ast::Pat, ctx: Option<&LoweringContext>) -> Type {
    extract_pattern_type_with_ctx(pat, ctx)
}

/// Extract type from a variable declaration binding
pub(crate) fn extract_binding_type(binding: &ast::Pat) -> Type {
    extract_pattern_type(binding)
}

/// Lower decorators from SWC AST to HIR Decorators
pub(crate) fn lower_decorators(
    _ctx: &mut LoweringContext,
    decorators: &[ast::Decorator],
) -> Vec<Decorator> {
    decorators
        .iter()
        .filter_map(|dec| {
            // The decorator expression can be:
            // - Identifier: @log
            // - Call expression: @log("prefix")
            match dec.expr.as_ref() {
                ast::Expr::Ident(ident) => Some(Decorator {
                    name: ident.sym.to_string(),
                    args: Vec::new(),
                }),
                ast::Expr::Call(call) => {
                    // Get the callee name
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                            // Lower the arguments - for now just handle simple literals
                            let args: Vec<Expr> = call
                                .args
                                .iter()
                                .filter_map(|arg| {
                                    if arg.spread.is_some() {
                                        None // Skip spread arguments for now
                                    } else {
                                        // For decorator args, only handle simple literals for now
                                        match arg.expr.as_ref() {
                                            ast::Expr::Lit(lit) => lower_lit(lit).ok(),
                                            _ => None,
                                        }
                                    }
                                })
                                .collect();
                            return Some(Decorator {
                                name: ident.sym.to_string(),
                                args,
                            });
                        }
                    }
                    None
                }
                _ => None,
            }
        })
        .collect()
}
