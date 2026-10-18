//! Builtin summaries: what runtime-provided objects and functions do with
//! the values they receive (design 3.2, "runtime builtin call").
//!
//! Every summary over-approximates the runtime entry it stands for. Anything
//! without a summary falls back to `unknown_method`: arguments escape to ⊤
//! and the result is ⊤. A missing summary costs precision, never soundness.

use crate::model::*;

/// Enumerate own values without following prototype methods. A callable can
/// have toJSON and be passed to the replacer, but its properties are not visited.
pub fn own_values(m: &mut Model, t: Tok, dst: NodeId, json: bool) {
    if t == TOP {
        m.add_tok(dst, TOP);
        m.add_prim(dst, prim::ALL);
        return;
    }
    if json
        && matches!(
            m.toks[t as usize],
            TokKind::Func(_)
                | TokKind::Bound(_)
                | TokKind::ClassObj(_)
                | TokKind::BuiltinFn
                | TokKind::BuiltinMethod(_, _)
                | TokKind::Builtin(
                    BuiltinNs::Object
                        | BuiltinNs::Array
                        | BuiltinNs::Promise
                        | BuiltinNs::Error
                        | BuiltinNs::Symbol
                        | BuiltinNs::Number
                        | BuiltinNs::String
                        | BuiltinNs::Boolean
                        | BuiltinNs::Function
                        | BuiltinNs::Map
                        | BuiltinNs::Set
                )
        )
    {
        return;
    }
    m.own_values(t, dst);
}

/// Global names the `GlobalGet(0)` sentinel resolves to.
fn ns_by_name(name: &str) -> Option<BuiltinNs> {
    Some(match name {
        "Object" => BuiltinNs::Object,
        "Reflect" => BuiltinNs::Reflect,
        "Array" => BuiltinNs::Array,
        "JSON" => BuiltinNs::Json,
        "Math" => BuiltinNs::Math,
        "Promise" => BuiltinNs::Promise,
        "console" => BuiltinNs::Console,
        "globalThis" | "global" | "window" | "self" => BuiltinNs::GlobalThis,
        "process" => BuiltinNs::Process,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" | "AggregateError" => BuiltinNs::Error,
        "Symbol" => BuiltinNs::Symbol,
        "Number" | "BigInt" => BuiltinNs::Number,
        "String" => BuiltinNs::String,
        "Boolean" => BuiltinNs::Boolean,
        "Function" => BuiltinNs::Function,
        "Map" | "WeakMap" => BuiltinNs::Map,
        "Set" | "WeakSet" => BuiltinNs::Set,
        _ => return None,
    })
}

const CONSOLE_METHODS: &[&str] = &[
    "log",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "dir",
    "dirxml",
    "table",
    "time",
    "timeEnd",
    "timeLog",
    "assert",
    "group",
    "groupCollapsed",
    "groupEnd",
    "count",
    "countReset",
];

/// E14: interpreted code may run; it can patch `Object.prototype`.
pub fn set_dyn_eval(m: &mut Model, why: &str) {
    if m.dyn_eval.is_none() {
        m.dyn_eval = Some(why.to_string());
        m.rel(|| format!("dyneval {}", why.replace(' ', "\\s")));
    }
}

fn unknown_method(m: &mut Model, call: CallId, why: &str) {
    let c = m.calls[call as usize].clone();
    let r = m.reason(why);
    for &a in c.args.iter().chain(c.spread.iter()) {
        m.sink_node(a, r);
    }
    // A method run on one receiver token sees that token as `this`, not
    // every token of the receiver node.
    match (m.cur_this, &c.this) {
        (Some(t), _) => m.escape(t, r),
        (None, ThisArg::Node(n)) => m.sink_node(*n, r),
        (None, ThisArg::Tok(t)) => m.escape(*t, r),
        (None, ThisArg::Undef) => {}
    }
    m.add_top(c.ret, |_| format!("result: {why}"));
    m.add_prim(c.ret, prim::ALL);
}

/// Stores on the collapsed builtin prototype already escape their values.
/// Depend on those same fields, including computed writes, to revoke the
/// argument-role summary when a method is replaced (even after the call was
/// first processed). No runtime state is introduced.
pub fn overwritten(m: &mut Model, call: CallId) {
    let prev = m.cur_this.take();
    unknown_method(m, call, "argument of overwritten builtin method");
    m.cur_this = prev;
}

fn watch_override(m: &mut Model, holder: Tok, key: Key, call: CallId) {
    let f = match key {
        Key::Named(a) => m.field(holder, a),
        Key::Any => m.all_fields(holder),
    };
    m.cons(f, Cons::BuiltinOverride { call });
    let opaque = match key {
        Key::Named(a) => m.field(TOP, a),
        Key::Any => m.field_any(TOP),
    };
    m.cons(opaque, Cons::BuiltinOverride { call });
}

fn implicit_args(m: &mut Model, call: CallId, why: &str) {
    let c = m.calls[call as usize].clone();
    let r = m.reason(why);
    for &a in c.args.iter().chain(c.spread.iter()) {
        m.cons(a, Cons::Implicit { reason: r });
    }
}

fn ret_prim(m: &mut Model, call: CallId, p: u16) {
    let ret = m.calls[call as usize].ret;
    m.add_prim(ret, p);
}

fn ret_tok(m: &mut Model, call: CallId, t: Tok) {
    let ret = m.calls[call as usize].ret;
    m.add_tok(ret, t);
}

fn arg(m: &Model, call: CallId, i: usize) -> Option<NodeId> {
    m.calls[call as usize].args.get(i).copied()
}

/// The elements of every array argument plus every spread array.
fn all_arg_values(m: &mut Model, call: CallId) -> NodeId {
    let c = m.calls[call as usize].clone();
    let out = m.node_l("argvals");
    for &a in &c.args {
        m.edge(a, out);
    }
    for &s in &c.spread {
        m.cons(
            s,
            Cons::Load {
                key: Key::Any,
                dst: out,
            },
        );
    }
    out
}

/// A per-call-site template (solve-time births: promises, arrays …).
fn site_tmpl(m: &mut Model, call: CallId, what: &'static str) -> Tok {
    // Keyed by the SOURCE call site: calls derived while solving (bindings
    // through relays, protocol touches) share their origin's template.
    let site = m.calls[call as usize].site;
    let key = (site, what);
    if let Some(&t) = m.site_tmpls.get(&key) {
        return t;
    }
    let tm = m.tmpls.len() as TmplId;
    let kind = if what == "Array" {
        TmplKind::Arr {
            module: u32::MAX,
            site,
        }
    } else {
        TmplKind::BObj {
            module: u32::MAX,
            site,
            what,
        }
    };
    m.tmpls.push(Template {
        kind,
        display: format!("{what}@call{site}"),
        birth_keys: vec![],
        definite: vec![],
        parent: None,
        ctor: None,
        native_parent: None,
        accessor_keys: vec![],
        has_computed_members: false,
        field_init: None,
        proto_members: vec![],
    });
    let t = m.tok(TokKind::Obj(tm));
    m.site_tmpls.insert(key, t);
    t
}

/// A synthetic resolver function: its first parameter flows into `dst`.
pub fn resolver(m: &mut Model, dst: NodeId) -> Tok {
    if let Some(&t) = m.resolvers.get(&dst) {
        return t;
    }
    let fx = m.funcs.len() as FnIdx;
    let tok = m.tok(TokKind::Func(fx));
    let this = m.node_l("this(resolver)");
    let ret = m.node_l("ret(resolver)");
    let args_all = m.node_l("args(resolver)");
    let p = m.node_l("resolver-param");
    m.edge(p, dst);
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: u32::MAX,
        name: "<resolver>".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all,
        has_arguments: false,
        token: tok,
        ctor_of: None,
        lexical_this: true,
    });
    m.add_prim(ret, prim::UNDEF);
    m.resolvers.insert(dst, tok);
    tok
}

/// The settled-value field of a promise token.
fn promise_value(m: &mut Model, p: Tok) -> NodeId {
    let a = m.atom("#value");
    m.field(p, a)
}

