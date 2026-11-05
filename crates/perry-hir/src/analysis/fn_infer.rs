//! Per-function inferred signatures (owner decision 140, "fninfer").
//!
//! Every function — TypeScript or plain bundled JavaScript — gets a signature
//! that is as fixed as the module lets us see: a type per parameter, a return
//! type, the type of `this`, and a flow-insensitive type per local (captured
//! locals included, since `LocalId`s are module-unique). The facts are LOCAL
//! guesses, never whole-program proofs: a function reached by a caller this
//! pass cannot see still gets its body typed, because codegen admits a call
//! into the typed body only through an O(1) entry check of the very types
//! recorded here (types pick the lowering, the entry check and the shapes
//! prove the semantics).
//!
//! Sources, joined per function and iterated over the module's call graph
//! (Jacobi rounds, bounded):
//! - declared parameter types and default values;
//! - argument types at every call this module can resolve (a `FuncRef`, a
//!   local bound once to a closure, a method of a known class, `new C`);
//! - how a parameter is used (arithmetic operands are numbers);
//! - literal and initializer types, through the existing HIR expression typer
//!   (`infer_expr_type`) with this pass's local/return/field facts behind it;
//! - callee return types and class field types from `this.f = v` stores.
//!
//! The lattice is HIR's [`Type`]: `Int32`, `Number`, `String`, `Boolean`,
//! `Void`/`Null` (as nullable variants of a union), `Named(class)`,
//! `Object` (a literal shape, ordered keys), `Array(elem)`, `Function`, and
//! `Any`. Unions are capped at [`UNION_CAP`] members; past it, `Any`.

use std::collections::{HashMap, HashSet};

use crate::ir::*;
use crate::types::{FuncId, FunctionType, GlobalId, LocalId, ObjectType, Type};
use crate::walker::walk_expr_children;

use super::value_types::{infer_expr_type, HirTypeFacts};

mod apply;
mod callee;
mod layout;
pub use apply::{apply_param_types, ApplyPolicy};
pub use callee::{CalleeFacts, CalleeId, Callees, CALLEE_CAP};
pub use layout::{Layout, LayoutFacts};

/// Union members kept before widening to `Any`.
pub const UNION_CAP: usize = 4;
/// Jacobi rounds over the module before unstable facts widen to `Any`.
pub const MAX_ROUNDS: usize = 6;
/// Array/object nesting kept in a type before widening.
const MAX_DEPTH: usize = 3;

/// Join two lattice values (least upper bound, capped).
pub fn join(a: &Type, b: &Type) -> Type {
    join_depth(a, b, 0)
}

fn join_depth(a: &Type, b: &Type, depth: usize) -> Type {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (Type::Never, x) | (x, Type::Never) => x.clone(),
        (Type::Any | Type::Unknown, _) | (_, Type::Any | Type::Unknown) => Type::Any,
        (Type::Int32, Type::Number) | (Type::Number, Type::Int32) => Type::Number,
        (Type::StringLiteral(_), Type::String)
        | (Type::String, Type::StringLiteral(_))
        | (Type::StringLiteral(_), Type::StringLiteral(_)) => Type::String,
        (Type::Array(x), Type::Array(y)) => {
            if depth >= MAX_DEPTH {
                Type::Array(Box::new(Type::Any))
            } else {
                Type::Array(Box::new(join_depth(x, y, depth + 1)))
            }
        }
        (Type::Object(x), Type::Object(y)) => match join_objects(x, y, depth) {
            Some(o) => Type::Object(o),
            None => Type::Any,
        },
        (Type::Function(x), Type::Function(y)) => Type::Function(FunctionType {
            params: Vec::new(),
            return_type: Box::new(join_depth(&x.return_type, &y.return_type, depth + 1)),
            is_async: x.is_async && y.is_async,
            is_generator: x.is_generator && y.is_generator,
        }),
        _ => join_union(a, b, depth),
    }
}

fn join_objects(x: &ObjectType, y: &ObjectType, depth: usize) -> Option<ObjectType> {
    let (Some(xo), Some(yo)) = (&x.property_order, &y.property_order) else {
        return None;
    };
    if xo != yo || x.name != y.name || depth >= MAX_DEPTH {
        return None;
    }
    let mut out = x.clone();
    for (k, info) in out.properties.iter_mut() {
        let other = y.properties.get(k)?;
        info.ty = join_depth(&info.ty, &other.ty, depth + 1);
    }
    Some(out)
}

fn members(t: &Type) -> Vec<Type> {
    match t {
        Type::Union(v) => v.clone(),
        other => vec![other.clone()],
    }
}

