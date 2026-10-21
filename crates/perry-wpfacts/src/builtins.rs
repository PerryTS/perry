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
        if m.explain {
            eprintln!("perry: wpfacts explain: dynamic code reachable: {why}");
        }
        // Not the global object's concern: code runs only with a linked
        // interpreter, whose global reads and writes `compute` records up
        // front; without one the Function constructor throws.
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
        let tm = m.tmpl_of(t).expect("site template");
        let ctx = m.birth_ctx(call);
        return m.site_tok(tm, ctx);
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
    let t = m.tok(TokKind::Obj(tm, NO_CTX));
    m.site_tmpls.insert(key, t);
    let ctx = m.birth_ctx(call);
    let t = m.site_tok(tm, ctx);
    t
}

/// A synthetic resolver function: its first parameter flows into `dst`.
pub fn resolver(m: &mut Model, dst: NodeId) -> Tok {
    if let Some(&t) = m.resolvers.get(&dst) {
        return t;
    }
    let fx = m.funcs.len() as FnIdx;
    let tok = m.tok(TokKind::Func(fx, NO_CTX));
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
        no_summary: false,
        gen_step: false,
        parent: NO_FN,
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
pub fn adopt(m: &mut Model, by: CallId, v: NodeId, dst: NodeId) {
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
    m.solver_call(
        by,
        CallSite {
            this: ThisArg::Node(v),
            args: vec![rn, jn],
            spread: vec![],
            ret,
            site: call,
        },
    );
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
            Some(n) if global_user_key(n) => {
                let Key::Named(a) = key else { unreachable!() };
                let f = global_field(m, t, a);
                if native_written(m, t, a) {
                    if m.explain {
                        let op = m.tok_index.get(&TokKind::ObjectProto).copied();
                        let hits: Vec<String> = m
                            .native_writes
                            .iter()
                            .filter(|((w, k), _)| {
                                (*w == TOP || *w == t || Some(*w) == op) && (k.is_none() || *k == Some(a))
                            })
                            .map(|((w, k), r)| format!("{}/{:?}: {}", m.tok_name(*w), k.map(|x| m.atoms[x as usize].clone()), m.reasons[*r as usize]))
                            .collect();
                        eprintln!("perry: wpfacts explain: globalThis.{n} written (in_top {}): {hits:?}", m.global_in_top);
                    }
                    m.add_top(dst, |_| format!("globalThis.{n} written by unmodeled code"));
                    m.add_prim(dst, prim::ALL);
                } else {
                    m.native_missing.push((t, a, NativeMiss::Global(dst, f)));
                }
                m.edge(f, dst);
                m.add_prim(dst, prim::UNDEF);
            }
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
        TokKind::Builtin(BuiltinNs::Require) => match name.as_deref() {
            Some("cache") | Some("extensions") => {
                let (cache, ext) = require_parts(m);
                let p = if name.as_deref() == Some("cache") { cache } else { ext };
                m.add_tok(dst, p);
            }
            Some("main") => {
                let u = m.cjs_union;
                m.edge(u, dst);
                m.add_prim(dst, prim::UNDEF | prim::NULL);
            }
            _ => {
                let bf = m.tok(TokKind::BuiltinFn);
                m.add_tok(dst, bf);
                m.add_top(dst, |_| {
                    format!("require.{}", name.as_deref().unwrap_or("[computed]"))
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
        TokKind::Func(f, _) => match name.as_deref() {
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
        TokKind::Obj(tm, _) => match m.tmpls[tm as usize].kind.clone() {
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
                // The CJS bootstrap main record starts empty. Its actual
                // fields are supplied by generated wrapper stores, not by
                // opaque builtin-method summaries.
                _ if matches!(what, "CjsMain" | "CjsRecord") => m.add_prim(dst, prim::UNDEF),
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
                // A container of primitives (split/match results, keys ...).
                _ if what == "primbox" => m.add_prim(dst, prim::ALL),
                // `require.cache` / `require.extensions` are plain objects:
                // their fields hold what was stored (by compiled code, or ⊤
                // from the loader, `call_builtin_ns`); an absent key reads
                // `undefined`.
                _ if what == "require.cache" || what == "require.extensions" => {
                    m.add_prim(dst, prim::UNDEF)
                }
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
        // A key the native prototype does not define: the chain goes on.
        TokKind::NativeProto(_) if native_skips(m, t, key, NativeMiss::Load(dst)) => {}
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
            if m.explain {
                let kn = match _key { Key::Named(a) => m.atoms[a as usize].clone(), Key::Any => "[computed]".into() };
                eprintln!("perry: wpfacts explain: E20 store on {} key {kn}", m.tok_name(t));
            }
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
    let pb = site_tmpl(m, call, "primbox");
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
    // A well-known-symbol method is a computed-key method, except the
    // iteration protocol of the builtin iterables, which is modeled.
    let iterable = match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => matches!(
            m.tmpls[*tm as usize].kind,
            TmplKind::Arr { .. }
                | TmplKind::BObj {
                    what: "Collection" | "Iterator" | "primbox",
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
        TokKind::Func(_, _) | TokKind::Bound(_) => function_method(m, t, name.as_deref(), call),
        TokKind::BuiltinFn => match name.as_deref() {
            None => {}
            Some(n) => unknown_method(
                m,
                call,
                &format!("method {n} of an opaque builtin function"),
            ),
        },
        TokKind::NativeProto(_) => {}
        TokKind::Obj(tm, _) => match m.tmpls[tm as usize].kind.clone() {
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
        TokKind::NativeProto(name_of) => native_method(m, p, &name_of, origin, &name, call),
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
                m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args: c.args.iter().skip(2).copied().collect(),
                        spread: vec![],
                        ret,
                        site: call2,
                    },
                );
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
                let site = m.anon_post_site("Object.assign (value call)");
                for &s in c.args.iter().skip(1) {
                    m.cons(s, Cons::Spread { into: t, site });
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
                    m.cons(inner, Cons::Spread { into: t, site });
                }
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
                let num = m.node_l("num");
                m.add_prim(num, prim::NUM);
                let call2 = m.calls.len() as CallId;
                m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args: vec![fa, num],
                        spread: vec![],
                        ret,
                        site: call2,
                    },
                );
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
                let s = m.node_l("str");
                m.add_prim(s, prim::STR);
                let call2 = m.calls.len() as CallId;
                m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Tok(j),
                        args: vec![s, fa],
                        spread: vec![],
                        ret,
                        site: call2,
                    },
                );
                m.cons(rv, Cons::Call { call: call2 });
                m.edge(ret, fa);
            }
        }
        (BuiltinNs::Math, _) | (BuiltinNs::Number, _) | (BuiltinNs::String, _) => {
            implicit_args(m, call, "ToPrimitive");
            ret_prim(m, call, prim::ALL);
            if name == "raw" || name == "split" {
                let pb = site_tmpl(m, call, "primbox");
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
                    adopt(m, call, a, v);
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
            adopt(m, call, items, settled);
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
            let pb = site_tmpl(m, call, "primbox");
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
        BuiltinNs::Require => {
            // A compiled module named by a constant path: its exports (the
            // registry record's `exports`, as `require.cache[path]` holds).
            let rec = m.call_lit.get(&call).and_then(|a| m.cjs_paths.get(a)).copied();
            if m.explain && rec.is_none() {
                let lit = m.call_lit.get(&call).map(|a| m.atoms[*a as usize].clone());
                let known: Vec<String> = m.cjs_paths.keys().map(|a| m.atoms[*a as usize].clone()).collect();
                eprintln!("perry: wpfacts explain: require call lit {lit:?} not in {known:?}");
            }
            match rec {
                Some(rec) => {
                    let ret = m.calls[call as usize].ret;
                    let ea = m.atom("exports");
                    // As requirePathModule: the named path, or a record the
                    // runtime published under a path it computed.
                    let unknown = m.cjs_unknown;
                    // The path node holds what `require` returns.
                    m.edge(rec, ret);
                    m.cons(unknown, Cons::Load { key: Key::Named(ea), dst: ret });
                    m.add_prim(ret, prim::UNDEF);
                }
                None => {
                    // The runtime loader never interprets code: a computed
                    // specifier resolves to a registered record (its live
                    // exports or a raw publication), a native module
                    // namespace, a manifest addon or parsed JSON. Only the
                    // last three are opaque.
                    // All of it only once a specifier reaches the call: a
                    // shared loader body no computed specifier reaches
                    // yields nothing.
                    implicit_args(m, call, "require specifier");
                    let ret = m.calls[call as usize].ret;
                    // The registry part stays inside that ⊤ as on main.
                    // It caches the registry records it loads.
                    let r = m.reason("module loaded by a computed specifier");
                    let (cache, _) = require_parts(m);
                    let fa = m.field_any(cache);
                    let reg = m.cjs_union;
                    m.edge(reg, fa);
                    if let Some(a0) = m.calls[call as usize].args.first().copied() {
                        m.cons(a0, Cons::TopIfAny { dst: ret, from: ret, reason: Some(r) });
                    }
                }
            }
        }
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
                m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args: vec![rn, jn],
                        spread: vec![],
                        ret,
                        site: call2,
                    },
                );
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
            let this = match c.args.first() {
                Some(&n) => ThisArg::Node(n),
                None => ThisArg::Undef,
            };
            let call2 = m.solver_call(
                call,
                CallSite {
                    this,
                    args: c.args.iter().skip(1).copied().collect(),
                    spread: c.spread.clone(),
                    ret: c.ret,
                    site: c.site,
                },
            );
            m.call_tok(call2, t, None);
        }
        Some("apply") => {
            let this = match c.args.first() {
                Some(&n) => ThisArg::Node(n),
                None => ThisArg::Undef,
            };
            let mut spread = c.spread.clone();
            if let Some(&a) = c.args.get(1) {
                spread.push(a);
            }
            let call2 = m.solver_call(
                call,
                CallSite {
                    this,
                    args: vec![],
                    spread,
                    ret: c.ret,
                    site: c.site,
                },
            );
            m.call_tok(call2, t, None);
        }
        Some("bind") => {
            // One bound value per (birth site, target): a bound value is a
            // birth, so every instance's copy of the bind call makes the
            // same one. Every bound target of a site shares one (`f =
            // f.bind(x)` in a loop rebinds its own result: per target that
            // would mint a new value each round).
            let key = (
                m.birth_ctx(call),
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
        // Generator protocol (a generator object is its step closure): the
        // step closure only; any other function value has no such methods
        // (a computed key on one is a runtime property: unknown).
        Some("next" | "return" | "throw") | None
            if !matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_step) =>
        {
            if name.is_none() || !matches!(m.toks[t as usize], TokKind::Func(..)) {
                unknown_method(m, call, "computed method of a function value");
            }
        }
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

fn cb_call(m: &mut Model, by: CallId, cb: NodeId, this: ThisArg, args: Vec<NodeId>) -> NodeId {
    let ret = m.node_l("cb-ret");
    let call2 = m.calls.len() as CallId;
    m.solver_call(
        by,
        CallSite {
            this,
            args,
            spread: vec![],
            ret,
            site: call2,
        },
    );
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
        let c2 = m.solver_call(
            call,
            CallSite {
                this: ThisArg::Node(this),
                args: vec![],
                spread: vec![],
                ret,
                site,
            },
        );
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
            let r = cb_call(m, call, cb, ThisArg::Undef, vec![elems, num]);
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
            let r = cb_call(m, call, cb, ThisArg::Undef, vec![acc, elems, num]);
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
        Some("next" | "return" | "throw") => {}
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
        // In place: the receiver itself.
        Some("reverse" | "copyWithin") => ret_tok(m, call, t),
        // A new array, born at this call site, holding the receiver's
        // elements and what the call adds. It is not the receiver: a write
        // into the copy never reaches the source (`args.slice()` then
        // `copy[i] = v`).
        Some("slice" | "concat" | "toReversed" | "flat" | "with" | "toSpliced") => {
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            let added: &[NodeId] = match name {
                // Array arguments contribute their elements, others themselves.
                Some("concat") => &c.args,
                Some("with") => c.args.get(1..).unwrap_or(&[]),
                Some("toSpliced") => c.args.get(2..).unwrap_or(&[]),
                _ => &[],
            };
            for &a in added {
                m.edge(a, ofa);
                if name == Some("concat") {
                    m.cons(a, Cons::Load { key: Key::Any, dst: ofa });
                }
            }
            if name == Some("flat") {
                m.cons(elems, Cons::Load { key: Key::Any, dst: ofa });
            }
            ret_tok(m, call, out);
        }
        Some("sort" | "toSorted") => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, call, cb, ThisArg::Undef, vec![elems, elems]);
                let why = m.reason("sort comparator result");
                m.cons(r, Cons::Implicit { reason: why });
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
            let r = cb_call(m, call, cb, this_arg(1), vec![elems, num, recv]);
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
                // A new array of some of the receiver's elements.
                Some("filter") => {
                    let out = site_tmpl(m, call, "Array");
                    let ofa = m.field_any(out);
                    m.edge(elems, ofa);
                    ret_tok(m, call, out);
                }
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
            let r = cb_call(m, call, cb, ThisArg::Undef, vec![acc, elems, num, recv]);
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
        Some(n) => unknown_method(m, call, &format!("argument of unmodeled array method {n}")),
    }
}

fn bobj_method(m: &mut Model, t: Tok, what: &str, name: Option<&str>, call: CallId) {
    let c = m.calls[call as usize].clone();
    let elems = m.field_any(t);
    match (what, name) {
        ("CjsMain" | "CjsRecord", _) => {}
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
                let _ = cb_call(m, call, cb, this, vec![elems, elems, recv]);
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
                let r = cb_call(m, call, f, ThisArg::Undef, vec![arg]);
                if name != Some("finally") {
                    adopt(m, call, r, ov);
                }
            }
            if let Some(f) = on_err {
                let r = cb_call(m, call, f, ThisArg::Undef, vec![thrown]);
                if name != Some("finally") {
                    adopt(m, call, r, ov);
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
            let pb = site_tmpl(m, call, "primbox");
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
                let _ = cb_call(m, call, cb, ThisArg::Undef, vec![num, num]);
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
            let pb = site_tmpl(m, call, "primbox");
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

/// The own keys of a summarized native parent's prototypes below
/// `Object.prototype` (node's `Reflect.ownKeys`; the runtime builds the same
/// sealed static layouts, e.g. `EVENT_EMITTER_PROTOTYPE`). The model's chain
/// goes from the native prototype straight to `Object.prototype`, so an
/// `Error` subclass's list holds `Error.prototype`'s keys too.
pub fn native_proto_keys(name: &str) -> &'static [&'static str] {
    match name {
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => {
            &["constructor", "name", "message", "toString"]
        }
        "EventEmitter" => &[
            "constructor",
            "_events",
            "_eventsCount",
            "_maxListeners",
            "setMaxListeners",
            "getMaxListeners",
            "emit",
            "addListener",
            "on",
            "prependListener",
            "once",
            "prependOnceListener",
            "removeListener",
            "off",
            "removeAllListeners",
            "listeners",
            "rawListeners",
            "listenerCount",
            "eventNames",
        ],
        _ => &[],
    }
}

/// May native prototype `np`'s key `a` be written ([`Model::native_writes`])?
pub fn native_written(m: &Model, np: Tok, a: Atom) -> bool {
    let hit = |t: Tok| {
        m.native_writes.contains_key(&(t, None)) || m.native_writes.contains_key(&(t, Some(a)))
    };
    let global = match &m.toks[np as usize] {
        TokKind::NativeProto(n) => ns_by_name(n).is_some(),
        TokKind::Builtin(BuiltinNs::GlobalThis) => true,
        _ => false,
    };
    let is_global = matches!(m.toks[np as usize], TokKind::Builtin(BuiltinNs::GlobalThis));
    (hit(TOP) && (!is_global || m.global_in_top))
        || hit(np)
        || (global
            && m.tok_index
                .get(&TokKind::ObjectProto)
                .is_some_and(|&op| hit(op)))
}

/// Does a lookup of `key` pass native prototype `np` (a key it does not
/// define, no write may have added it)? Then it is recorded, to run as an
/// opaque member if the key turns out written.
fn native_skips(m: &mut Model, np: Tok, key: Key, miss: NativeMiss) -> bool {
    let Key::Named(a) = key else {
        return false;
    };
    let TokKind::NativeProto(native) = &m.toks[np as usize] else {
        return false;
    };
    let name = m.atoms[a as usize].as_str();
    if !native_parent_summarized(native)
        || name.starts_with(WK_PREFIX)
        || name == crate::STAR_KEY
        || native_proto_keys(native).contains(&name)
        || native_written(m, np, a)
    {
        return false;
    }
    m.native_missing.push((np, a, miss));
    true
}

/// A write that may reach a native prototype: `t` is ⊤, `ObjectProto` or the
/// prototype itself; `key` `None` (or computed): every key. Lookups that
/// passed it under a now-written key run as opaque members.
pub fn note_native_write(m: &mut Model, t: Tok, key: Option<Key>, what: &str) {
    if t != TOP
        && !matches!(
            m.toks[t as usize],
            TokKind::ObjectProto
                | TokKind::NativeProto(_)
                | TokKind::Builtin(BuiltinNs::GlobalThis)
        )
    {
        return;
    }
    let star = m.atom(crate::STAR_KEY);
    let k = match key {
        Some(Key::Named(a)) if a != star => Some(a),
        _ => None,
    };
    if m.native_writes.contains_key(&(t, None)) || m.native_writes.contains_key(&(t, k)) {
        return;
    }
    let on = if t == TOP {
        "⊤".to_string()
    } else {
        m.tok_name(t)
    };
    if m.explain {
        eprintln!("perry: wpfacts explain: may be written: {what} on {on} key {:?}", k.map(|a| m.atoms[a as usize].clone()));
    }
    let r = m.reason(&format!("native prototype may be written: {what} on {on}"));
    m.native_writes.insert((t, k), r);
    replay_native_missing(m);
}

/// The global object may now be ⊤: stores into ⊤ may have written globals.
pub fn note_global_in_top(m: &mut Model) {
    if !m.global_in_top {
        m.global_in_top = true;
        replay_native_missing(m);
    }
}

/// Lookups that passed a native prototype (or read a program-owned global)
/// under a key that may now be written: they take the opaque member.
fn replay_native_missing(m: &mut Model) {
    if m.native_missing.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut m.native_missing);
    let mut keep = Vec::with_capacity(pending.len());
    m.with_b(|m| {
        for (np, a, miss) in pending {
            if !native_written(m, np, a) {
                keep.push((np, a, miss));
                continue;
            }
            match miss {
                NativeMiss::Load(dst) => {
                    let bf = m.tok(TokKind::BuiltinFn);
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::ALL);
                }
                NativeMiss::Global(dst, f) => {
                    let name = m.atoms[a as usize].clone();
                    if m.explain {
                        let ws: Vec<String> = m.native_writes.iter().map(|((w, k), r)| format!("{}/{:?}: {}", m.tok_name(*w), k.map(|x| m.atoms[x as usize].clone()), m.reasons[*r as usize])).collect();
                        eprintln!("perry: wpfacts explain: replay global {name} in_top {} writes {ws:?}", m.global_in_top);
                    }
                    m.add_top(dst, |_| format!("globalThis.{name} written by unmodeled code"));
                    m.add_prim(dst, prim::ALL);
                    // Unmodeled code may also read what the program stored.
                    let r = m.reason(&format!("globalThis.{name} reaches unmodeled code"));
                    m.sink_node(f, r);
                }
                NativeMiss::Call(origin, call) => {
                    let native = m.tok_name(np);
                    let name = m.atoms[a as usize].clone();
                    let prev = m.cur_this.replace(origin);
                    unknown_method(m, call, &format!("argument of native {native}.{name}"));
                    m.cur_this = prev;
                }
            }
        }
    });
    keep.append(&mut m.native_missing);
    m.native_missing = keep;
}

/// The `cache` and `extensions` objects of [`BuiltinNs::Require`].
pub fn require_parts(m: &mut Model) -> (Tok, Tok) {
    if let Some(p) = m.require_parts {
        return p;
    }
    let c = anon_tmpl(m, "require.cache");
    let e = anon_tmpl(m, "require.extensions");
    m.require_parts = Some((c, e));
    (c, e)
}

fn anon_tmpl(m: &mut Model, what: &'static str) -> Tok {
    let site = m.anon_birth_site(what);
    let tm = m.tmpls.len() as TmplId;
    m.tmpls.push(Template {
        kind: TmplKind::BObj {
            module: u32::MAX,
            site,
            what,
        },
        display: what.to_string(),
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
    m.tok(TokKind::Obj(tm, NO_CTX))
}

/// Node's `module.isBuiltin` (prefix-only builtins need `node:`).
pub fn is_builtin_module(s: &str) -> bool {
    match s.strip_prefix("node:") {
        Some(rest) => perry_hir::is_node_builtin_module(rest),
        None => {
            !perry_hir::is_bare_prefix_only_builtin(s) && perry_hir::is_node_builtin_module(s)
        }
    }
}

/// Object.prototype's legacy members: the only `__` names the global object
/// inherits.
pub const OBJECT_PROTO_DUNDER: [&str; 5] = [
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// A global-object key the program owns: the host defines no global whose
/// name starts with `__` beyond `Object.prototype`'s legacy members (the
/// test `host_defines_no_dunder_global` reads the runtime's global-object
/// sources). Such a global holds what the program stored, plus ⊤ once
/// something unmodeled may write it (`native_written`, as for native
/// prototypes).
pub fn global_user_key(name: &str) -> bool {
    name.starts_with("__") && !OBJECT_PROTO_DUNDER.contains(&name) && name != crate::STAR_KEY
}

/// The global object's field under program-owned key `a`.
pub fn global_field(m: &mut Model, g: Tok, a: Atom) -> NodeId {
    if let Some(&(_, n)) = m.global_fields.iter().find(|(k, _)| *k == a) {
        return n;
    }
    let n = m.field(g, a);
    m.global_fields.push((a, n));
    n
}

fn native_method(m: &mut Model, np: Tok, native: &str, origin: Tok, name: &str, call: CallId) {
    let a = m.atom(name);
    if native_skips(m, np, Key::Named(a), NativeMiss::Call(origin, call)) {
        return;
    }
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
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Tok(origin),
                    args: c.args.iter().skip(1).copied().collect(),
                    spread: c.spread.clone(),
                    ret,
                    site: call2,
                },
            );
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