/// `v` adopted by a promise: the value, the settled value of promise
/// tokens in it, and whatever a thenable's `then` resolves.
pub fn adopt(m: &mut Model, v: NodeId, dst: NodeId) {
    m.edge(v, dst);
    let a = m.atom("#value");
    m.cons(
        v,
        Cons::Load {
            key: Key::Named(a),
            dst,
        },
    );
    let res = resolver(m, dst);
    let thrown = m.thrown;
    let rej = resolver(m, thrown);
    let rn = m.node_l("resolver");
    m.add_tok(rn, res);
    let jn = m.node_l("rejecter");
    m.add_tok(jn, rej);
    let ret = m.node_l("then-ret");
    let call = m.calls.len() as CallId;
    m.calls.push(CallSite {
        this: ThisArg::Node(v),
        args: vec![rn, jn],
        spread: vec![],
        ret,
        site: call,
    });
    let then = m.atom("then");
    m.cons(
        v,
        Cons::MCall {
            key: Key::Named(then),
            call,
        },
    );
}

// -------------------------------------------------------------- loads

pub fn prim_load(m: &mut Model, prim: u16, key: Key, dst: NodeId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return; // TypeError
    }
    let len = m.atom("length");
    match key {
        Key::Named(a) if a == len => m.add_prim(dst, prim::NUM),
        // The elements of a string: its characters.
        k if m.is_elem_key(k) || m.is_iter_key(k) => m.add_prim(dst, prim::STR | prim::UNDEF),
        _ => {
            m.add_prim(dst, prim::STR | prim::NUM | prim::UNDEF | prim::BOOL);
            let bf = m.tok(TokKind::BuiltinFn);
            m.add_tok(dst, bf);
        }
    }
}