fn join_union(a: &Type, b: &Type, depth: usize) -> Type {
    let mut out: Vec<Type> = members(a);
    for m in members(b) {
        if matches!(m, Type::Any | Type::Unknown) {
            return Type::Any;
        }
        // Merge into a compatible member (same kind) or append.
        let mut merged = false;
        for o in out.iter_mut() {
            if same_kind(o, &m) {
                *o = join_depth(o, &m, depth);
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(m);
        }
    }
    if out.iter().any(|m| matches!(m, Type::Any | Type::Unknown)) || out.len() > UNION_CAP {
        return Type::Any;
    }
    if out.len() == 1 {
        return out.pop().unwrap();
    }
    Type::Union(out)
}

fn same_kind(a: &Type, b: &Type) -> bool {
    use Type::*;
    matches!(
        (a, b),
        (Int32 | Number, Int32 | Number)
            | (String | StringLiteral(_), String | StringLiteral(_))
            | (Array(_), Array(_))
            | (Function(_), Function(_))
    ) || a == b
}

/// A type an O(1) entry check can test (tags plus at most one shape/class
/// id per member).
pub fn is_concrete(t: &Type) -> bool {
    match t {
        Type::Any | Type::Unknown | Type::Never | Type::TypeVar(_) => false,
        Type::Object(o) => o.property_order.is_some(),
        Type::Union(v) => v.iter().all(is_concrete),
        Type::Generic { .. } | Type::Tuple(_) => false,
        _ => true,
    }
}

fn is_numeric(t: &Type) -> bool {
    matches!(t, Type::Int32 | Type::Number)
}

/// `+` of two such values is a Number (undefined/null/booleans coerce).
fn is_numberish(t: &Type) -> bool {
    members(t).iter().all(|m| {
        matches!(
            m,
            Type::Int32 | Type::Number | Type::Void | Type::Null | Type::Boolean
        )
    })
}

fn is_prim_concrete(t: &Type) -> bool {
    matches!(
        t,
        Type::Int32 | Type::Number | Type::String | Type::StringLiteral(_) | Type::Boolean
    )
}

/// Receiver types whose property access can lower without a type dispatch
/// (a class/literal shape, an array, a string).
fn is_receiver_concrete(t: &Type) -> bool {
    match t {
        // One object kind plus null/undefined: one null test, then typed.
        Type::Union(v) => {
            let objs: Vec<&Type> = v
                .iter()
                .filter(|m| !matches!(m, Type::Null | Type::Void))
                .collect();
            objs.len() == 1 && is_receiver_concrete(objs[0])
        }
        Type::Named(_) | Type::Array(_) | Type::String | Type::StringLiteral(_) => true,
        Type::Object(o) => o.property_order.is_some(),
        _ => false,
    }
}

thread_local! {
    /// Literal-shape class name -> its key list, for [`fmt_type`] (dump only).
    static ANON_FIELDS: std::cell::RefCell<HashMap<String, String>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Short printable form used by the dump.
pub fn fmt_type(t: &Type) -> String {
    match t {
        Type::Void => "undef".into(),
        Type::Null => "null".into(),
        Type::Boolean => "bool".into(),
        Type::Number => "f64".into(),
        Type::Int32 => "i32".into(),
        Type::BigInt => "bigint".into(),
        Type::String | Type::StringLiteral(_) => "str".into(),
        Type::Symbol => "sym".into(),
        Type::Array(e) => format!("arr<{}>", fmt_type(e)),
        Type::Object(o) => match &o.property_order {
            Some(order) => format!("lit{{{}}}", order.join(",")),
            None => "obj".into(),
        },
        Type::Function(_) => "fn".into(),
        Type::Union(v) => v.iter().map(fmt_type).collect::<Vec<_>>().join("/"),
        Type::Promise(_) => "promise".into(),
        Type::Named(n) => ANON_FIELDS.with(|a| match a.borrow().get(n) {
            Some(keys) => format!("lit{{{keys}}}"),
            None => format!("cls:{n}"),
        }),
        Type::Never => "never".into(),
        _ => "any".into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Function,
    Closure,
    Method,
    Constructor,
    Accessor,
    StaticMethod,
}

struct Unit<'a> {
    id: FuncId,
    name: String,
    kind: UnitKind,
    params: &'a [Param],
    body: &'a [Stmt],
    /// Class whose instances are `this` (methods, constructors, accessors).
    this_class: Option<String>,
    /// Arrow closures read the enclosing unit's `this`.
    this_from_parent: Option<usize>,
    is_async: bool,
    is_generator: bool,
}

/// The signature of one function, as inferred.
#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
    pub this: Type,
}

/// Per-function counts for the precision report.
#[derive(Debug, Clone, Default)]
pub struct FnStats {
    pub params: usize,
    pub params_concrete: usize,
    pub hot_params: usize,
    pub hot_params_concrete: usize,
    pub prop_ops: usize,
    pub prop_ops_concrete: usize,
    pub arith_ops: usize,
    pub arith_ops_concrete: usize,
}

/// Result of [`infer_module`].
#[derive(Debug, Default)]
pub struct ModuleInference {
    pub sigs: HashMap<FuncId, FnSig>,
    pub stats: HashMap<FuncId, FnStats>,
    pub names: HashMap<FuncId, (String, &'static str)>,
    pub locals: HashMap<LocalId, Type>,
    pub rounds: usize,
    /// Callee identities (`new <expr>` sites, value calls).
    pub callees: CalleeFacts,
    /// Birth layouts and family slots (`layout.rs`).
    pub layouts: LayoutFacts,
}

/// Configuration knobs.
#[derive(Debug, Clone, Copy, Default)]
pub struct InferConfig {
    /// Strict mode: a field read or an unresolved call result is `Any`, so
    /// only facts that hold by construction once the entry check passed
    /// remain (params, literals, arithmetic, builtins, typed-callee returns).
    pub strict: bool,
}

#[derive(Default)]
struct State {
    locals: HashMap<LocalId, Type>,
    globals: HashMap<GlobalId, Type>,
    rets: HashMap<FuncId, Type>,
    fields: HashMap<(String, String), Type>,
    params: HashMap<FuncId, Vec<Type>>,
    this_types: Vec<Option<Type>>,
}

struct Ctx<'a> {
    units: Vec<Unit<'a>>,
    by_id: HashMap<FuncId, usize>,
    /// Locals bound to exactly one closure and never reassigned.
    closure_of_local: HashMap<LocalId, FuncId>,
    /// (class, method) -> FuncId; constructors under "constructor".
    methods: HashMap<(String, String), FuncId>,
    class_parent: HashMap<String, String>,
    classes: HashSet<String>,
    /// Every member name a class's instances can answer (own fields,
    /// constructor stores, methods, accessors, parents').
    class_members: Vec<(String, HashSet<String>)>,
    /// Property names read/written/called on each parameter in its body.
    param_uses: HashMap<LocalId, Vec<String>>,
    /// Locals bound to constructor functions (`new F`, `F.prototype.m =`).
    fn_templates: HashMap<LocalId, String>,
    /// Locals with more than one `Let` (hoisted `var`s).
    multi_let: HashSet<LocalId>,
    /// `new <expr>` sites (by byte offset) → the instance types their callee
    /// identities construct (`callee.rs`).
    new_types: HashMap<u32, Type>,
    cfg: InferConfig,
}

impl<'a> Ctx<'a> {
    fn method(&self, class: &str, name: &str) -> Option<FuncId> {
        let mut c = class.to_string();
        for _ in 0..16 {
            if let Some(id) = self.methods.get(&(c.clone(), name.to_string())) {
                return Some(*id);
            }
            c = self.class_parent.get(&c)?.clone();
        }
        None
    }
}

struct Env<'s> {
    st: &'s State,
    this: Option<Type>,
    never: Type,
}

impl HirTypeFacts for Env<'_> {
    fn local_type(&self, id: LocalId) -> Option<&Type> {
        Some(self.st.locals.get(&id).unwrap_or(&self.never))
    }
    fn global_type(&self, id: GlobalId) -> Option<&Type> {
        self.st.globals.get(&id)
    }
    fn function_return_type(&self, id: FuncId) -> Option<&Type> {
        self.st.rets.get(&id)
    }
    fn this_type(&self) -> Option<Type> {
        self.this.clone()
    }
    fn named_property_type(&self, type_name: &str, property: &str) -> Option<Type> {
        self.st
            .fields
            .get(&(type_name.to_string(), property.to_string()))
            .cloned()
    }
}

