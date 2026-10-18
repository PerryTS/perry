//! Builtin summaries: what runtime-provided objects and functions do with
//! the values they receive (design 3.2, "runtime builtin call").
//!
//! Every summary over-approximates the runtime entry it stands for. Anything
//! without a summary falls back to `unknown_method`: arguments escape to ⊤
//! and the result is ⊤. A missing summary costs precision, never soundness.

use crate::model::*;

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
        TokKind::Builtin(_) => match name.as_deref() {
            Some("prototype") => {
                let op = m.tok(TokKind::ObjectProto);
                m.add_tok(dst, op);
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
        TokKind::NativeProto(_) | TokKind::BuiltinFn | TokKind::Bound(_) => {
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

pub fn prim_method(m: &mut Model, prim: u16, _key: Key, call: CallId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    // String/number/boolean methods: callbacks are called by the runtime
    // with primitives (replace), object arguments are coerced.
    implicit_args(m, call, "argument of a primitive method");
    ret_prim(m, call, prim::ALL);
    let pb = m.tok(TokKind::PrimBox);
    ret_tok(m, call, pb);
}

/// Methods on builtin tokens (the receiver's own builtin behaviour).
pub fn own_method(m: &mut Model, t: Tok, key: Key, call: CallId) {
    let prev = m.cur_this.replace(t);
    own_method_inner(m, t, key, call);
    m.cur_this = prev;
}

fn own_method_inner(m: &mut Model, t: Tok, key: Key, call: CallId) {
    let name = match key {
        Key::Named(a) => Some(m.atoms[a as usize].clone()),
        Key::Any => None,
    };
    match m.toks[t as usize].clone() {
        TokKind::Builtin(ns) => ns_method(m, ns, name.as_deref(), call),
        TokKind::Func(_) | TokKind::Bound(_) => function_method(m, t, name.as_deref(), call),
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

fn ns_method(m: &mut Model, ns: BuiltinNs, name: Option<&str>, call: CallId) {
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
                    Cons::Load {
                        key: Key::Any,
                        dst: fa,
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
            let vals = all_arg_values(m, call);
            m.cons(
                vals,
                Cons::Load {
                    key: Key::Any,
                    dst: fa,
                },
            );
            if name == "of" {
                m.edge(vals, fa);
            }
            // A map function is called with each element.
            if let Some(f) = arg(m, call, 1) {
                let ret = m.node_l("from-map");
                let call2 = m.calls.len() as CallId;
                let num = m.node_l("num");
                m.add_prim(num, prim::NUM);
                m.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![fa, num],
                    spread: vec![],
                    ret,
                    site: call2,
                });
                m.cons(f, Cons::Call { call: call2 });
                m.edge(ret, fa);
            }
            ret_tok(m, call, arr);
        }
        (BuiltinNs::Json, "stringify") => {
            implicit_args(m, call, "JSON.stringify value");
            ret_prim(m, call, prim::STR | prim::UNDEF);
        }
        (BuiltinNs::Json, "parse") => {
            implicit_args(m, call, "JSON.parse text");
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
                m.calls.push(CallSite {
                    this: ThisArg::Tok(j),
                    args: vec![s, fa],
                    spread: vec![],
                    ret,
                    site: call2,
                });
                m.cons(rv, Cons::Call { call: call2 });
                m.edge(ret, fa);
            }
        }
        (BuiltinNs::Math, _) | (BuiltinNs::Number, _) | (BuiltinNs::String, _) => {
            implicit_args(m, call, "ToPrimitive");
            ret_prim(m, call, prim::ALL);
            if name == "raw" || name == "split" {
                let pb = m.tok(TokKind::PrimBox);
                ret_tok(m, call, pb);
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
        BuiltinNs::String | BuiltinNs::Number | BuiltinNs::Boolean | BuiltinNs::Symbol => {
            implicit_args(m, call, "ToPrimitive");
            ret_prim(m, call, prim::ALL);
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
        "Map" | "Set" | "WeakMap" | "WeakSet" => {
            let c = site_tmpl(m, call, "Collection");
            let fa = m.field_any(c);
            if let Some(it) = arg(m, call, 0) {
                let elems = m.node_l("coll-init");
                m.cons(
                    it,
                    Cons::Load {
                        key: Key::Any,
                        dst: elems,
                    },
                );
                m.edge(elems, fa);
                m.cons(
                    elems,
                    Cons::Load {
                        key: Key::Any,
                        dst: fa,
                    },
                );
            }
            ret_tok(m, call, c);
        }
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

fn array_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
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
        // Iteration protocol: an array is its own iterator source.
        None => ret_tok(m, call, t),
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
            m.edge(elems, c.ret);
            ret_prim(m, call, prim::UNDEF);
        }
        Some("splice") => {
            for &a in c.args.iter().skip(2) {
                m.edge(a, elems);
            }
            ret_tok(m, call, t);
        }
        Some("fill") => {
            if let Some(&a) = c.args.first() {
                m.edge(a, elems);
            }
            ret_tok(m, call, t);
        }
        Some(
            "slice" | "concat" | "reverse" | "toReversed" | "flat" | "copyWithin" | "with"
            | "toSpliced",
        ) => {
            for &a in &c.args {
                // concat: array arguments contribute their elements.
                m.edge(a, elems);
                m.cons(
                    a,
                    Cons::Load {
                        key: Key::Any,
                        dst: elems,
                    },
                );
            }
            if name == Some("flat") {
                m.cons(
                    elems,
                    Cons::Load {
                        key: Key::Any,
                        dst: elems,
                    },
                );
            }
            ret_tok(m, call, t);
        }
        Some("sort" | "toSorted") => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, cb, ThisArg::Undef, vec![elems, elems]);
                let why = m.reason("sort comparator result");
                m.cons(r, Cons::Implicit { reason: why });
            }
            ret_tok(m, call, t);
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
            ret_prim(m, call, prim::NUM | prim::BOOL);
        }
        Some("join" | "toString" | "toLocaleString") => {
            let why = m.reason("array join (element ToString)");
            m.cons(elems, Cons::Implicit { reason: why });
            implicit_args(m, call, "join separator");
            ret_prim(m, call, prim::STR);
        }
        Some("keys") => {
            let pb = m.tok(TokKind::PrimBox);
            ret_tok(m, call, pb);
        }
        Some("values" | "entries") => {
            m.add_prim(elems, prim::NUM);
            if name == Some("entries") {
                m.add_tok(elems, t);
            }
            ret_tok(m, call, t);
        }
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        Some(n) => unknown_method(m, call, &format!("argument of unmodeled array method {n}")),
    }
}

fn bobj_method(m: &mut Model, t: Tok, what: &str, name: Option<&str>, call: CallId) {
    let c = m.calls[call as usize].clone();
    let elems = m.field_any(t);
    match (what, name) {
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
        ("Collection", Some("keys" | "values" | "entries")) => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            m.edge(elems, fa);
            if name == Some("entries") {
                m.add_tok(fa, arr);
            }
            ret_tok(m, call, arr);
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