pub fn tok_load(m: &mut Model, t: Tok, key: Key, dst: NodeId) {
    let name = match key {
        Key::Named(a) => Some(m.atoms[a as usize].clone()),
        Key::Any => None,
    };
    match m.toks[t as usize].clone() {
        TokKind::Builtin(BuiltinNs::Other) => match name.as_deref() {
            Some(n) => {
                if let Some(ns) = ns_by_name(n) {
                    let bt = m.tok(TokKind::Builtin(ns));
                    m.add_tok(dst, bt);
                } else if matches!(n, "undefined" | "NaN" | "Infinity") {
                    m.add_prim(dst, prim::NUM | prim::UNDEF);
                } else {
                    m.add_top(dst, |_| format!("global {n}"));
                    m.add_prim(dst, prim::ALL);
                }
            }
            None => {
                m.add_top(dst, |_| "global[computed]".into());
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Builtin(BuiltinNs::GlobalThis) => match name.as_deref() {
            Some(n) if ns_by_name(n).is_some() => {
                let bt = m.tok(TokKind::Builtin(ns_by_name(n).unwrap()));
                m.add_tok(dst, bt);
            }
            _ => {
                // Any global, the Function constructor included (E14).
                let f = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, f);
                m.add_top(dst, |_| {
                    format!("globalThis.{}", name.as_deref().unwrap_or("[computed]"))
                });
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Builtin(BuiltinNs::Process) => {
            m.add_top(dst, |_| {
                format!("process.{}", name.as_deref().unwrap_or("[computed]"))
            });
            m.add_prim(dst, prim::ALL);
        }
        TokKind::Builtin(ns) => match name.as_deref() {
            Some("prototype") => {
                let op = m.tok(TokKind::ObjectProto);
                m.add_tok(dst, op);
            }
            Some(n) if summarized_static(ns, n) => {
                let key = m.atom(n);
                let bf = m.tok(TokKind::BuiltinMethod(ns, key));
                m.add_tok(dst, bf);
            }
            _ => {
                let bf = m.tok(TokKind::BuiltinFn);
                m.add_tok(dst, bf);
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Func(f) => match name.as_deref() {
            Some("constructor") => {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
            Some("prototype") => {
                let tm = m.fn_inst_template(f);
                let p = m.tok(TokKind::Proto(tm));
                let a = m.atom("prototype");
                let fl = m.field(t, a);
                m.add_tok(fl, p);
            }
            Some("name") => m.add_prim(dst, prim::STR),
            Some("length") => m.add_prim(dst, prim::NUM),
            Some("call" | "apply" | "bind" | "toString") => {
                let bf = m.tok(TokKind::BuiltinFn);
                m.add_tok(dst, bf);
            }
            _ => m.add_prim(dst, prim::UNDEF),
        },
        TokKind::ClassObj(_) => match name.as_deref() {
            Some("constructor") => {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
            Some("name") => m.add_prim(dst, prim::STR),
            Some("length") => m.add_prim(dst, prim::NUM),
            _ => m.add_prim(dst, prim::UNDEF),
        },
        TokKind::Obj(tm) => match m.tmpls[tm as usize].kind.clone() {
            TmplKind::Arr { .. } => match name.as_deref() {
                Some("length") => m.add_prim(dst, prim::NUM),
                // A computed key may name a builtin method too.
                _ => {
                    let bf = m.tok(TokKind::BuiltinFn);
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::UNDEF | prim::NUM);
                }
            },
            TmplKind::BObj { what, .. } => match name.as_deref() {
                Some("buffer") if what == "TypedArray" => {
                    // B1: the backing buffer is observed (it can be
                    // transferred, detached or wrapped by another view).
                    let r = m.reason("B1 .buffer read");
                    m.mark_hard(t, Hard::B1Exposed, r);
                    m.add_tok(dst, t);
                }
                Some("byteOffset") if what == "TypedArray" => {
                    let r = m.reason("B1 .byteOffset read");
                    m.mark_hard(t, Hard::B1Exposed, r);
                    m.add_prim(dst, prim::NUM)
                }
                Some("size" | "length" | "byteLength" | "byteOffset") => m.add_prim(dst, prim::NUM),
                // An iterator result's own fields hold all it has.
                Some("value" | "done") if what == "IterResult" => {}
                // Builtin iterator / iterator result: a prototype method or
                // `undefined`.
                Some(_) if what == "Iterator" || what == "IterResult" => {
                    let bf = m.tok(TokKind::BuiltinFn);
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::UNDEF);
                }
                _ => {
                    let bf = m.tok(TokKind::BuiltinFn);
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::ALL);
                }
            },
            _ => {}
        },
        TokKind::PrimBox => m.add_prim(dst, prim::ALL),
        TokKind::ObjectProto if name.as_deref() == Some("constructor") || name.is_none() => {
            // Object.prototype stands for every builtin prototype here:
            // its `constructor` may be Object or Function (E14).
            for ns in [BuiltinNs::Object, BuiltinNs::Function] {
                let b = m.tok(TokKind::Builtin(ns));
                m.add_tok(dst, b);
            }
            m.add_prim(dst, prim::UNDEF);
        }
        TokKind::ModNs(_) | TokKind::ObjectProto | TokKind::Proto(_) => {
            m.add_prim(dst, prim::UNDEF)
        }
        TokKind::NativeProto(_)
        | TokKind::BuiltinFn
        | TokKind::BuiltinMethod(..)
        | TokKind::Bound(_) => {
            let bf = m.tok(TokKind::BuiltinFn);
            m.add_tok(dst, bf);
            m.add_prim(dst, prim::ALL);
            if matches!(name.as_deref(), Some("constructor") | None) {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
        }
        TokKind::Top => {}
    }
}

pub fn tok_store(m: &mut Model, t: Tok, _key: Key, src: NodeId) {
    match m.toks[t as usize] {
        // Object.prototype members are reachable from every object the
        // runtime handles: unknown code may call them with any receiver.
        TokKind::ObjectProto
        | TokKind::Builtin(_)
        | TokKind::BuiltinFn
        | TokKind::BuiltinMethod(..)
        | TokKind::ModNs(_)
        | TokKind::NativeProto(_) => {
            // E20: stored where the runtime / unmodeled code reads it.
            let r = m.reason("E20 stored on a builtin/global object");
            m.sink_node(src, r);
        }
        _ => {}
    }
}

// ------------------------------------------------------------- methods

mod primitives;
use primitives::coerce_args;
pub use primitives::{coerce, prim_method};

/// Methods on builtin tokens (the receiver's own builtin behaviour).
pub fn own_method(m: &mut Model, t: Tok, key: Key, call: CallId) {
    // Array element storage shares its wildcard field with indexed values;
    // prototype writes are tracked on the actual builtin prototype instead.
    if matches!(m.toks[t as usize], TokKind::Builtin(_)) {
        watch_override(m, t, key, call);
    }
    let summarized = match m.toks[t as usize] {
        TokKind::Builtin(_)
        | TokKind::BuiltinMethod(..)
        | TokKind::BuiltinFn
        | TokKind::Func(_)
        | TokKind::Bound(_)
        | TokKind::PrimBox => true,
        TokKind::Obj(tm) => matches!(
            m.tmpls[tm as usize].kind,
            TmplKind::Arr { .. } | TmplKind::BObj { .. }
        ),
        _ => false,
    };
    if summarized && matches!(key, Key::Named(_)) {
        let proto = m.tok(TokKind::ObjectProto);
        watch_override(m, proto, key, call);
    }
    let prev = m.cur_this.replace(t);
    own_method_inner(m, t, key, call);
    m.cur_this = prev;
}

fn own_method_inner(m: &mut Model, t: Tok, key: Key, call: CallId) {
    let name = match key {
        Key::Named(a) => Some(m.atoms[a as usize].clone()),
        Key::Any => None,
    };
    // A well-known-symbol method is a computed-key method, except the
    // iteration protocol of the builtin iterables, which is modeled.
    let iterable = match &m.toks[t as usize] {
        TokKind::PrimBox => true,
        TokKind::Obj(tm) => matches!(
            m.tmpls[*tm as usize].kind,
            TmplKind::Arr { .. }
                | TmplKind::BObj {
                    what: "Collection" | "Iterator",
                    ..
                }
        ),
        _ => false,
    };
    let name = match name {
        Some(n) if n.starts_with(WK_PREFIX) && !(iterable && n == WK_ITERATOR) => None,
        n => n,
    };
    match m.toks[t as usize].clone() {
        TokKind::Builtin(ns) => ns_method(m, ns, name.as_deref(), call),
        TokKind::Func(_) | TokKind::Bound(_) | TokKind::BuiltinMethod(..) => {
            function_method(m, t, name.as_deref(), call)
        }
        TokKind::BuiltinFn => match name.as_deref() {
            None => {}
            Some(n) => unknown_method(
                m,
                call,
                &format!("method {n} of an opaque builtin function"),
            ),
        },
        TokKind::PrimBox => array_method(m, t, name.as_deref(), call),
        TokKind::NativeProto(_) => {}
        TokKind::Obj(tm) => match m.tmpls[tm as usize].kind.clone() {
            TmplKind::Arr { .. } => array_method(m, t, name.as_deref(), call),
            TmplKind::BObj { what, .. } => bobj_method(m, t, what, name.as_deref(), call),
            _ => {}
        },
        _ => {}
    }
}

/// Builtin methods found on the prototype chain (`Object.prototype` and
/// native parent prototypes), run with `this` = `origin`.
pub fn proto_method(m: &mut Model, p: Tok, origin: Tok, key: Key, call: CallId) {
    let prev = m.cur_this.replace(origin);
    proto_method_inner(m, p, origin, key, call);
    m.cur_this = prev;
}

fn proto_method_inner(m: &mut Model, p: Tok, origin: Tok, key: Key, call: CallId) {
    let name = match key {
        Key::Named(a) => m.atoms[a as usize].clone(),
        Key::Any => return,
    };
    if name.starts_with(WK_PREFIX) {
        return;
    }
    match m.toks[p as usize].clone() {
        TokKind::ObjectProto => match name.as_str() {
            "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => {
                implicit_args(m, call, "ToPropertyKey");
                ret_prim(m, call, prim::BOOL);
            }
            "toString" | "toLocaleString" => {
                ret_prim(m, call, prim::STR);
            }
            "valueOf" => ret_tok(m, call, origin),
            "__defineGetter__" | "__defineSetter__" => {
                let r = m.reason("E5 __defineGetter__/__defineSetter__");
                m.mark_hard(origin, Hard::E5DefineProperty, r);
                m.escape(origin, r);
                unknown_method(m, call, "E5 __defineGetter__ argument");
            }
            "__lookupGetter__" | "__lookupSetter__" => {
                m.implicit(origin, 0);
                ret_tok(m, call, TOP);
            }
            _ => {}
        },
        TokKind::NativeProto(name_of) => native_method(m, &name_of, origin, &name, call),
        _ => {}
    }
}

/// Keep callable identity for static methods with argument-role summaries.
fn summarized_static(ns: BuiltinNs, name: &str) -> bool {
    match (ns, name) {
        (
            BuiltinNs::Object,
            "keys"
            | "values"
            | "entries"
            | "getOwnPropertyNames"
            | "getOwnPropertySymbols"
            | "freeze"
            | "seal"
            | "preventExtensions"
            | "isFrozen"
            | "isSealed"
            | "isExtensible"
            | "is"
            | "hasOwn"
            | "getPrototypeOf"
            | "assign",
        )
        | (BuiltinNs::Array, "from" | "of" | "isArray")
        | (BuiltinNs::Json, "parse" | "stringify")
        | (
            BuiltinNs::Number,
            "isFinite" | "isNaN" | "isInteger" | "isSafeInteger" | "parseInt" | "parseFloat",
        )
        | (BuiltinNs::String, "fromCharCode" | "fromCodePoint" | "raw") => true,
        (BuiltinNs::Math, n) => matches!(
            n,
            "random"
                | "max"
                | "min"
                | "hypot"
                | "atan2"
                | "pow"
                | "imul"
                | "abs"
                | "acos"
                | "acosh"
                | "asin"
                | "asinh"
                | "atan"
                | "atanh"
                | "cbrt"
                | "ceil"
                | "clz32"
                | "cos"
                | "cosh"
                | "exp"
                | "expm1"
                | "floor"
                | "fround"
                | "f16round"
                | "log"
                | "log10"
                | "log1p"
                | "log2"
                | "round"
                | "sign"
                | "sin"
                | "sinh"
                | "sqrt"
                | "tan"
                | "tanh"
                | "trunc"
        ),
        _ => false,
    }
}

pub fn call_static(m: &mut Model, ns: BuiltinNs, key: Atom, call: CallId) {
    let t = m.tok(TokKind::Builtin(ns));
    watch_override(m, t, Key::Named(key), call);
    let name = m.atoms[key as usize].clone();
    ns_method(m, ns, Some(&name), call);
}

fn ns_method(m: &mut Model, ns: BuiltinNs, name: Option<&str>, call: CallId) {
    if !m.calls[call as usize].spread.is_empty() {
        return unknown_method(m, call, "spread arguments of builtin method");
    }
    let Some(name) = name else {
        return;
    };
    match (ns, name) {
        (BuiltinNs::Other, n) if CONSOLE_METHODS.contains(&n) => {
            implicit_args(m, call, "console argument (formatted by the runtime)");
            ret_prim(m, call, prim::UNDEF);
        }
        (BuiltinNs::Console, _) => {
            implicit_args(m, call, "console argument (formatted by the runtime)");
            ret_prim(m, call, prim::UNDEF);
        }
        (BuiltinNs::Other, "setTimeout" | "setInterval" | "setImmediate" | "queueMicrotask") => {
            // The callback runs later with the extra arguments.
            let c = m.calls[call as usize].clone();
            if let Some(&cb) = c.args.first() {
                let ret = m.node_l("timer-ret");
                let call2 = m.calls.len() as CallId;
                m.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: c.args.iter().skip(2).copied().collect(),
                    spread: vec![],
                    ret,
                    site: call2,
                });
                m.cons(cb, Cons::Call { call: call2 });
            }
            let bt = site_tmpl(m, call, "Timer");
            ret_tok(m, call, bt);
        }
        (BuiltinNs::Other, "clearTimeout" | "clearInterval" | "clearImmediate") => {
            ret_prim(m, call, prim::UNDEF);
        }
        (
            BuiltinNs::Other,
            "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "encodeURIComponent"
            | "decodeURIComponent" | "encodeURI" | "decodeURI" | "escape" | "unescape" | "atob"
            | "btoa" | "String" | "Number" | "Boolean" | "BigInt",
        ) => {
            implicit_args(m, call, "ToPrimitive");
            ret_prim(m, call, prim::ALL);
        }
        (BuiltinNs::Object, "keys" | "getOwnPropertyNames" | "getOwnPropertySymbols") => {
            let pb = site_tmpl(m, call, "keys");
            let fa = m.field_any(pb);
            m.add_prim(fa, prim::STR | prim::SYMBOL);
            ret_tok(m, call, pb);
        }
        (BuiltinNs::Object, "values" | "entries") => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            if let Some(o) = arg(m, call, 0) {
                m.cons(
                    o,
                    Cons::OwnValues {
                        dst: fa,
                        json: false,
                    },
                );
            }
            m.add_prim(fa, prim::STR);
            if name == "entries" {
                m.add_tok(fa, arr);
            }
            ret_tok(m, call, arr);
        }
        (BuiltinNs::Object, "freeze" | "seal" | "preventExtensions") => {
            if let Some(o) = arg(m, call, 0) {
                let r = m.reason("E8 Object.freeze (as a value call)");
                m.cons(
                    o,
                    Cons::Hard {
                        kind: Hard::E8Freeze,
                        reason: r,
                    },
                );
                let ret = m.calls[call as usize].ret;
                m.edge(o, ret);
            }
        }
        (BuiltinNs::Object, "isFrozen" | "isSealed" | "isExtensible" | "is" | "hasOwn") => {
            if name == "hasOwn" {
                coerce_args(m, call, &[1]);
            }
            ret_prim(m, call, prim::BOOL);
        }
        (BuiltinNs::Object, "getPrototypeOf") | (BuiltinNs::Reflect, "getPrototypeOf") => {
            if let Some(o) = arg(m, call, 0) {
                let ret = m.calls[call as usize].ret;
                m.cons(o, Cons::ProtoOf { dst: ret });
            }
        }
        (BuiltinNs::Object, "assign") => {
            let c = m.calls[call as usize].clone();
            if let Some(&t) = c.args.first() {
                let vals = m.node_l("assign-vals");
                for &s in c.args.iter().skip(1) {
                    m.cons(
                        s,
                        Cons::Load {
                            key: Key::Any,
                            dst: vals,
                        },
                    );
                }
                for &s in &c.spread {
                    let inner = m.node_l("assign-src");
                    m.cons(
                        s,
                        Cons::Load {
                            key: Key::Any,
                            dst: inner,
                        },
                    );
                    m.cons(
                        inner,
                        Cons::Load {
                            key: Key::Any,
                            dst: vals,
                        },
                    );
                }
                let site = m.anon_post_site("Object.assign (value call)");
                m.cons(
                    t,
                    Cons::Store {
                        key: Key::Any,
                        src: vals,
                        site,
                    },
                );
                m.edge(t, c.ret);
            }
        }
        (BuiltinNs::Array, "isArray") => ret_prim(m, call, prim::BOOL),
        (BuiltinNs::Array, "from" | "of") => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            if name == "of" {
                let vals = all_arg_values(m, call);
                m.edge(vals, fa);
            } else if let Some(src) = arg(m, call, 0) {
                let vals = iterate(m, call, src);
                if let Some(cb) = arg(m, call, 1) {
                    let num = m.node_l("from-index");
                    m.add_prim(num, prim::NUM);
                    let this = arg(m, call, 2).map(ThisArg::Node).unwrap_or(ThisArg::Undef);
                    let r = cb_call(m, cb, this, vec![vals, num]);
                    m.edge(r, fa);
                    // An optional mapper may also be undefined.
                    m.edge(vals, fa);
                } else {
                    m.edge(vals, fa);
                }
            }
            ret_tok(m, call, arr);
        }
        (BuiltinNs::Json, "stringify") => {
            // Traverse values (not array methods), including property getters.
            let values = m.node_l("json-values");
            if let Some(root) = arg(m, call, 0) {
                m.edge(root, values);
            }
            m.cons(
                values,
                Cons::OwnValues {
                    dst: values,
                    json: true,
                },
            );
            let key = m.node_l("json-key");
            m.add_prim(key, prim::STR);
            let converted = m.node_l("json-toJSON");
            let call2 = m.calls.len() as CallId;
            m.calls.push(CallSite {
                this: ThisArg::Node(values),
                args: vec![key],
                spread: vec![],
                ret: converted,
                site: m.calls[call as usize].site,
            });
            let to_json = Key::Named(m.atom("toJSON"));
            m.cons(
                values,
                Cons::MCall {
                    key: to_json,
                    call: call2,
                },
            );
            m.edge(converted, values);
            if let Some(cb) = arg(m, call, 1) {
                let holder = site_tmpl(m, call, "json-holder");
                let hf = m.field_any(holder);
                m.edge(values, hf);
                let holders = m.node_l("json-holders");
                m.edge(values, holders);
                m.add_tok(holders, holder);
                let ret = cb_call(m, cb, ThisArg::Node(holders), vec![key, values]);
                m.edge(ret, values);
            }
            coerce_args(m, call, &[2]);
            ret_prim(m, call, prim::STR | prim::UNDEF);
        }
        (BuiltinNs::Json, "parse") => {
            coerce_args(m, call, &[0]);
            let j = site_tmpl(m, call, "json");
            let fa = m.field_any(j);
            m.add_prim(fa, prim::ALL);
            m.add_tok(fa, j);
            ret_tok(m, call, j);
            ret_prim(m, call, prim::ALL);
            if let Some(rv) = arg(m, call, 1) {
                // reviver(key, value) with `this` = the holder
                let ret = m.calls[call as usize].ret;
                let call2 = m.calls.len() as CallId;
                let s = m.node_l("str");
                m.add_prim(s, prim::STR);
                let context = site_tmpl(m, call, "reviver-context");
                let source_key = m.atom("source");
                let source = m.field(context, source_key);
                m.add_prim(source, prim::STR | prim::UNDEF);
                let context_node = m.node_l("reviver-context");
                m.add_tok(context_node, context);
                m.calls.push(CallSite {
                    this: ThisArg::Tok(j),
                    args: vec![s, fa, context_node],
                    spread: vec![],
                    ret,
                    site: call2,
                });
                m.cons(rv, Cons::Call { call: call2 });
                m.edge(ret, fa);
            }
        }
        (BuiltinNs::Number, "isFinite" | "isInteger" | "isNaN" | "isSafeInteger") => {
            ret_prim(m, call, prim::BOOL);
        }
        (BuiltinNs::Math, _) | (BuiltinNs::Number, _) | (BuiltinNs::String, _) => {
            if !primitives::static_method(m, ns, name, call) {
                unknown_method(
                    m,
                    call,
                    &format!("argument of unmodeled builtin {ns:?}.{name}"),
                );
            }
        }
        (BuiltinNs::Symbol, _) => {
            implicit_args(m, call, "ToString");
            ret_prim(m, call, prim::SYMBOL | prim::STR | prim::UNDEF);
        }
        (BuiltinNs::Promise, "resolve" | "reject") => {
            let p = site_tmpl(m, call, "Promise");
            let v = promise_value(m, p);
            if let Some(a) = arg(m, call, 0) {
                if name == "resolve" {
                    adopt(m, a, v);
                } else {
                    let thrown = m.thrown;
                    m.edge(a, thrown);
                }
            } else {
                m.add_prim(v, prim::UNDEF);
            }
            ret_tok(m, call, p);
        }
        (BuiltinNs::Promise, "all" | "allSettled" | "race" | "any") => {
            let p = site_tmpl(m, call, "Promise");
            let v = promise_value(m, p);
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            let vals = all_arg_values(m, call);
            let items = m.node_l("promise-items");
            m.cons(
                vals,
                Cons::Load {
                    key: Key::Any,
                    dst: items,
                },
            );
            let settled = m.node_l("settled");
            adopt(m, items, settled);
            m.edge(settled, fa);
            if name == "allSettled" {
                // {status, value|reason} records
                let rec = site_tmpl(m, call, "settledRecord");
                let rfa = m.field_any(rec);
                m.edge(settled, rfa);
                let thrown = m.thrown;
                m.edge(thrown, rfa);
                m.add_prim(rfa, prim::STR);
                m.add_tok(fa, rec);
            }
            if name == "all" || name == "allSettled" {
                m.add_tok(v, arr);
            } else {
                m.edge(settled, v);
            }
            ret_tok(m, call, p);
        }
        (BuiltinNs::Reflect, "ownKeys" | "has") => {
            implicit_args(m, call, "ToPropertyKey");
            let pb = m.tok(TokKind::PrimBox);
            ret_tok(m, call, pb);
            ret_prim(m, call, prim::BOOL);
        }
        (BuiltinNs::Error, "captureStackTrace") => {
            // E21: a writer of `stack` on the target. The runtime installs a
            // lazy `stack` GETTER (`install_lazy_stack_accessor`), an
            // accessor definition like `defineProperty` (E5): the slot no
            // longer holds the value, so the target is HARD.
            if let Some(o) = arg(m, call, 0) {
                let r = m.reason("E21 Error.captureStackTrace installs a stack accessor");
                m.cons(
                    o,
                    Cons::Hard {
                        kind: Hard::E5DefineProperty,
                        reason: r,
                    },
                );
                let a = m.atom("stack");
                let s = m.node_l("stack");
                m.add_prim(s, prim::STR);
                let site = m.anon_post_site("Error.captureStackTrace");
                m.cons(
                    o,
                    Cons::Store {
                        key: Key::Named(a),
                        src: s,
                        site,
                    },
                );
            }
            ret_prim(m, call, prim::UNDEF);
        }
        (BuiltinNs::Function, "call" | "apply" | "bind") => {
            // `Function.call(...)`: the Function constructor runs (E14).
            set_dyn_eval(m, "E14 Function(...) value call");
            unknown_method(m, call, "E14 dynamic code");
        }
        (BuiltinNs::Function, _) => {
            // Not a method of the Function constructor (TypeError).
        }
        (_, n) => unknown_method(m, call, &format!("argument of builtin {ns:?}.{n}")),
    }
}