fn ty_of(e: &Expr, env: &Env<'_>, cx: &Ctx<'_>) -> Type {
    match e {
        Expr::LocalSet(_, v) => ty_of(v, env, cx),
        Expr::Sequence(v) => v.last().map(|x| ty_of(x, env, cx)).unwrap_or(Type::Void),
        Expr::Closure { func_id, .. } => Type::Function(FunctionType {
            params: Vec::new(),
            return_type: Box::new(env.st.rets.get(func_id).cloned().unwrap_or(Type::Never)),
            is_async: false,
            is_generator: false,
        }),
        Expr::Call { callee, .. } => match call_target(callee, env, cx) {
            Some(fid) => env.st.rets.get(&fid).cloned().unwrap_or(Type::Never),
            None => {
                if cx.cfg.strict {
                    Type::Any
                } else {
                    infer_expr_type(e, env)
                }
            }
        },
        Expr::New { class_name, .. } if cx.classes.contains(class_name) => {
            Type::Named(class_name.clone())
        }
        Expr::NewDynamic {
            callee,
            byte_offset,
            ..
        } => match callee.as_ref() {
            Expr::LocalGet(l) if cx.fn_templates.contains_key(l) => match cx.fn_templates.get(l) {
                Some(n) => Type::Named(n.clone()),
                None => infer_expr_type(e, env),
            },
            _ => match cx.new_types.get(byte_offset) {
                Some(t) => t.clone(),
                None => infer_expr_type(e, env),
            },
        },
        Expr::Binary { op, left, right } => {
            let l = ty_of(left, env, cx);
            let r = ty_of(right, env, cx);
            if matches!(l, Type::Never) || matches!(r, Type::Never) {
                return Type::Never;
            }
            match op {
                BinaryOp::Add => {
                    if is_numberish(&l) && is_numberish(&r) {
                        Type::Number
                    } else if l.is_string_like() || r.is_string_like() {
                        Type::String
                    } else {
                        Type::Any
                    }
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr => Type::Int32,
                _ => {
                    if matches!(l, Type::BigInt) || matches!(r, Type::BigInt) {
                        Type::Any
                    } else {
                        Type::Number
                    }
                }
            }
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => join(&ty_of(then_expr, env, cx), &ty_of(else_expr, env, cx)),
        Expr::Logical { op, left, right } => {
            let l = ty_of(left, env, cx);
            let r = ty_of(right, env, cx);
            match op {
                LogicalOp::Coalesce => {
                    let strip: Vec<Type> = members(&l)
                        .into_iter()
                        .filter(|m| !matches!(m, Type::Null | Type::Void))
                        .collect();
                    let mut acc = Type::Never;
                    for m in strip {
                        acc = join(&acc, &m);
                    }
                    join(&acc, &r)
                }
                _ => join(&l, &r),
            }
        }
        Expr::PropertyGet {
            object, property, ..
        } => {
            let ot = ty_of(object, env, cx);
            match &ot {
                Type::Never => Type::Never,
                Type::Array(_) | Type::String | Type::StringLiteral(_) if property == "length" => {
                    Type::Int32
                }
                Type::Named(c) if !cx.cfg.strict => env
                    .st
                    .fields
                    .get(&(c.clone(), property.clone()))
                    .cloned()
                    .unwrap_or(Type::Any),
                Type::Object(o) if !cx.cfg.strict => o
                    .properties
                    .get(property)
                    .map(|i| i.ty.clone())
                    .unwrap_or(Type::Any),
                _ => Type::Any,
            }
        }
        Expr::IndexGet { object, .. } => match ty_of(object, env, cx) {
            Type::Array(el) if !cx.cfg.strict => join(&el, &Type::Void),
            Type::String | Type::StringLiteral(_) => join(&Type::String, &Type::Void),
            _ => Type::Any,
        },
        _ => infer_expr_type(e, env),
    }
}

fn call_target(callee: &Expr, env: &Env<'_>, cx: &Ctx<'_>) -> Option<FuncId> {
    match callee {
        Expr::FuncRef(id) => cx.by_id.contains_key(id).then_some(*id),
        Expr::LocalGet(l) => cx.closure_of_local.get(l).copied(),
        Expr::PropertyGet {
            object, property, ..
        } => match ty_of(object, env, cx) {
            Type::Named(c) => cx.method(&c, property),
            _ => None,
        },
        _ => None,
    }
}

/// Collect every function-like unit of the module.
fn collect_units<'a>(m: &'a Module) -> Vec<Unit<'a>> {
    let mut units = Vec::new();
    for f in &m.functions {
        push_fn(&mut units, f, UnitKind::Function, None, None);
    }
    for c in &m.classes {
        if c.name.starts_with("__AnonShape_") {
            // Synthesized literal-shape constructor, not a source function.
            continue;
        }
        if let Some(ctor) = &c.constructor {
            push_fn(
                &mut units,
                ctor,
                UnitKind::Constructor,
                Some(c.name.clone()),
                None,
            );
        }
        for meth in &c.methods {
            push_fn(
                &mut units,
                meth,
                UnitKind::Method,
                Some(c.name.clone()),
                None,
            );
        }
        for (_, g) in c.getters.iter().chain(c.setters.iter()) {
            push_fn(
                &mut units,
                g,
                UnitKind::Accessor,
                Some(c.name.clone()),
                None,
            );
        }
        for s in &c.static_methods {
            push_fn(&mut units, s, UnitKind::StaticMethod, None, None);
        }
    }
    // Closures nested anywhere (module init, functions, methods, fields).
    let top = units.len();
    let mut roots: Vec<(Option<usize>, &'a [Stmt])> = vec![(None, &m.init)];
    for (idx, u) in units.iter().enumerate().take(top) {
        roots.push((Some(idx), u.body));
    }
    let mut field_exprs: Vec<(Option<usize>, &'a Expr)> = Vec::new();
    for c in &m.classes {
        for f in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(init) = &f.init {
                field_exprs.push((None, init));
            }
        }
    }
    for (parent, body) in roots {
        collect_closures_stmts(&mut units, parent, body);
    }
    for (parent, e) in field_exprs {
        collect_closures_expr(&mut units, parent, e);
    }
    // Nested closures of closures are found by the recursive walk itself.
    units
}

fn push_fn<'a>(
    units: &mut Vec<Unit<'a>>,
    f: &'a Function,
    kind: UnitKind,
    this_class: Option<String>,
    this_from_parent: Option<usize>,
) {
    units.push(Unit {
        id: f.id,
        name: f.name.clone(),
        kind,
        params: &f.params,
        body: &f.body,
        this_class,
        this_from_parent,
        is_async: f.is_async,
        is_generator: f.is_generator,
    });
}

fn collect_closures_stmts<'a>(units: &mut Vec<Unit<'a>>, parent: Option<usize>, body: &'a [Stmt]) {
    for s in body {
        let first = units.len();
        for_each_stmt_expr(s, &mut |e| collect_closures_expr(units, parent, e));
        // `let f = function …` / hoisted `function f` names the closure.
        let named = match s {
            Stmt::Let {
                name,
                init: Some(Expr::Closure { .. }),
                ..
            } => Some(name),
            Stmt::Expr(Expr::LocalSet(_, v)) if matches!(v.as_ref(), Expr::Closure { .. }) => None,
            _ => None,
        };
        if let (Some(n), Some(u)) = (named, units.get_mut(first)) {
            if u.name.is_empty() {
                u.name = n.clone();
            }
        }
    }
}