pub fn call_builtin_ns(m: &mut Model, ns: BuiltinNs, call: CallId) {
    match ns {
        BuiltinNs::Function => {
            set_dyn_eval(m, "E14 Function(...) call");
            unknown_method(m, call, "E14 dynamic code");
        }
        BuiltinNs::Error => {
            implicit_args(m, call, "Error message");
            let e = error_tok(m, call);
            ret_tok(m, call, e);
        }
        BuiltinNs::Boolean => ret_prim(m, call, prim::BOOL),
        BuiltinNs::String | BuiltinNs::Number | BuiltinNs::Symbol => {
            coerce_args(m, call, &[0]);
            ret_prim(
                m,
                call,
                match ns {
                    BuiltinNs::String => prim::STR,
                    BuiltinNs::Symbol => prim::SYMBOL,
                    _ => prim::NUM | prim::BIGINT,
                },
            );
        }
        BuiltinNs::Object => {
            let c = m.calls[call as usize].clone();
            for &a in &c.args {
                m.edge(a, c.ret);
            }
            let o = site_tmpl(m, call, "Object()");
            ret_tok(m, call, o);
        }
        BuiltinNs::Array => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            let vals = all_arg_values(m, call);
            m.edge(vals, fa);
            ret_tok(m, call, arr);
        }
        _ => unknown_method(m, call, &format!("call of builtin {ns:?}")),
    }
}

fn error_tok(m: &mut Model, call: CallId) -> Tok {
    let e = site_tmpl(m, call, "Error");
    for k in ["message", "stack", "name"] {
        let a = m.atom(k);
        let f = m.field(e, a);
        m.add_prim(f, prim::STR);
    }
    e
}

pub fn new_builtin_ns(m: &mut Model, ns: BuiltinNs, call: CallId) {
    match ns {
        BuiltinNs::Map | BuiltinNs::Set => {
            let c = site_tmpl(m, call, "Collection");
            let fa = m.field_any(c);
            if let Some(src) = arg(m, call, 0) {
                let vals = iterate(m, call, src);
                if ns == BuiltinNs::Map {
                    // Entries `[key, value]`: the collection holds both.
                    let ek = m.elem_key();
                    m.cons(vals, Cons::Load { key: ek, dst: fa });
                } else {
                    m.edge(vals, fa);
                }
            }
            ret_tok(m, call, c);
        }
        BuiltinNs::Promise => {
            let p = site_tmpl(m, call, "Promise");
            let v = promise_value(m, p);
            if let Some(ex) = arg(m, call, 0) {
                let res = resolver(m, v);
                let thrown = m.thrown;
                let rej = resolver(m, thrown);
                let rn = m.node_l("resolver");
                m.add_tok(rn, res);
                let jn = m.node_l("rejecter");
                m.add_tok(jn, rej);
                let ret = m.node_l("executor-ret");
                let call2 = m.calls.len() as CallId;
                m.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![rn, jn],
                    spread: vec![],
                    ret,
                    site: call2,
                });
                m.cons(ex, Cons::Call { call: call2 });
            }
            ret_tok(m, call, p);
        }
        BuiltinNs::Error => {
            implicit_args(m, call, "Error message");
            let e = error_tok(m, call);
            if let Some(o) = arg(m, call, 1) {
                let ca = m.atom("cause");
                let f = m.field(e, ca);
                m.cons(
                    o,
                    Cons::Load {
                        key: Key::Named(ca),
                        dst: f,
                    },
                );
            }
            ret_tok(m, call, e);
        }
        BuiltinNs::Function => {
            set_dyn_eval(m, "E14 new Function(...)");
            unknown_method(m, call, "E14 dynamic code");
        }
        BuiltinNs::Array => call_builtin_ns(m, ns, call),
        BuiltinNs::Object => {
            let o = site_tmpl(m, call, "Object()");
            ret_tok(m, call, o);
        }
        _ => {
            implicit_args(m, call, "builtin constructor argument");
            let o = site_tmpl(m, call, "builtin");
            ret_tok(m, call, o);
        }
    }
}

/// `new Name(...)` where `Name` is not a program class: a builtin by name.
pub fn construct_named(m: &mut Model, name: &str, call: CallId) {
    if let Some(ns) = ns_by_name(name) {
        return new_builtin_ns(m, ns, call);
    }
    match name {
        "Date" | "RegExp" | "URL" | "URLSearchParams" | "TextEncoder" | "TextDecoder"
        | "AbortController" | "Headers" => {
            implicit_args(m, call, "builtin constructor argument");
            let o = site_tmpl(m, call, "builtin");
            ret_tok(m, call, o);
        }
        "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
        | "Int32Array" | "Float32Array" | "Float64Array" | "BigInt64Array" | "BigUint64Array"
        | "Uint8ClampedArray" | "ArrayBuffer" | "DataView" => {
            implicit_args(m, call, "typed array source");
            let o = site_tmpl(m, call, "TypedArray");
            let fa = m.field_any(o);
            m.add_prim(fa, prim::NUM | prim::BIGINT);
            let c = m.calls[call as usize].clone();
            if let Some(&src) = c.args.first() {
                m.cons(src, Cons::ShareBuffer { ta: o });
            }
            if name == "ArrayBuffer" && c.args.len() > 1 {
                // B4: `new ArrayBuffer(n, { maxByteLength })` is resizable.
                let r = m.reason("B4 resizable ArrayBuffer");
                m.mark_hard(o, Hard::B4Resizable, r);
            }
            ret_tok(m, call, o);
        }
        "Proxy" => {
            let c = m.calls[call as usize].clone();
            let r = m.reason("E9 new Proxy (by name)");
            if let Some(&t) = c.args.first() {
                m.cons(
                    t,
                    Cons::Hard {
                        kind: Hard::E9Proxy,
                        reason: r,
                    },
                );
            }
            unknown_method(m, call, "E9 Proxy");
        }
        _ => unknown_method(m, call, &format!("argument of unknown constructor {name}")),
    }
}

// ------------------------------------------------- function protocol