fn collect_closures_expr<'a>(units: &mut Vec<Unit<'a>>, parent: Option<usize>, e: &'a Expr) {
    if let Expr::Closure {
        func_id,
        params,
        body,
        captures_this,
        is_arrow,
        is_async,
        is_generator,
        ..
    } = e
    {
        let idx = units.len();
        units.push(Unit {
            id: *func_id,
            name: String::new(),
            kind: UnitKind::Closure,
            params,
            body,
            this_class: None,
            this_from_parent: if *captures_this || *is_arrow {
                parent
            } else {
                None
            },
            is_async: *is_async,
            is_generator: *is_generator,
        });
        for p in params.iter() {
            if let Some(d) = &p.default {
                collect_closures_expr(units, Some(idx), d);
            }
        }
        collect_closures_stmts(units, Some(idx), body);
        return;
    }
    walk_expr_children(e, &mut |c| collect_closures_expr(units, parent, c));
}

/// Visit the top-level expressions of a statement tree (closure bodies are
/// NOT entered: the caller decides).
fn for_each_stmt_expr<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a Expr)) {
    match s {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e)
            }
        }
        Stmt::Return(e) => {
            if let Some(e) = e {
                f(e)
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            for s in then_branch {
                for_each_stmt_expr(s, f);
            }
            if let Some(b) = else_branch {
                for s in b {
                    for_each_stmt_expr(s, f);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            for s in body {
                for_each_stmt_expr(s, f);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                for_each_stmt_expr(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
            for s in body {
                for_each_stmt_expr(s, f);
            }
        }
        Stmt::Labeled { body, .. } => for_each_stmt_expr(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                for_each_stmt_expr(s, f);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    for_each_stmt_expr(s, f);
                }
            }
            if let Some(b) = finally {
                for s in b {
                    for_each_stmt_expr(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &c.test {
                    f(t);
                }
                for s in &c.body {
                    for_each_stmt_expr(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

/// Visit every expression node of `e` except the bodies of nested closures.
fn walk_local<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    f(e);
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    walk_expr_children(e, &mut |c| walk_local(c, f));
}

/// Statements of a unit, with every `Let`, return, and catch binding, in
/// order (closure bodies excluded).
fn walk_stmts<'a>(
    body: &'a [Stmt],
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for s in body {
        on_stmt(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_stmts(then_branch, on_stmt, on_expr);
                if let Some(b) = else_branch {
                    walk_stmts(b, on_stmt, on_expr);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                walk_stmts(body, on_stmt, on_expr)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    walk_stmts(std::slice::from_ref(i.as_ref()), on_stmt, on_expr);
                }
                walk_stmts(body, on_stmt, on_expr)
            }
            Stmt::Labeled { body, .. } => {
                walk_stmts(std::slice::from_ref(body.as_ref()), on_stmt, on_expr)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, on_stmt, on_expr);
                if let Some(c) = catch {
                    walk_stmts(&c.body, on_stmt, on_expr);
                }
                if let Some(b) = finally {
                    walk_stmts(b, on_stmt, on_expr);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_stmts(&c.body, on_stmt, on_expr);
                }
            }
            _ => {}
        }
        // Expressions directly owned by this statement (not nested stmts).
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => walk_local(e, on_expr),
            Stmt::Expr(e) | Stmt::Throw(e) => walk_local(e, on_expr),
            Stmt::If { condition, .. }
            | Stmt::While { condition, .. }
            | Stmt::DoWhile { condition, .. } => walk_local(condition, on_expr),
            Stmt::For {
                condition, update, ..
            } => {
                if let Some(c) = condition {
                    walk_local(c, on_expr);
                }
                if let Some(u) = update {
                    walk_local(u, on_expr);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_local(discriminant, on_expr);
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_local(t, on_expr);
                    }
                }
            }
            _ => {}
        }
    }
}

fn add(map: &mut HashMap<LocalId, Type>, id: LocalId, t: Type) {
    let e = map.entry(id).or_insert(Type::Never);
    *e = join(e, &t);
}

/// Infer every function of `m`.
pub fn infer_module(m: &Module, cfg: InferConfig) -> ModuleInference {
    let units = collect_units(m);
    let mut by_id = HashMap::new();
    for (i, u) in units.iter().enumerate() {
        by_id.insert(u.id, i);
    }
    let mut methods = HashMap::new();
    let mut class_parent = HashMap::new();
    let mut classes = HashSet::new();
    for c in &m.classes {
        classes.insert(c.name.clone());
        if let Some(p) = &c.extends_name {
            class_parent.insert(c.name.clone(), p.clone());
        }
        if let Some(ctor) = &c.constructor {
            methods.insert((c.name.clone(), "constructor".to_string()), ctor.id);
        }
        for meth in &c.methods {
            methods.insert((c.name.clone(), meth.name.clone()), meth.id);
        }
    }
    // Locals bound once to a closure, never reassigned.
    let mut closure_of_local = HashMap::new();
    let mut reassigned: HashSet<LocalId> = HashSet::new();
    {
        let mut bound: Vec<(LocalId, FuncId)> = Vec::new();
        let mut bound_set: Vec<(LocalId, FuncId)> = Vec::new();
        let mut set_ids: Vec<LocalId> = Vec::new();
        let mut on_stmt = |s: &Stmt| {
            if let Stmt::Let {
                id,
                init: Some(Expr::Closure { func_id, .. }),
                ..
            } = s
            {
                bound.push((*id, *func_id));
            }
        };
        let mut on_expr = |e: &Expr| match e {
            // A hoisted function declaration is `let f;` + `f = <closure>`:
            // one closure write is still a binding.
            Expr::LocalSet(id, v) => match v.as_ref() {
                Expr::Closure { func_id, .. } => bound_set.push((*id, *func_id)),
                _ => set_ids.push(*id),
            },
            Expr::Update { id, .. } => set_ids.push(*id),
            _ => {}
        };
        walk_stmts(&m.init, &mut on_stmt, &mut on_expr);
        for u in &units {
            walk_stmts(u.body, &mut on_stmt, &mut on_expr);
        }
        for (id, f) in bound.into_iter().chain(bound_set) {
            if let Some(prev) = closure_of_local.insert(id, f) {
                if prev != f {
                    reassigned.insert(id);
                }
            }
        }
        reassigned.extend(set_ids);
    }
    for id in reassigned {
        closure_of_local.remove(&id);
    }
    // Constructor-function templates (`function F(){ this.x = … }`,
    // `F.prototype.m = function …`, `new F(…)`): perry already types their
    // instances as `Named("F")`; give them members, methods and `this`.
    let mut units = units;
    let mut fn_templates: HashMap<LocalId, String> = HashMap::new();
    {
        let mut names: HashMap<LocalId, String> = HashMap::new();
        let mut protos: Vec<(LocalId, String, FuncId)> = Vec::new();
        let mut news: HashSet<LocalId> = HashSet::new();
        let mut on_stmt = |s: &Stmt| {
            if let Stmt::Let { id, name, .. } = s {
                names.insert(*id, name.clone());
            }
        };
        let mut on_expr = |e: &Expr| match e {
            Expr::PutValueSet {
                target, key, value, ..
            } => {
                if let (Some(l), Expr::String(k), Expr::Closure { func_id, .. }) =
                    (proto_of(target), key.as_ref(), value.as_ref())
                {
                    protos.push((l, k.clone(), *func_id));
                }
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } => {
                if let (Some(l), Expr::Closure { func_id, .. }) = (proto_of(object), value.as_ref())
                {
                    protos.push((l, property.clone(), *func_id));
                }
            }
            Expr::NewDynamic { callee, .. } => {
                if let Expr::LocalGet(l) = callee.as_ref() {
                    news.insert(*l);
                }
            }
            _ => {}
        };
        walk_stmts(&m.init, &mut on_stmt, &mut on_expr);
        for u in &units {
            walk_stmts(u.body, &mut on_stmt, &mut on_expr);
        }
        let idx_of: HashMap<FuncId, usize> =
            units.iter().enumerate().map(|(i, u)| (u.id, i)).collect();
        let mut is_template: HashSet<LocalId> = news;
        is_template.extend(protos.iter().map(|(l, _, _)| *l));
        for l in is_template {
            let (Some(fid), Some(name)) = (closure_of_local.get(&l), names.get(&l)) else {
                continue;
            };
            if classes.contains(name) {
                continue;
            }
            fn_templates.insert(l, name.clone());
            classes.insert(name.clone());
            methods.insert((name.clone(), "constructor".to_string()), *fid);
            if let Some(&i) = idx_of.get(fid) {
                units[i].this_class = Some(name.clone());
            }
        }
        for (l, k, f) in protos {
            if let Some(name) = fn_templates.get(&l) {
                methods.insert((name.clone(), k), f);
                if let Some(&i) = idx_of.get(&f) {
                    units[i].this_class = Some(name.clone());
                }
            }
        }
    }
    let mut class_members = class_member_index(m, &class_parent);
    for (l, name) in &fn_templates {
        let mut set: HashSet<String> = methods
            .keys()
            .filter(|(c, _)| c == name)
            .map(|(_, k)| k.clone())
            .collect();
        if let Some(i) = closure_of_local
            .get(l)
            .and_then(|f| units.iter().position(|u| u.id == *f))
        {
            let mut on_stmt = |_: &Stmt| {};
            let mut on_expr = |e: &Expr| {
                if let Some(k) = this_store_key(e) {
                    set.insert(k);
                }
            };
            walk_stmts(units[i].body, &mut on_stmt, &mut on_expr);
        }
        set.remove("constructor");
        class_members.push((name.clone(), set));
    }
    let param_uses = collect_param_uses(&units);
    let multi_let = {
        let mut seen: HashSet<LocalId> = HashSet::new();
        let mut multi: HashSet<LocalId> = HashSet::new();
        let mut on_stmt = |s: &Stmt| {
            if let Stmt::Let { id, .. } = s {
                if !seen.insert(*id) {
                    multi.insert(*id);
                }
            }
        };
        let mut on_expr = |_: &Expr| {};
        walk_stmts(&m.init, &mut on_stmt, &mut on_expr);
        for u in &units {
            walk_stmts(u.body, &mut on_stmt, &mut on_expr);
        }
        multi
    };
    // Callee identities first: they type `new <expr>` sites whose callee is
    // a known class or constructor-function template.
    let callees = callee::infer_callees(m, &units);
    let template_of_fn: HashMap<FuncId, String> = fn_templates
        .iter()
        .filter_map(|(l, n)| closure_of_local.get(l).map(|f| (*f, n.clone())))
        .collect();
    let new_types: HashMap<u32, Type> = callees
        .new_sites
        .iter()
        .filter_map(|(off, c)| {
            let ids = match c {
                Callees::Set(s) => s,
                _ => return None,
            };
            let mut t = Type::Never;
            for id in ids {
                let n = match id {
                    CalleeId::Class(n) if classes.contains(n) => n.clone(),
                    CalleeId::Func(f) => template_of_fn.get(f)?.clone(),
                    CalleeId::Class(_) => return None,
                };
                t = join(&t, &Type::Named(n));
            }
            (!matches!(t, Type::Never | Type::Any)).then_some((*off, t))
        })
        .collect();
    let cx = Ctx {
        new_types,
        units,
        by_id,
        closure_of_local,
        methods,
        class_parent,
        classes,
        class_members,
        param_uses,
        multi_let,
        fn_templates,
        cfg,
    };

    let n = cx.units.len();
    let mut st = State {
        this_types: vec![None; n],
        ..Default::default()
    };
    let mut rounds = 0;
    let mut stable = false;
    let mut prev_snapshot: Option<State> = None;
    while rounds < MAX_ROUNDS {
        rounds += 1;
        let next = round(&cx, &st, m);
        stable = next.locals == st.locals
            && next.rets == st.rets
            && next.params == st.params
            && next.fields == st.fields
            && next.globals == st.globals;
        prev_snapshot = Some(std::mem::replace(&mut st, next));
        if stable {
            break;
        }
    }
    if !stable {
        // Widen whatever still moves.
        if let Some(prev) = prev_snapshot {
            widen_unstable(&mut st.locals, &prev.locals);
            widen_unstable(&mut st.rets, &prev.rets);
            widen_unstable(&mut st.fields, &prev.fields);
            widen_unstable(&mut st.globals, &prev.globals);
            for (k, v) in st.params.iter_mut() {
                if let Some(p) = prev.params.get(k) {
                    for (a, b) in v.iter_mut().zip(p.iter()) {
                        if a != b {
                            *a = Type::Any;
                        }
                    }
                }
            }
        }
    }

    // Final signatures and stats.
    let mut out = ModuleInference {
        rounds,
        ..Default::default()
    };
    for (ui, u) in cx.units.iter().enumerate() {
        let params: Vec<Type> = u
            .params
            .iter()
            .map(|p| finalize(st.locals.get(&p.id)))
            .collect();
        let ret = finalize(st.rets.get(&u.id));
        let this = st.this_types[ui].clone().unwrap_or(Type::Any);
        let stats = unit_stats(&cx, &st, ui);
        out.sigs.insert(u.id, FnSig { params, ret, this });
        out.stats.insert(u.id, stats);
        let kind = match u.kind {
            UnitKind::Function => "function",
            UnitKind::Closure => "closure",
            UnitKind::Method => "method",
            UnitKind::Constructor => "constructor",
            UnitKind::Accessor => "accessor",
            UnitKind::StaticMethod => "static",
        };
        out.names.insert(u.id, (u.name.clone(), kind));
    }
    out.locals = st.locals;
    out.callees = callees;
    let templates: HashMap<String, usize> = cx
        .fn_templates
        .iter()
        .filter_map(|(l, n)| {
            let f = cx.closure_of_local.get(l)?;
            Some((n.clone(), *cx.by_id.get(f)?))
        })
        .collect();
    let param_types: HashMap<FuncId, Vec<Type>> = out
        .sigs
        .iter()
        .map(|(f, s)| (*f, s.params.clone()))
        .collect();
    let this_types: HashMap<FuncId, Type> =
        out.sigs.iter().map(|(f, s)| (*f, s.this.clone())).collect();
    out.layouts = layout::infer_layouts(m, &cx.units, &templates, &param_types, &this_types);
    out
}

const STRING_ONLY: &[&str] = &[
    "charCodeAt",
    "charAt",
    "codePointAt",
    "substring",
    "substr",
    "toLowerCase",
    "toUpperCase",
    "trim",
    "trimStart",
    "trimEnd",
    "startsWith",
    "endsWith",
    "padStart",
    "padEnd",
    "localeCompare",
    "normalize",
    "split",
    "replace",
    "replaceAll",
    "match",
    "matchAll",
    "search",
    "repeat",
];
const ARRAY_ONLY: &[&str] = &[
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "map",
    "filter",
    "forEach",
    "reduce",
    "reduceRight",
    "some",
    "every",
    "find",
    "findIndex",
    "sort",
    "reverse",
    "join",
    "flat",
    "flatMap",
    "fill",
    "findLast",
    "findLastIndex",
];
const SHARED: &[&str] = &[
    "length",
    "slice",
    "indexOf",
    "lastIndexOf",
    "includes",
    "concat",
    "at",
    "toString",
];

/// A guess from how a parameter is used: string/array method names pick a
/// builtin kind; other names pick the classes/literal shapes that have every
/// one of them (at most [`UNION_CAP`]). The entry check makes any guess safe.
fn guess_from_uses(cx: &Ctx<'_>, names: &[String]) -> Option<Type> {
    if names.is_empty() {
        return None;
    }
    let is = |set: &[&str], n: &str| set.contains(&n);
    let all_str = names.iter().all(|n| is(STRING_ONLY, n) || is(SHARED, n));
    let any_str = names.iter().any(|n| is(STRING_ONLY, n));
    let all_arr = names.iter().all(|n| is(ARRAY_ONLY, n) || is(SHARED, n));
    let any_arr = names.iter().any(|n| is(ARRAY_ONLY, n));
    if all_str && any_str {
        return Some(Type::String);
    }
    if all_arr && any_arr {
        return Some(Type::Array(Box::new(Type::Any)));
    }
    if names.iter().all(|n| is(SHARED, n)) {
        return None;
    }
    let mut cands: Vec<&str> = Vec::new();
    for (c, members) in &cx.class_members {
        if names.iter().all(|n| members.contains(n)) {
            cands.push(c);
            if cands.len() > UNION_CAP {
                return None;
            }
        }
    }
    match cands.len() {
        0 => None,
        1 => Some(Type::Named(cands[0].to_string())),
        _ => Some(Type::Union(
            cands
                .into_iter()
                .map(|c| Type::Named(c.to_string()))
                .collect(),
        )),
    }
}

/// Narrow a call-site type `t` with the body's use-based guess `g`:
/// keep the members of `t` that `g` admits; if none, or `t` is `Never`
/// or `Any`, take `g`.
fn narrow_to_use(t: &Type, g: &Type) -> Type {
    if matches!(t, Type::Never | Type::Any | Type::Unknown) {
        return g.clone();
    }
    let gm = members(g);
    let keep: Vec<Type> = members(t)
        .into_iter()
        .filter(|m| gm.iter().any(|x| same_kind(x, m)))
        .collect();
    if keep.is_empty() {
        return g.clone();
    }
    let mut acc = Type::Never;
    for m in keep {
        acc = join(&acc, &m);
    }
    acc
}

fn class_member_index(
    m: &Module,
    parent: &HashMap<String, String>,
) -> Vec<(String, HashSet<String>)> {
    let mut own: HashMap<String, HashSet<String>> = HashMap::new();
    for c in &m.classes {
        let set = own.entry(c.name.clone()).or_default();
        for f in &c.fields {
            set.insert(f.name.clone());
        }
        for meth in &c.methods {
            set.insert(meth.name.clone());
        }
        for (n, _) in c.getters.iter().chain(c.setters.iter()) {
            set.insert(n.clone());
        }
        if let Some(ctor) = &c.constructor {
            let mut on_stmt = |_: &Stmt| {};
            // `this.k = v` lowers to a PropertySet in strict code and a
            // PutValueSet in sloppy JS; both name an own instance key.
            let mut on_expr = |e: &Expr| {
                if let Some(k) = this_store_key(e) {
                    set.insert(k);
                }
            };
            walk_stmts(&ctor.body, &mut on_stmt, &mut on_expr);
        }
    }
    let names: Vec<String> = own.keys().cloned().collect();
    let mut out = Vec::new();
    for c in names {
        let mut set = own.get(&c).cloned().unwrap_or_default();
        let mut p = parent.get(&c);
        for _ in 0..16 {
            let Some(pc) = p else { break };
            if let Some(ps) = own.get(pc) {
                set.extend(ps.iter().cloned());
            }
            p = parent.get(pc);
        }
        out.push((c, set));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `F.prototype` where `F` is a local: the local.
fn proto_of(e: &Expr) -> Option<LocalId> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } if property == "prototype" => match object.as_ref() {
            Expr::LocalGet(l) => Some(*l),
            _ => None,
        },
        _ => None,
    }
}

/// `this.k = v` (strict `PropertySet` or sloppy `PutValueSet`): `k`.
fn this_store_key(e: &Expr) -> Option<String> {
    match e {
        Expr::PropertySet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => Some(property.clone()),
        Expr::PutValueSet { target, key, .. } if matches!(target.as_ref(), Expr::This) => {
            match key.as_ref() {
                Expr::String(k) => Some(k.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn collect_param_uses(units: &[Unit<'_>]) -> HashMap<LocalId, Vec<String>> {
    let mut out: HashMap<LocalId, Vec<String>> = HashMap::new();
    for u in units {
        let ids: HashSet<LocalId> = u.params.iter().map(|p| p.id).collect();
        let mut on_stmt = |_: &Stmt| {};
        let mut on_expr = |e: &Expr| {
            let (object, property) = match e {
                Expr::PropertyGet {
                    object, property, ..
                }
                | Expr::PropertySet {
                    object, property, ..
                }
                | Expr::PropertyUpdate {
                    object, property, ..
                } => (object.as_ref(), property),
                _ => return,
            };
            if let Expr::LocalGet(id) = object {
                if ids.contains(id) {
                    let v = out.entry(*id).or_default();
                    if !v.contains(property) {
                        v.push(property.clone());
                    }
                }
            }
        };
        walk_stmts(u.body, &mut on_stmt, &mut on_expr);
    }
    out
}

/// `PERRY_FNINFER_TRACE=<function name>`: print each round's parameter
/// sources for that function (diagnostics).
fn trace_name() -> Option<&'static str> {
    use std::sync::OnceLock;
    static N: OnceLock<Option<String>> = OnceLock::new();
    N.get_or_init(|| std::env::var("PERRY_FNINFER_TRACE").ok())
        .as_deref()
}

fn finalize(t: Option<&Type>) -> Type {
    match t {
        None | Some(Type::Never) => Type::Any,
        Some(t) => t.clone(),
    }
}

fn widen_unstable<K: std::hash::Hash + Eq + Clone>(
    cur: &mut HashMap<K, Type>,
    prev: &HashMap<K, Type>,
) {
    for (k, v) in cur.iter_mut() {
        if prev.get(k) != Some(v) {
            *v = Type::Any;
        }
    }
}

fn this_of(cx: &Ctx<'_>, st: &State, ui: usize) -> Option<Type> {
    let u = &cx.units[ui];
    if let Some(c) = &u.this_class {
        return Some(Type::Named(c.clone()));
    }
    if let Some(p) = u.this_from_parent {
        return st.this_types.get(p).cloned().flatten();
    }
    None
}

/// One Jacobi round: read `st`, produce the next state.
fn round(cx: &Ctx<'_>, st: &State, m: &Module) -> State {
    let n = cx.units.len();
    let mut next = State {
        this_types: (0..n).map(|ui| this_of(cx, st, ui)).collect(),
        ..Default::default()
    };
    // Call-site argument types per callee, gathered while reading `st`.
    let mut args_seen: HashMap<FuncId, Vec<Type>> = HashMap::new();
    let mut hints: HashMap<LocalId, Type> = HashMap::new();

    let mut visit_body = |body: &[Stmt],
                          this: Option<Type>,
                          this_class: Option<&str>,
                          next: &mut State,
                          ret_of: Option<FuncId>| {
        let env = Env {
            st,
            this,
            never: Type::Never,
        };
        let mut lets: Vec<(LocalId, Type)> = Vec::new();
        let mut rets: Vec<Type> = Vec::new();
        let mut falls_off = true;
        let mut on_stmt = |s: &Stmt| match s {
            // `var x` hoisting placeholder (`let x = undefined` followed by
            // the real `let x = v` of the same local): perry's lowering
            // already types the local from the real initializer.
            Stmt::Let {
                id,
                init: Some(Expr::Undefined),
                ..
            } if cx.multi_let.contains(id) => {}
            Stmt::Let { id, init, .. } => {
                let t = match init {
                    Some(e) => ty_of(e, &env, cx),
                    None => Type::Void,
                };
                lets.push((*id, t));
            }
            Stmt::Return(e) => {
                rets.push(match e {
                    Some(e) => ty_of(e, &env, cx),
                    None => Type::Void,
                });
            }
            Stmt::Try {
                catch:
                    Some(CatchClause {
                        param: Some((id, _)),
                        ..
                    }),
                ..
            } => lets.push((*id, Type::Any)),
            _ => {}
        };
        let mut sets: Vec<(LocalId, Type)> = Vec::new();
        let mut gsets: Vec<(GlobalId, Type)> = Vec::new();
        let mut fsets: Vec<((String, String), Type)> = Vec::new();
        let mut calls: Vec<(FuncId, Vec<Type>)> = Vec::new();
        let mut hint_v: Vec<(LocalId, Type)> = Vec::new();
        let mut on_expr = |e: &Expr| match e {
            Expr::LocalSet(id, v) => sets.push((*id, ty_of(v, &env, cx))),
            Expr::Update { id, .. } => sets.push((*id, Type::Number)),
            Expr::GlobalSet(id, v) => gsets.push((*id, ty_of(v, &env, cx))),
            Expr::PropertySet { value, .. } | Expr::PutValueSet { value, .. }
                if this_class.is_some() && this_store_key(e).is_some() =>
            {
                let c = this_class.unwrap_or_default();
                let k = this_store_key(e).unwrap_or_default();
                fsets.push(((c.to_string(), k), ty_of(value, &env, cx)));
            }
            Expr::Call { callee, args, .. } => {
                if let Some(fid) = call_target(callee, &env, cx) {
                    calls.push((fid, args.iter().map(|a| ty_of(a, &env, cx)).collect()));
                    // A local handed to a callee whose parameter is concrete
                    // is probably of that type (callee demand).
                    if let Some(ps) = st.params.get(&fid) {
                        for (a, t) in args.iter().zip(ps.iter()) {
                            if let Expr::LocalGet(id) = a {
                                if is_concrete(t) {
                                    hint_v.push((*id, t.clone()));
                                }
                            }
                        }
                    }
                }
            }
            Expr::Compare { op, left, right }
                if matches!(
                    op,
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                ) =>
            {
                for (side, other) in [(left, right), (right, left)] {
                    if let Expr::LocalGet(id) = side.as_ref() {
                        if is_numeric(&ty_of(other, &env, cx)) {
                            hint_v.push((*id, Type::Number));
                        }
                    }
                }
            }
            Expr::NewDynamic { callee, args, .. } => {
                if let Expr::LocalGet(l) = callee.as_ref() {
                    if let Some(fid) = cx.closure_of_local.get(l) {
                        calls.push((*fid, args.iter().map(|a| ty_of(a, &env, cx)).collect()));
                    }
                }
            }
            Expr::New {
                class_name, args, ..
            } => {
                if let Some(fid) = cx.method(class_name, "constructor") {
                    calls.push((fid, args.iter().map(|a| ty_of(a, &env, cx)).collect()));
                }
            }
            Expr::Binary { op, left, right } if !matches!(op, BinaryOp::Add) => {
                for side in [left, right] {
                    if let Expr::LocalGet(id) = side.as_ref() {
                        hint_v.push((*id, Type::Number));
                    }
                }
            }
            _ => {}
        };
        walk_stmts(body, &mut on_stmt, &mut on_expr);
        if let Some(Stmt::Return(_)) | Some(Stmt::Throw(_)) = body.last() {
            falls_off = false;
        }
        for (id, t) in lets.into_iter().chain(sets) {
            add(&mut next.locals, id, t);
        }
        for (id, t) in gsets {
            let e = next.globals.entry(id).or_insert(Type::Never);
            *e = join(e, &t);
        }
        for (k, t) in fsets {
            let e = next.fields.entry(k).or_insert(Type::Never);
            *e = join(e, &t);
        }
        for (fid, a) in calls {
            let e = args_seen.entry(fid).or_default();
            for (i, t) in a.into_iter().enumerate() {
                if e.len() <= i {
                    e.push(Type::Never);
                }
                // An `Any` argument is "unknown", not "polymorphic": the
                // entry check already covers unknown callers.
                if !matches!(t, Type::Any | Type::Unknown) {
                    e[i] = join(&e[i], &t);
                }
            }
        }
        for (id, t) in hint_v {
            let e = hints.entry(id).or_insert(Type::Never);
            *e = join(e, &t);
        }
        if let Some(fid) = ret_of {
            let mut r = Type::Never;
            for t in rets {
                r = join(&r, &t);
            }
            if falls_off {
                r = join(&r, &Type::Void);
            }
            next.rets.insert(fid, r);
        }
    };

    visit_body(&m.init, None, None, &mut next, None);
    for (ui, u) in cx.units.iter().enumerate() {
        let this = this_of(cx, st, ui);
        visit_body(u.body, this, u.this_class.as_deref(), &mut next, Some(u.id));
        if u.is_async || u.is_generator {
            next.rets.insert(u.id, Type::Any);
        }
    }
    // Parameters: declared type, call-site args, defaults, use hints.
    for u in cx.units.iter() {
        let seen = args_seen.get(&u.id);
        let mut ps = Vec::with_capacity(u.params.len());
        for (i, p) in u.params.iter().enumerate() {
            let mut t = Type::Never;
            if p.is_rest {
                t = Type::Array(Box::new(Type::Any));
            } else {
                if !matches!(p.ty, Type::Any | Type::Unknown | Type::TypeVar(_)) {
                    t = join(&t, &p.ty);
                }
                if let Some(s) = seen {
                    // a call that passes fewer args passes undefined
                    let a = s.get(i).cloned().unwrap_or(Type::Void);
                    t = join(&t, &a);
                }
                if let Some(d) = &p.default {
                    let env = Env {
                        st,
                        this: None,
                        never: Type::Never,
                    };
                    // the default replaces undefined
                    let strip: Vec<Type> = members(&t)
                        .into_iter()
                        .filter(|m| !matches!(m, Type::Void))
                        .collect();
                    let mut acc = Type::Never;
                    for m in strip {
                        acc = join(&acc, &m);
                    }
                    t = join(&acc, &ty_of(d, &env, cx));
                }
                // The body's own evidence (member names, string/array
                // methods) wins over a call-site join that is too wide: the
                // join is narrowed to the members the body can use, or
                // replaced when it is `Any` (too polymorphic) or empty.
                let used = cx
                    .param_uses
                    .get(&p.id)
                    .and_then(|u| guess_from_uses(cx, u));
                if let Some(g) = &used {
                    t = narrow_to_use(&t, g);
                }
                if matches!(t, Type::Never) {
                    if let Some(h) = hints.get(&p.id) {
                        t = h.clone();
                    }
                }
            }
            if matches!(t, Type::Never) {
                t = Type::Any;
            }
            if trace_name().is_some_and(|n| n == u.name) {
                eprintln!(
                    "fninfer trace {} param {} = {:?}; seen={:?} hint={:?} uses={:?}",
                    u.name,
                    p.name,
                    t,
                    seen.and_then(|s| s.get(i)),
                    hints.get(&p.id),
                    cx.param_uses.get(&p.id)
                );
            }
            add(&mut next.locals, p.id, t.clone());
            ps.push(t);
        }
        next.params.insert(u.id, ps);
    }
    next
}

fn unit_stats(cx: &Ctx<'_>, st: &State, ui: usize) -> FnStats {
    let u = &cx.units[ui];
    let env = Env {
        st,
        this: st.this_types[ui].clone(),
        never: Type::Never,
    };
    let param_ids: HashSet<LocalId> = u.params.iter().map(|p| p.id).collect();
    let mut hot: HashSet<LocalId> = HashSet::new();
    let mut s = FnStats {
        params: u.params.len(),
        ..Default::default()
    };
    let mut on_stmt = |_: &Stmt| {};
    let mut on_expr = |e: &Expr| {
        let recv = match e {
            Expr::PropertyGet { object, .. }
            | Expr::PropertySet { object, .. }
            | Expr::PropertyUpdate { object, .. }
            | Expr::IndexGet { object, .. }
            | Expr::IndexSet { object, .. } => Some(object.as_ref()),
            _ => None,
        };
        if let Some(o) = recv {
            s.prop_ops += 1;
            if is_receiver_concrete(&ty_of(o, &env, cx)) {
                s.prop_ops_concrete += 1;
            }
            if let Expr::LocalGet(id) = o {
                if param_ids.contains(id) {
                    hot.insert(*id);
                }
            }
        }
        let operands = match e {
            Expr::Binary { left, right, .. } => Some((left, right)),
            Expr::Compare { op, left, right }
                if matches!(
                    op,
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                ) =>
            {
                Some((left, right))
            }
            _ => None,
        };
        if let Some((l, r)) = operands {
            s.arith_ops += 1;
            let lt = ty_of(l, &env, cx);
            let rt = ty_of(r, &env, cx);
            if is_prim_concrete(&lt) && is_prim_concrete(&rt) {
                s.arith_ops_concrete += 1;
            }
            for side in [l, r] {
                if let Expr::LocalGet(id) = side.as_ref() {
                    if param_ids.contains(id) {
                        hot.insert(*id);
                    }
                }
            }
        }
    };
    walk_stmts(u.body, &mut on_stmt, &mut on_expr);
    for p in u.params {
        let t = finalize(st.locals.get(&p.id));
        let c = is_concrete(&t);
        if c {
            s.params_concrete += 1;
        }
        if hot.contains(&p.id) {
            s.hot_params += 1;
            if c {
                s.hot_params_concrete += 1;
            }
        }
    }
    s
}

/// FNV-1a pair over UTF-16 code units — the same key the node oracle
/// computes from `Function.prototype.toString` text.
pub fn source_key(text: &str) -> String {
    let mut h1: u32 = 0x811c9dc5;
    let mut h2: u32 = 0x01000193;
    let mut len = 0usize;
    for c in text.encode_utf16() {
        h1 = (h1 ^ c as u32).wrapping_mul(16777619);
        h2 = (h2 ^ c as u32).wrapping_mul(2246822519);
        len += 1;
    }
    format!("{h1:08x}{h2:08x}:{len}")
}

/// Dump lines (TSV) for one module: one per function with source text.
pub fn dump_module(path: &str, m: &Module, inf: &ModuleInference, out: &mut String) {
    ANON_FIELDS.with(|a| {
        let mut a = a.borrow_mut();
        a.clear();
        for c in &m.classes {
            if c.name.starts_with("__AnonShape_") {
                let keys: Vec<&str> = c.fields.iter().map(|f| f.name.as_str()).collect();
                a.insert(c.name.clone(), keys.join(","));
            }
        }
    });
    for (fid, sig) in &inf.sigs {
        let key = m
            .closure_source_text
            .get(fid)
            .map(|s| source_key(&s.text))
            .unwrap_or_else(|| "-".into());
        let st = inf.stats.get(fid).cloned().unwrap_or_default();
        let (name, kind) = inf.names.get(fid).cloned().unwrap_or_default();
        out.push_str(&format!(
            "F\t{key}\t{path}\t{}\t{name}\t{kind}\t{}\t{}\t{}\t{}/{}\t{}/{}\t{}/{}\t{}/{}\n",
            fid,
            sig.params
                .iter()
                .map(fmt_type)
                .collect::<Vec<_>>()
                .join("|"),
            fmt_type(&sig.ret),
            fmt_type(&sig.this),
            st.params_concrete,
            st.params,
            st.hot_params_concrete,
            st.hot_params,
            st.prop_ops_concrete,
            st.prop_ops,
            st.arith_ops_concrete,
            st.arith_ops,
        ));
    }
    callee::dump_callees(path, &inf.callees, out);
    layout::dump_layouts(path, &inf.layouts, out);
}