fn function_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
    let c = m.calls[call as usize].clone();
    match name {
        Some("call") => {
            let call2 = m.calls.len() as CallId;
            let this = match c.args.first() {
                Some(&n) => ThisArg::Node(n),
                None => ThisArg::Undef,
            };
            m.calls.push(CallSite {
                this,
                args: c.args.iter().skip(1).copied().collect(),
                spread: c.spread.clone(),
                ret: c.ret,
                site: c.site,
            });
            m.call_tok(call2, t, None);
        }
        Some("apply") => {
            let call2 = m.calls.len() as CallId;
            let this = match c.args.first() {
                Some(&n) => ThisArg::Node(n),
                None => ThisArg::Undef,
            };
            let mut spread = c.spread.clone();
            if let Some(&a) = c.args.get(1) {
                spread.push(a);
            }
            m.calls.push(CallSite {
                this,
                args: vec![],
                spread,
                ret: c.ret,
                site: c.site,
            });
            m.call_tok(call2, t, None);
        }
        Some("bind") => {
            // One bound value per (site, target); every bound target of a
            // site shares one (`f = f.bind(x)` in a loop rebinds its own
            // result: per target that would mint a new value each round).
            let key = (
                call,
                if matches!(m.toks[t as usize], TokKind::Bound(_)) {
                    u32::MAX
                } else {
                    t
                },
            );
            let b = match m.bound_index.get(&key) {
                Some(&b) => b,
                None => {
                    let target = m.node_l("bind-target");
                    let bthis = match c.args.first() {
                        Some(&n) => n,
                        None => {
                            let u = m.node_l("undef");
                            m.add_prim(u, prim::UNDEF);
                            u
                        }
                    };
                    let bargs: Vec<NodeId> = c.args.iter().skip(1).copied().collect();
                    let b = m.bound.len() as u32;
                    m.bound.push((target, bthis, bargs));
                    m.bound_index.insert(key, b);
                    b
                }
            };
            let target = m.bound[b as usize].0;
            m.add_tok(target, t);
            let bt = m.tok(TokKind::Bound(b));
            m.add_tok(c.ret, bt);
            for &s in &c.spread {
                // bind(...spread): extra bound args land anywhere.
                let (tg, _, _) = m.bound[b as usize].clone();
                let _ = tg;
                let r = m.reason("bind with spread arguments");
                m.sink_node(s, r);
            }
        }
        // Generator protocol (a generator object is its step closure) and
        // iteration: the function runs with this = itself.
        Some("next" | "return" | "throw") | None => {
            m.call_tok(call, t, Some(t));
        }
        Some("toString" | "toLocaleString") => ret_prim(m, call, prim::STR),
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        _ => {}
    }
}

// ------------------------------------------------------------- arrays

fn cb_call(m: &mut Model, cb: NodeId, this: ThisArg, args: Vec<NodeId>) -> NodeId {
    let ret = m.node_l("cb-ret");
    let call2 = m.calls.len() as CallId;
    m.calls.push(CallSite {
        this,
        args,
        spread: vec![],
        ret,
        site: call2,
    });
    m.cons(cb, Cons::Call { call: call2 });
    ret
}

/// The values iterating `v` yields, as `Gen::iterate` models them for
/// compiled code: array-like elements, then the protocol
/// `v[Symbol.iterator]().next().value` (and `v.next().value`).
fn iterate(m: &mut Model, call: CallId, v: NodeId) -> NodeId {
    let site = m.calls[call as usize].site;
    let out = m.node_l("b-iterated");
    let ik = m.iter_key();
    m.cons(v, Cons::Load { key: ik, dst: out });
    let sub = |m: &mut Model, this: NodeId, key: Key| -> NodeId {
        let ret = m.node_l("b-iter-ret");
        let c2 = m.calls.len() as CallId;
        m.calls.push(CallSite {
            this: ThisArg::Node(this),
            args: vec![],
            spread: vec![],
            ret,
            site,
        });
        m.protocol_calls.insert(c2);
        m.cons(this, Cons::MCall { key, call: c2 });
        ret
    };
    let wk = Key::Named(m.atom(WK_ITERATOR));
    let it = sub(m, v, wk);
    let na = Key::Named(m.atom("next"));
    let r1 = sub(m, it, na);
    let r2 = sub(m, v, na);
    let va = Key::Named(m.atom("value"));
    m.cons(r1, Cons::Load { key: va, dst: out });
    m.cons(r2, Cons::Load { key: va, dst: out });
    out
}

/// HIR's spelling of a well-known symbol key (`Symbol.iterator` is
/// `@@__perry_wk_iterator`), see `Gen::key_of`.
pub(crate) const WK_PREFIX: &str = "@@__perry_wk_";
pub(crate) const WK_ITERATOR: &str = "@@__perry_wk_iterator";

/// The values a builtin iterator yields, held in its `#elems` field: an
/// all-keys read (`iterate`) sees them, a named read (`it.next`) does not.
fn iter_elems(m: &mut Model, it: Tok) -> NodeId {
    let a = m.atom("#elems");
    m.field(it, a)
}

/// A fresh builtin iterator (array/Map/Set/string iterator, one template
/// per source call site) yielding the values in `values`.
fn iter_over(m: &mut Model, call: CallId, values: NodeId) -> Tok {
    let it = site_tmpl(m, call, "Iterator");
    let e = iter_elems(m, it);
    m.edge(values, e);
    it
}

/// The `[key, value]` pairs of an `entries()` iterator: one array template
/// per call site holding `values` (plus the index for arrays).
fn entry_pairs(m: &mut Model, call: CallId, values: NodeId, index: bool) -> NodeId {
    let pair = site_tmpl(m, call, "Array");
    let fa = m.field_any(pair);
    m.edge(values, fa);
    if index {
        m.add_prim(fa, prim::NUM);
    }
    let n = m.node_l("entry-pairs");
    m.add_tok(n, pair);
    n
}

/// Methods of a builtin iterator: `next()` gives `{ value, done }` over its
/// values; `[Symbol.iterator]()` is itself; the iterator helpers run their
/// callback on the values. Builtin iterators have no `return`/`throw`.
fn iterator_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
    let c = m.calls[call as usize].clone();
    let elems = iter_elems(m, t);
    let num = m.node_l("num");
    m.add_prim(num, prim::NUM);
    match name {
        None | Some(WK_ITERATOR) => ret_tok(m, call, t),
        Some("next") => {
            let r = site_tmpl(m, call, "IterResult");
            let va = m.atom("value");
            let v = m.field(r, va);
            m.edge(elems, v);
            m.add_prim(v, prim::UNDEF);
            let da = m.atom("done");
            let d = m.field(r, da);
            m.add_prim(d, prim::BOOL);
            ret_tok(m, call, r);
        }
        Some("return" | "throw") => {}
        Some("toArray") => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            m.edge(elems, fa);
            ret_tok(m, call, arr);
        }
        Some("forEach" | "some" | "every" | "find" | "map" | "filter" | "flatMap") => {
            let Some(&cb) = c.args.first() else {
                return;
            };
            let r = cb_call(m, cb, ThisArg::Undef, vec![elems, num]);
            match name {
                Some("forEach") => ret_prim(m, call, prim::UNDEF),
                Some("some" | "every") => ret_prim(m, call, prim::BOOL),
                Some("find") => {
                    m.edge(elems, c.ret);
                    ret_prim(m, call, prim::UNDEF);
                }
                Some("map") => {
                    let it = iter_over(m, call, r);
                    ret_tok(m, call, it);
                }
                Some("flatMap") => {
                    // The callback's results are iterated in turn.
                    let inner = m.node_l("flatmap-inner");
                    m.cons(
                        r,
                        Cons::Load {
                            key: Key::Any,
                            dst: inner,
                        },
                    );
                    let it = iter_over(m, call, inner);
                    ret_tok(m, call, it);
                }
                _ => {
                    let it = iter_over(m, call, elems);
                    ret_tok(m, call, it);
                }
            }
        }
        Some("take" | "drop") => {
            let it = iter_over(m, call, elems);
            ret_tok(m, call, it);
        }
        Some("reduce") => {
            let Some(&cb) = c.args.first() else {
                return;
            };
            let acc = m.node_l("acc");
            match c.args.get(1) {
                Some(&i) => m.edge(i, acc),
                None => m.edge(elems, acc),
            }
            let r = cb_call(m, cb, ThisArg::Undef, vec![acc, elems, num]);
            m.edge(r, acc);
            m.edge(acc, c.ret);
        }
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        Some(n) => unknown_method(
            m,
            call,
            &format!("argument of unmodeled iterator method {n}"),
        ),
    }
}

fn array_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
    if !m.calls[call as usize].spread.is_empty()
        && !matches!(name, Some("push" | "unshift" | "concat"))
    {
        return unknown_method(m, call, "spread arguments of array method");
    }
    let c = m.calls[call as usize].clone();
    let elems = m.field_any(t);
    let num = m.node_l("num");
    m.add_prim(num, prim::NUM);
    let recv = m.node_l("arr-recv");
    m.add_tok(recv, t);
    let this_arg = |i: usize| match c.args.get(i) {
        Some(&n) => ThisArg::Node(n),
        None => ThisArg::Undef,
    };
    match name {
        // `[Symbol.iterator]()`: a fresh iterator over the elements.
        Some(WK_ITERATOR) => {
            let it = iter_over(m, call, elems);
            ret_tok(m, call, it);
        }
        // Any other computed-key method.
        None => ret_tok(m, call, t),
        // Arrays and strings are iterables, not iterators: no `next`,
        // `return` or `throw` (a call throws; `Gen::iterate` tries `next`
        // on every iterable).
        Some("next" | "return" | "throw" | "toJSON") => {}
        Some("push" | "unshift") => {
            for &a in &c.args {
                m.edge(a, elems);
            }
            for &s in &c.spread {
                m.cons(
                    s,
                    Cons::Load {
                        key: Key::Any,
                        dst: elems,
                    },
                );
            }
            ret_prim(m, call, prim::NUM);
        }
        Some("pop" | "shift" | "at") => {
            if name == Some("at") {
                coerce_args(m, call, &[0]);
            }
            m.edge(elems, c.ret);
            ret_prim(m, call, prim::UNDEF);
        }
        Some("splice" | "toSpliced") => {
            coerce_args(m, call, &[0, 1]);
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            let target = if name == Some("splice") { elems } else { ofa };
            for &a in c.args.iter().skip(2) {
                m.edge(a, target);
            }
            // Spread may contribute both index arguments and inserted values.
            for &sp in &c.spread {
                let key = m.elem_key();
                m.cons(sp, Cons::Load { key, dst: target });
            }
            ret_tok(m, call, out);
        }
        Some("fill") => {
            coerce_args(m, call, &[1, 2]);
            if let Some(&a) = c.args.first() {
                m.edge(a, elems);
            }
            ret_tok(m, call, t);
        }
        Some("reverse" | "copyWithin") => {
            if name == Some("copyWithin") {
                coerce_args(m, call, &[0, 1, 2]);
            }
            ret_tok(m, call, t);
        }
        Some("slice" | "concat" | "toReversed" | "flat" | "with") => {
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            match name {
                Some("slice") => coerce_args(m, call, &[0, 1]),
                Some("flat") => {
                    coerce_args(m, call, &[0]);
                    let key = m.elem_key();
                    // A dynamic depth can flatten arbitrarily many levels.
                    m.cons(ofa, Cons::Load { key, dst: ofa });
                }
                Some("with") => {
                    coerce_args(m, call, &[0]);
                    if let Some(&a) = c.args.get(1) {
                        m.edge(a, ofa);
                    }
                }
                Some("concat") => {
                    let vals = all_arg_values(m, call);
                    m.edge(vals, ofa);
                    let key = m.elem_key();
                    m.cons(vals, Cons::Load { key, dst: ofa });
                }
                _ => {}
            }
            ret_tok(m, call, out);
        }
        Some("sort" | "toSorted") => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, cb, ThisArg::Undef, vec![elems, elems]);
                let why = m.reason("sort comparator result");
                m.cons(r, Cons::Coerce { reason: why });
            }
            if name == Some("toSorted") {
                let out = site_tmpl(m, call, "Array");
                let ofa = m.field_any(out);
                m.edge(elems, ofa);
                ret_tok(m, call, out);
            } else {
                ret_tok(m, call, t);
            }
        }
        Some(
            "forEach" | "map" | "filter" | "some" | "every" | "find" | "findIndex" | "findLast"
            | "findLastIndex" | "flatMap",
        ) => {
            let Some(&cb) = c.args.first() else {
                return;
            };
            let r = cb_call(m, cb, this_arg(1), vec![elems, num, recv]);
            match name {
                Some("map" | "flatMap") => {
                    let out = site_tmpl(m, call, "Array");
                    let ofa = m.field_any(out);
                    m.edge(r, ofa);
                    if name == Some("flatMap") {
                        m.cons(
                            r,
                            Cons::Load {
                                key: Key::Any,
                                dst: ofa,
                            },
                        );
                    }
                    ret_tok(m, call, out);
                }
                Some("filter") => ret_tok(m, call, t),
                Some("find" | "findLast") => {
                    m.edge(elems, c.ret);
                    ret_prim(m, call, prim::UNDEF);
                }
                Some("forEach") => ret_prim(m, call, prim::UNDEF),
                Some("some" | "every") => ret_prim(m, call, prim::BOOL),
                _ => ret_prim(m, call, prim::NUM),
            }
        }
        Some("reduce" | "reduceRight") => {
            let Some(&cb) = c.args.first() else {
                return;
            };
            let acc = m.node_l("acc");
            match c.args.get(1) {
                Some(&i) => m.edge(i, acc),
                None => m.edge(elems, acc),
            }
            let r = cb_call(m, cb, ThisArg::Undef, vec![acc, elems, num, recv]);
            m.edge(r, acc);
            m.edge(acc, c.ret);
        }
        Some("indexOf" | "lastIndexOf" | "includes" | "findIndexOf") => {
            coerce_args(m, call, &[1]);
            ret_prim(
                m,
                call,
                if name == Some("includes") {
                    prim::BOOL
                } else {
                    prim::NUM
                },
            );
        }
        Some("join" | "toString" | "toLocaleString") => {
            let reason = m.reason("array join element conversion");
            m.cons(elems, Cons::Coerce { reason });
            if name == Some("join") {
                coerce_args(m, call, &[0]);
            }
            ret_prim(m, call, prim::STR);
        }
        Some("keys") => {
            let it = iter_over(m, call, num);
            ret_tok(m, call, it);
        }
        Some("values") => {
            let it = iter_over(m, call, elems);
            ret_tok(m, call, it);
        }
        Some("entries") => {
            let pairs = entry_pairs(m, call, elems, true);
            let it = iter_over(m, call, pairs);
            ret_tok(m, call, it);
        }
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        Some(n) => {
            // A missing intrinsic on an actual array throws. Source-defined
            // own/prototype members and opaque writes are followed separately.
            let actual_array = matches!(m.toks[t as usize], TokKind::Obj(tm)
                if matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { .. }));
            let inherited = matches!(
                n,
                "constructor"
                    | "propertyIsEnumerable"
                    | "isPrototypeOf"
                    | "__defineGetter__"
                    | "__defineSetter__"
                    | "__lookupGetter__"
                    | "__lookupSetter__"
                    | "__proto__"
            );
            if !actual_array || inherited {
                unknown_method(m, call, &format!("argument of unmodeled array method {n}"));
            }
        }
    }
}

fn bobj_method(m: &mut Model, t: Tok, what: &str, name: Option<&str>, call: CallId) {
    let c = m.calls[call as usize].clone();
    let elems = m.field_any(t);
    match (what, name) {
        ("Iterator", _) => iterator_method(m, t, name, call),
        // Iterables or plain results, not iterators: no `next`/`return`/
        // `throw` (a call throws).
        (
            "Collection" | "TypedArray" | "Promise" | "Error" | "IterResult",
            Some("next" | "return" | "throw"),
        ) => {}
        ("Collection", Some(WK_ITERATOR)) => {
            // `[Symbol.iterator]()`: Map entries or Set values.
            let pairs = entry_pairs(m, call, elems, false);
            let vals = m.node_l("coll-iter");
            m.edge(elems, vals);
            m.edge(pairs, vals);
            let it = iter_over(m, call, vals);
            ret_tok(m, call, it);
        }
        (_, None) => ret_tok(m, call, t),
        ("Collection", Some("set" | "add")) => {
            for &a in &c.args {
                m.edge(a, elems);
            }
            ret_tok(m, call, t);
        }
        ("Collection", Some("get")) => {
            m.edge(elems, c.ret);
            ret_prim(m, call, prim::UNDEF);
        }
        ("Collection", Some("has" | "delete")) => ret_prim(m, call, prim::BOOL),
        ("Collection", Some("clear")) => ret_prim(m, call, prim::UNDEF),
        ("Collection", Some("keys" | "values")) => {
            let it = iter_over(m, call, elems);
            ret_tok(m, call, it);
        }
        ("Collection", Some("entries")) => {
            let pairs = entry_pairs(m, call, elems, false);
            let it = iter_over(m, call, pairs);
            ret_tok(m, call, it);
        }
        ("Collection", Some("forEach")) => {
            if let Some(&cb) = c.args.first() {
                let recv = m.node_l("coll");
                m.add_tok(recv, t);
                let this = match c.args.get(1) {
                    Some(&n) => ThisArg::Node(n),
                    None => ThisArg::Undef,
                };
                let _ = cb_call(m, cb, this, vec![elems, elems, recv]);
            }
            ret_prim(m, call, prim::UNDEF);
        }
        ("Promise", Some("then" | "catch" | "finally")) => {
            let v = promise_value(m, t);
            let out = site_tmpl(m, call, "Promise");
            let ov = promise_value(m, out);
            let thrown = m.thrown;
            let (on_ok, on_err) = match name {
                Some("then") => (c.args.first().copied(), c.args.get(1).copied()),
                Some("catch") => (None, c.args.first().copied()),
                _ => (c.args.first().copied(), c.args.first().copied()),
            };
            if let Some(f) = on_ok {
                let arg = if name == Some("finally") {
                    let u = m.node_l("undef");
                    m.add_prim(u, prim::UNDEF);
                    u
                } else {
                    v
                };
                let r = cb_call(m, f, ThisArg::Undef, vec![arg]);
                if name != Some("finally") {
                    adopt(m, r, ov);
                }
            }
            if let Some(f) = on_err {
                let r = cb_call(m, f, ThisArg::Undef, vec![thrown]);
                if name != Some("finally") {
                    adopt(m, r, ov);
                }
            }
            if name == Some("finally") || on_ok.is_none() {
                m.edge(v, ov);
            }
            ret_tok(m, call, out);
        }
        ("Error", Some("toString")) => ret_prim(m, call, prim::STR),
        ("builtin" | "RegExp", Some("@@__perry_wk_replace")) => {
            primitives::string_method(m, "replace", call);
        }

        (
            "Date" | "RegExp" | "URL" | "TextCodec" | "builtin" | "BoxedPrimitive" | "Timer",
            Some(n),
        ) if !n.starts_with("on") && n != "addEventListener" => {
            implicit_args(m, call, "builtin method argument");
            ret_prim(m, call, prim::ALL);
            let pb = m.tok(TokKind::PrimBox);
            ret_tok(m, call, pb);
            if matches!(n, "unref" | "ref" | "refresh") {
                ret_tok(m, call, t);
            }
        }
        (
            "TypedArray",
            Some("subarray" | "slice" | "fill" | "reverse" | "sort" | "map" | "filter"),
        ) => {
            if let Some(&cb) = c.args.first() {
                let num = m.node_l("num");
                m.add_prim(num, prim::NUM);
                let _ = cb_call(m, cb, ThisArg::Undef, vec![num, num]);
            }
            ret_tok(m, call, t);
        }
        ("TypedArray", Some("transfer" | "transferToFixedLength" | "resize" | "grow")) => {
            let r = m.reason("B2 buffer transfer/resize");
            m.mark_hard(t, Hard::B2BufferResize, r);
            ret_tok(m, call, t);
            implicit_args(m, call, "buffer resize argument");
        }
        ("TypedArray", Some(_)) => {
            implicit_args(m, call, "typed array method argument");
            ret_prim(m, call, prim::ALL);
            let pb = m.tok(TokKind::PrimBox);
            ret_tok(m, call, pb);
        }
        ("primbox" | "keys" | "json" | "descriptor" | "groups" | "fromEntries", Some(n)) => {
            array_method(m, t, Some(n), call)
        }
        (_, Some(n)) => unknown_method(m, call, &format!("argument of {what}.{n}")),
    }
}

// ------------------------------------------------------ native parents

/// Prototype token of a native parent class (`extends EventEmitter`).
pub fn native_proto(m: &mut Model, name: &str) -> Tok {
    m.tok(TokKind::NativeProto(name.to_string()))
}

/// Native parents with a summary (owner decision 135.5).
pub fn native_parent_summarized(name: &str) -> bool {
    matches!(
        name,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EventEmitter"
    )
}

/// Keys the native parent constructor defines on the instance.
pub fn native_parent_keys(name: &str) -> &'static [&'static str] {
    match name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => {
            &["stack", "message"]
        }
        "EventEmitter" => &["_events", "_eventsCount", "_maxListeners"],
        _ => &[],
    }
}

/// `super(...)` into a native parent.
pub fn native_parent_ctor(m: &mut Model, tm: TmplId, call: CallId, inst: Tok) {
    let name = m.tmpls[tm as usize]
        .native_parent
        .as_ref()
        .map(|(_, c)| c.clone())
        .unwrap_or_default();
    if !native_parent_summarized(&name) {
        let r = m.reason(&format!("E21 native parent {name} without a summary"));
        m.mark_hard(inst, Hard::E21NativeParent, r);
        m.escape(inst, r);
        unknown_method(m, call, "E21 native parent constructor argument");
        return;
    }
    implicit_args(m, call, "native parent constructor argument");
    let site = m.anon_birth_site(&format!("{name} constructor"));
    for k in native_parent_keys(&name) {
        let a = m.atom(k);
        let v = m.node_l("native-key");
        if k.starts_with('_') {
            // EventEmitter bookkeeping: runtime objects/numbers.
            m.add_prim(v, prim::NUM | prim::UNDEF);
            m.add_top(v, |_| "EventEmitter bookkeeping field".into());
        } else {
            m.add_prim(v, prim::STR | prim::UNDEF);
        }
        let n = m.node_l("inst");
        m.add_tok(n, inst);
        m.cons(
            n,
            Cons::Store {
                key: Key::Named(a),
                src: v,
                site,
            },
        );
    }
}

fn native_method(m: &mut Model, native: &str, origin: Tok, name: &str, call: CallId) {
    let c = m.calls[call as usize].clone();
    match (native, name) {
        (
            "EventEmitter",
            "on" | "addListener" | "once" | "prependListener" | "prependOnceListener",
        ) => {
            if let Some(&f) = c.args.get(1) {
                let a = m.atom("#listeners");
                let fl = m.field(origin, a);
                m.edge(f, fl);
            }
            ret_tok(m, call, origin);
        }
        ("EventEmitter", "emit") => {
            let a = m.atom("#listeners");
            let fl = m.field(origin, a);
            let ret = m.node_l("emit-ret");
            let call2 = m.calls.len() as CallId;
            m.calls.push(CallSite {
                this: ThisArg::Tok(origin),
                args: c.args.iter().skip(1).copied().collect(),
                spread: c.spread.clone(),
                ret,
                site: call2,
            });
            m.cons(fl, Cons::Call { call: call2 });
            ret_prim(m, call, prim::BOOL);
        }
        ("EventEmitter", "off" | "removeListener" | "removeAllListeners" | "setMaxListeners") => {
            ret_tok(m, call, origin)
        }
        ("EventEmitter", "listenerCount" | "getMaxListeners") => ret_prim(m, call, prim::NUM),
        ("EventEmitter", "listeners" | "rawListeners" | "eventNames") => {
            let a = m.atom("#listeners");
            let fl = m.field(origin, a);
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            m.edge(fl, fa);
            m.add_prim(fa, prim::STR);
            ret_tok(m, call, arr);
        }
        (_, "toString") => ret_prim(m, call, prim::STR),
        _ => unknown_method(m, call, &format!("argument of native {native}.{name}")),
    }
}
