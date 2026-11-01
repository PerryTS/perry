//! Builtin summaries: what runtime-provided objects and functions do with
//! the values they receive (design 3.2, "runtime builtin call").
//!
//! Every summary over-approximates the runtime entry it stands for. Anything
//! without a summary falls back to `unknown_method`: arguments escape to ⊤
//! and the result is ⊤. A missing summary costs precision, never soundness.

use crate::model::*;

mod collections;
mod constructors;
pub use constructors::{construct_named, new_builtin_ns};
mod members;
mod native_protocol;
pub(crate) mod string_methods;
pub(crate) mod typed_array_methods;
use collections::{array_method, bobj_method, function_method, iterate};
pub(crate) use collections::to_prim;
pub(crate) use collections::{WK_ITERATOR, WK_PREFIX};
use members::{Family, Mem};
pub use native_protocol::{
    global_field, global_user_key, is_builtin_module, native_parent_ctor, native_parent_keys,
    native_parent_summarized, native_proto, native_written, note_global_in_top, note_native_write,
    require_parts, settle_native_given,
};
use native_protocol::{native_method, native_skips};

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
        | "URIError" => BuiltinNs::Error,
        "AggregateError" => BuiltinNs::AggregateError,
        "Date" => BuiltinNs::Date,
        "RegExp" => BuiltinNs::RegExp,
        "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
        | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
        | "BigUint64Array" | "Uint8ClampedArray" | "DataView" => BuiltinNs::TypedArray,
        "ArrayBuffer" | "SharedArrayBuffer" => BuiltinNs::ArrayBuffer,
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
    interpreted_code_runs(m);
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

/// DECISIONS 235: interpreted code may run, from a real eval-like source
/// site or a solved call or construct that may be the Function constructor
/// (a ⊤ callee, the constructor token, the constructor given to unknown
/// code). With the interpreter linked it reads and writes any global (E20;
/// the prototype-write guard does not cover it) and catches what compiled
/// code throws into its `try`. Without it the Function constructor throws.
pub fn interpreted_code_runs(m: &mut Model) {
    if !m.interp_linked || m.interp_runs {
        return;
    }
    m.interp_runs = true;
    let gt = m.tok(crate::model::TokKind::Builtin(BuiltinNs::GlobalThis));
    note_native_write(m, gt, None, "interpreted code may run");
    let r = m.reason("thrown where interpreted code can catch it");
    let thrown = m.thrown;
    m.thrown_reason = Some(r);
    m.sink_node(thrown, r);
}

fn unknown_method(m: &mut Model, call: CallId, why: &str) {
    unknown_method_proto(m, call, why, None, false)
}

/// `unknown_method`, where argument `proto` becomes a prototype
/// (`Object.create`, `setPrototypeOf`): reachable only along the prototype
/// chain of the object it ends up in, it escapes as the prototypes of an
/// escaped value do (`Model::chain_sinks`), no runtime-owned object given
/// to unknown code. `own`: the method is the runtime's own method of its
/// receiver (a builtin namespace's, a builtin function's), which gives that
/// object to no one.
fn unknown_method_proto(m: &mut Model, call: CallId, why: &str, proto: Option<usize>, own: bool) {
    m.diagnostic_builtin(call, |_| format!("opaque-summary:{why}"));
    let c = m.calls[call as usize].clone();
    let r = m.reason(why);
    for (i, &a) in c.args.iter().enumerate() {
        if proto == Some(i) {
            let n = m.node_l("proto-arg");
            m.chain_sinks.insert(n);
            m.cons(n, Cons::TopSink { reason: r });
            m.edge(a, n);
        } else {
            m.sink_node(a, r);
        }
    }
    for &a in &c.spread {
        m.sink_node(a, r);
    }
    // A method run on one receiver token sees that token as `this`, not
    // every token of the receiver node.
    let kept = |m: &Model, t: Tok| own && m.named_shared(t).is_some();
    match (m.cur_this, &c.this) {
        (Some(t), _) if kept(m, t) => {}
        (Some(t), _) => m.escape(t, r),
        (None, ThisArg::Node(n)) => m.sink_node(*n, r),
        (None, ThisArg::Tok(t)) if kept(m, *t) => {}
        (None, ThisArg::Tok(t)) => m.escape(*t, r),
        (None, ThisArg::Undef) => {}
    }
    m.add_top(c.ret, |_| format!("result: {why}"));
    m.add_prim(c.ret, prim::ALL);
}

fn implicit_args(m: &mut Model, call: CallId, why: &str, proto: Protocol) {
    let c = m.calls[call as usize].clone();
    let r = m.reason(why);
    for &a in c.args.iter().chain(c.spread.iter()) {
        m.cons(a, Cons::Implicit { reason: r, proto });
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
    let ek = m.elem_key();
    for &s in &c.spread {
        m.cons(s, Cons::Load { key: ek, dst: out });
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
        declared: Vec::new(),
        reserved: Vec::new(),
        parent: None,
        ctor: None,
        native_parent: None,
        accessor_keys: vec![],
        has_computed_members: false,
        field_init: None,
        proto_members: vec![],
        parent_value: None,
        parent_links: Vec::new(),
        null_proto: false,
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
        gen_async: false,
        gen_reyields: false,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
    });
    m.add_prim(ret, prim::UNDEF);
    m.resolvers.insert(dst, tok);
    tok
}

/// Another unit's accessor on a shared object (`Model::replay_shared`), a
/// function this unit cannot see: what a lookup reached it from (its
/// receiver) and what a setter is given reach that unit's code, and a
/// getter returns an unknown value.
pub fn other_unit_accessor(m: &mut Model) -> Tok {
    if let Some(t) = m.other_accessor {
        return t;
    }
    let fx = m.funcs.len() as FnIdx;
    let tok = m.tok(TokKind::Func(fx, NO_CTX));
    let this = m.node_l("this(another unit's accessor)");
    let ret = m.node_l("ret(another unit's accessor)");
    let args_all = m.node_l("args(another unit's accessor)");
    let p = m.node_l("another unit's accessor-param");
    let r = m.reason("given to another unit's accessor");
    m.sink_node(this, r);
    m.sink_node(p, r);
    m.add_top(ret, |_| "result of another unit's accessor".into());
    m.add_prim(ret, prim::ALL);
    m.funcs.push(FuncInfo {
        module: 0,
        func_id: u32::MAX,
        name: "<another unit's accessor>".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all,
        has_arguments: false,
        token: tok,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_step: false,
        gen_async: false,
        gen_reyields: false,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
    });
    m.other_accessor = Some(tok);
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

// ------------------------------------------------ intrinsic members

/// May key `a` have been written onto a builtin prototype (`Object.prototype`
/// stands for every one of them here): a store, an accessor, a delete or a
/// reach into unmodeled code on it, another unit's such write, or a store
/// into ⊤ (a ⊤ value may be any builtin prototype: `native_written`)?
/// Until then the intrinsic's description (`members`) decides what a
/// builtin object or primitive reads under `a`.
pub fn intrinsic_written(m: &mut Model, a: Atom) -> bool {
    let op = m.tok(TokKind::ObjectProto);
    native_written(m, op, a)
}

/// Key `a` is absent on the intrinsic chain of a builtin object or
/// primitive: recorded, so that the lookup runs as an opaque member if the
/// key turns out written onto a builtin prototype later in the solve
/// (`replay_native_missing`). `false`: it may be written already, the
/// caller gives the opaque answer now.
fn intrinsic_miss(m: &mut Model, a: Atom, miss: NativeMiss) -> bool {
    if intrinsic_written(m, a) {
        return false;
    }
    let op = m.tok(TokKind::ObjectProto);
    m.native_missing.push((op, a, miss));
    true
}

/// The opaque builtin function value; in explain mode, counted by the
/// arm that minted it (`perry: wpfacts explain: builtin-fn minted`).
fn builtin_fn(m: &mut Model, why: &'static str) -> Tok {
    if m.explain {
        *m.bf_mints.entry(why).or_insert(0) += 1;
    }
    let t = m.tok(TokKind::BuiltinFn);
    t
}

/// The opaque member: what a key a store into ⊤ (or unmodeled code) may
/// have written onto a builtin prototype holds: any value (⊤), a builtin
/// function value, any primitive.
fn opaque_member(m: &mut Model, dst: NodeId) {
    let bf = builtin_fn(m, "opaque member (written key)");
    m.add_tok(dst, bf);
    m.add_top(dst, |_| {
        "member of a builtin prototype written through ⊤".into()
    });
    m.add_prim(dst, prim::ALL);
}

/// A member of an intrinsic the model does not describe (a typed array's,
/// a `Date`'s): a builtin function value or any primitive. Unless written
/// (`opaque_member`), it is no value of the program.
fn undescribed_member(m: &mut Model, dst: NodeId) {
    let bf = builtin_fn(m, "undescribed member");
    m.add_tok(dst, bf);
    m.add_prim(dst, prim::ALL);
}

/// A read on `ObjectProto` itself, not through a chain (`Function.prototype
/// .call`, `String.prototype.replace`): it stands for every builtin
/// prototype, the undescribed ones included, so any key but `constructor`
/// (`tok_load`) may name a builtin function or a primitive member.
pub fn intrinsic_proto_own(m: &mut Model, key: Key, dst: NodeId) {
    if m.is_constructor_key(key) {
        return;
    }
    // A method no builtin prototype overrides is `Object.prototype`'s on
    // every one of them; replaced (DECISIONS 234's writes), it is the
    // opaque member.
    if let Key::Named(a) = key {
        if members::object_proto_own(&m.atoms[a as usize]) {
            let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoOwn(a)));
            m.add_tok(dst, f);
            if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
            return;
        }
    }
    undescribed_member(m, dst);
}

/// The family's `constructor`.
fn family_ctor(m: &mut Model, fam: Family, dst: NodeId) {
    let nss: &[BuiltinNs] = match fam {
        Family::Object => &[BuiltinNs::Object],
        Family::Function => &[BuiltinNs::Function],
        Family::Array => &[BuiltinNs::Array],
        Family::Str => &[BuiltinNs::String],
        Family::Num | Family::BigInt => &[BuiltinNs::Number],
        Family::Bool => &[BuiltinNs::Boolean],
        Family::Sym => &[BuiltinNs::Symbol],
        Family::Collection => &[BuiltinNs::Map, BuiltinNs::Set],
        Family::Promise => &[BuiltinNs::Promise],
        Family::Error => &[BuiltinNs::Error],
        Family::Box => &[BuiltinNs::Object, BuiltinNs::Array],
        Family::Wrapper => &[
            BuiltinNs::Object,
            BuiltinNs::String,
            BuiltinNs::Number,
            BuiltinNs::Boolean,
            BuiltinNs::Symbol,
        ],
        // %Iterator% and the constructors of undescribed families: opaque.
        Family::Iterator | Family::Opaque => {
            let bf = builtin_fn(m, "constructor of an undescribed family");
            m.add_tok(dst, bf);
            return;
        }
    };
    for &ns in nss {
        let b = m.tok(TokKind::Builtin(ns));
        m.add_tok(dst, b);
    }
}

/// `dst` ⊇ what a builtin object of family `fam` reads under named key `a`
/// from its intrinsic chain: the described member, or the opaque member
/// once the key may have been written (own data fields come from the field
/// storage, `load_from`; an absent key reads nothing more).
fn family_load(m: &mut Model, fam: Family, a: Atom, dst: NodeId) {
    let name = m.atoms[a as usize].clone();
    if fam == Family::Opaque {
        undescribed_member(m, dst);
        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
            opaque_member(m, dst);
        }
        return;
    }
    match members::member(fam, &name) {
        Some(Mem::Fn) => {
            let bf = builtin_fn(m, "described member");
            m.add_tok(dst, bf);
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        Some(Mem::Ctor) => family_ctor(m, fam, dst),
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => {}
    }
    // Described or not, a key a store into ⊤ may have written onto the
    // chain reads what was stored (a described member may be replaced).
    if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// `dst` ⊇ what builtin namespace `ns` (token `t`) reads under named key
/// `a`: its described static, else its chain's member (`Function.prototype`
/// or `Object.prototype`), else nothing (absent: `undefined`). A key the
/// program may have written onto the namespace itself, or onto a builtin
/// prototype of its chain, reads the opaque member.
fn ns_load(m: &mut Model, t: Tok, ns: BuiltinNs, a: Atom, dst: NodeId) {
    let name = m.atoms[a as usize].clone();
    let Some(mem) = members::ns_member(ns, &name) else {
        undescribed_member(m, dst);
        if native_written(m, t, a) {
            return opaque_member(m, dst);
        }
        m.native_missing.push((t, a, NativeMiss::Load(dst)));
        return;
    };
    if native_written(m, t, a) {
        return opaque_member(m, dst);
    }
    // A write of the key onto the namespace later in the solve.
    m.native_missing.push((t, a, NativeMiss::Load(dst)));
    match mem {
        Some(Mem::Fn) => {
            let bf = builtin_fn(m, "builtin namespace static");
            m.add_tok(dst, bf);
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        // `@@species` is the constructor itself; `constructor` is the
        // chain's (`Function` for a constructor, `Object` for a namespace).
        Some(Mem::Ctor) if name == "@@__perry_wk_species" => m.add_tok(dst, t),
        Some(Mem::Ctor) => family_ctor(m, members::ns_chain(ns), dst),
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => m.add_prim(dst, prim::UNDEF),
    }
    // Absent on the namespace and on its chain: a key the program writes
    // onto a builtin prototype is a member of the chain then.
    if mem.is_none() && !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

// -------------------------------------------------------------- loads

pub fn prim_load(m: &mut Model, prim: u16, key: Key, dst: NodeId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return; // TypeError
    }
    let a = match key {
        // The elements of a string: its characters.
        k if m.is_elem_key(k) || m.is_iter_key(k) => {
            return m.add_prim(dst, prim::STR | prim::UNDEF);
        }
        // A computed key may name any member of the chain, or a character.
        Key::Any => {
            m.add_prim(dst, prim::STR | prim::NUM | prim::UNDEF | prim::BOOL);
            let bf = builtin_fn(m, "primitive computed key");
            m.add_tok(dst, bf);
            return;
        }
        Key::Named(a) => a,
    };
    let name = m.atoms[a as usize].clone();
    if members::is_index_name(&name) {
        return m.add_prim(dst, prim::STR | prim::UNDEF);
    }
    // A kind whose chain lacks the member reads `undefined`.
    if !members::prim_families(prim).all(|f| members::member(f, &name).is_some()) {
        m.add_prim(dst, prim::UNDEF);
    }
    match members::prim_member(prim, &name) {
        Some(Mem::Fn) => {
            let bf = builtin_fn(m, "primitive member");
            m.add_tok(dst, bf);
        }
        Some(Mem::Prim(p)) => m.add_prim(dst, p),
        Some(Mem::Ctor) => {
            for fam in members::prim_families(prim) {
                family_ctor(m, fam, dst);
            }
        }
        Some(Mem::Proto) => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
        }
        None => {}
    }
    // Described or not: a key written onto the chain reads what was stored.
    if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
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
        TokKind::Builtin(BuiltinNs::GlobalThis) => {
            match name.as_deref() {
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
                                    (*w == TOP || *w == t || Some(*w) == op)
                                        && (k.is_none() || *k == Some(a))
                                })
                                .map(|((w, k), r)| {
                                    format!(
                                        "{}/{:?}: {}",
                                        m.tok_name(*w),
                                        k.map(|x| m.atoms[x as usize].clone()),
                                        m.reasons[*r as usize]
                                    )
                                })
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
                    // A host global, or under a computed key any global: the
                    // Function constructor only under a key that may be
                    // "Function" (E14, DECISIONS 235). The program stores
                    // into the global object only through `__` keys or as
                    // an escape.
                    if name.is_none() {
                        let f = m.tok(TokKind::Builtin(BuiltinNs::Function));
                        m.add_tok(dst, f);
                    }
                    m.add_top(dst, |_| {
                        format!("globalThis.{}", name.as_deref().unwrap_or("[computed]"))
                    });
                    m.add_prim(dst, prim::ALL);
                }
            }
        }
        TokKind::Builtin(BuiltinNs::Require) => match name.as_deref() {
            Some("cache") | Some("extensions") => {
                require_parts(m);
                let p = if name.as_deref() == Some("cache") {
                    m.cjs_cache
                } else {
                    m.cjs_ext
                };
                m.edge(p, dst);
            }
            Some("main") => {
                let u = m.cjs_union;
                m.edge(u, dst);
                m.add_prim(dst, prim::UNDEF | prim::NULL);
            }
            _ => {
                let bf = builtin_fn(m, "require.<x>");
                m.add_tok(dst, bf);
                m.add_top(dst, |_| {
                    format!("require.{}", name.as_deref().unwrap_or("[computed]"))
                });
                m.add_prim(dst, prim::ALL);
            }
        },
        TokKind::Builtin(BuiltinNs::Process) => {
            // `process.mainModule` is the CommonJS main record the support
            // module installed (or `undefined`); a computed key may name it.
            if matches!(name.as_deref(), Some("mainModule") | None) {
                let main = m.cjs_main;
                m.edge(main, dst);
            }
            if name.as_deref() != Some("mainModule") {
                m.add_top(dst, |_| {
                    format!("process.{}", name.as_deref().unwrap_or("[computed]"))
                });
                m.add_prim(dst, prim::ALL);
            }
        }
        // A method has no `prototype`.
        TokKind::Builtin(BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_))
            if name.as_deref() == Some("prototype") =>
        {
            m.add_prim(dst, prim::UNDEF)
        }
        TokKind::Builtin(ns) => match name.as_deref() {
            Some("@@__perry_wk_species") if matches!(ns, BuiltinNs::TypedArray | BuiltinNs::ArrayBuffer) => {
                m.add_tok(dst, t);
                let Key::Named(a) = key else { unreachable!() };
                if !intrinsic_miss(m, a, NativeMiss::Load(dst)) { opaque_member(m, dst); }
            }
            Some("prototype") => {
                let op = m.tok(TokKind::ObjectProto);
                m.add_tok(dst, op);
            }
            // Its `constructor` is its chain's (`Function` for a
            // constructor, E14; `Object` for a namespace object).
            Some(n) if members::ns_member(ns, n).is_some() => {
                let Key::Named(a) = key else { unreachable!() };
                ns_load(m, t, ns, a, dst);
            }
            _ => {
                let bf = builtin_fn(m, "builtin namespace static");
                m.add_tok(dst, bf);
                m.add_prim(dst, prim::ALL);
                // A computed key may name the chain's `constructor`.
                if name.is_none() {
                    family_ctor(m, members::ns_chain(ns), dst);
                }
            }
        },
        TokKind::Func(f, _) => match name.as_deref() {
            Some("constructor") => {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
            // DECISIONS 210: another unit's constructor function: its
            // prototype is its owner's layout's once the owner published it
            // (a store there is this unit's census report); before that an
            // unknown value, never a prototype of this unit's own.
            Some("prototype") if m.foreign_ctor.contains_key(&f) => match m.foreign_ctor[&f] {
                Some((_, tm)) => {
                    let p = m.tok(TokKind::Proto(tm));
                    let a = m.atom("prototype");
                    let fl = m.field(t, a);
                    m.add_tok(fl, p);
                }
                None => {
                    // Beyond the contract: the owner's chain would take
                    // what is stored through it unseen. Reported, so the
                    // owner escapes the function (its chain is ⊤ there).
                    m.fn_touched.insert(t);
                    m.add_top(dst, |_| {
                        "prototype of another unit's constructor function (layout not published)"
                            .into()
                    });
                    m.add_prim(dst, prim::ALL);
                }
            },
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
                let bf = builtin_fn(m, "function call/apply/bind/toString");
                m.add_tok(dst, bf);
                let Key::Named(a) = key else { unreachable!() };
                if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            // An inherited key a store into ⊤ may have written onto
            // `Function.prototype` (or `Object.prototype`).
            Some(_) => {
                m.add_prim(dst, prim::UNDEF);
                let Key::Named(a) = key else { unreachable!() };
                if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                    opaque_member(m, dst);
                }
            }
            None => m.add_prim(dst, prim::UNDEF),
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
            TmplKind::Arr { .. } => match (key, name.as_deref()) {
                (_, Some("length")) => m.add_prim(dst, prim::NUM),
                // A computed key may name a builtin method too.
                (_, None) => {
                    let bf = builtin_fn(m, "array computed key");
                    m.add_tok(dst, bf);
                    m.add_prim(dst, prim::UNDEF | prim::NUM);
                }
                // An index names an element.
                (_, Some(n)) if members::is_index_name(n) => {
                    let all = m.all_fields(t);
                    m.edge(all, dst);
                    m.add_prim(dst, prim::UNDEF);
                }
                (Key::Named(a), Some(_)) => family_load(m, Family::Array, a, dst),
                (Key::Any, Some(_)) => unreachable!(),
            },
            TmplKind::BObj { what, .. } => {
                let fam = members::bobj_family(what);
                match (key, name.as_deref()) {
                    (_, Some("constructor")) if what == "TypedArray" => {
                        let ctor = m.tok(TokKind::Builtin(BuiltinNs::TypedArray));
                        m.add_tok(dst, ctor);
                        let Key::Named(a) = key else { unreachable!() };
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) { opaque_member(m, dst); }
                    }
                    (_, Some("buffer")) if what == "TypedArray" => {
                        // B1: the backing buffer is observed (it can be
                        // transferred, detached or wrapped by another view).
                        let r = m.reason("B1 .buffer read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_tok(dst, t);
                    }
                    (_, Some("byteOffset")) if what == "TypedArray" => {
                        let r = m.reason("B1 .byteOffset read");
                        m.mark_hard(t, Hard::B1Exposed, r);
                        m.add_prim(dst, prim::NUM)
                    }
                    (_, Some("size" | "length" | "byteLength" | "byteOffset"))
                        if fam == Family::Opaque =>
                    {
                        m.add_prim(dst, prim::NUM);
                        let Key::Named(a) = key else { unreachable!() };
                        if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                            opaque_member(m, dst);
                        }
                    }
                    // `require.cache` / `require.extensions` are plain
                    // objects: their fields hold what was stored (by compiled
                    // code, or ⊤ from the loader, `call_builtin_ns`); an
                    // absent key reads `undefined`.
                    _ if what == "require.cache" || what == "require.extensions" => {
                        m.add_prim(dst, prim::UNDEF)
                    }
                    // A computed key: any member of the chain, and the
                    // container's own values (primitives, or an iterator
                    // result's `value`/`done`: all it has).
                    (_, None) => {
                        let bf = builtin_fn(m, "builtin object computed key");
                        m.add_tok(dst, bf);
                        if what == "Iterator" || what == "IterResult" {
                            m.add_prim(dst, prim::UNDEF);
                        } else {
                            m.add_prim(dst, prim::ALL);
                        }
                    }
                    (Key::Named(a), Some(_)) => {
                        // A container of primitives (split/match results,
                        // keys …) holds them under every key.
                        if what == "primbox" {
                            m.add_prim(dst, prim::ALL);
                        }
                        family_load(m, fam, a, dst);
                    }
                    (Key::Any, Some(_)) => unreachable!(),
                }
            }
            _ => {}
        },
        TokKind::ObjectProto if name.as_deref() == Some("constructor") || name.is_none() => {
            // Object.prototype stands for every builtin prototype here:
            // its `constructor` may be Object or Function (E14). On a chain
            // its object's kind says which one it is (`chain_load`).
            for ns in [BuiltinNs::Object, BuiltinNs::Function] {
                let b = m.tok(TokKind::Builtin(ns));
                m.add_tok(dst, b);
            }
            m.add_prim(dst, prim::UNDEF);
            if name.is_none() {
                any_written(m, dst);
            }
        }
        // A program object's lookup that reaches `Object.prototype`: a key
        // a store into ⊤ may have written there reads what was stored.
        TokKind::ObjectProto => {
            m.add_prim(dst, prim::UNDEF);
            let Key::Named(a) = key else { unreachable!() };
            if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
        }
        TokKind::ModNs(_) | TokKind::Proto(_) => m.add_prim(dst, prim::UNDEF),
        // A key the native prototype does not define: the chain goes on.
        TokKind::NativeProto(_) if native_skips(m, t, key, NativeMiss::Load(dst)) => {}
        // A builtin function value (or a bound function): the members of
        // `Function.prototype`; a computed key may name any of them.
        TokKind::BuiltinFn | TokKind::Bound(_) if matches!(key, Key::Named(_)) => {
            let Key::Named(a) = key else { unreachable!() };
            family_load(m, Family::Function, a, dst);
        }
        TokKind::NativeProto(_) | TokKind::BuiltinFn | TokKind::Bound(_) => {
            let bf = builtin_fn(m, "native proto / builtin fn computed key");
            m.add_tok(dst, bf);
            m.add_prim(dst, prim::ALL);
            // A function's `constructor` is the Function constructor (E14);
            // a native prototype's is its native class (the builtin
            // function above).
            if matches!(name.as_deref(), Some("constructor") | None)
                && !matches!(m.toks[t as usize], TokKind::NativeProto(_))
            {
                let fc = m.tok(TokKind::Builtin(BuiltinNs::Function));
                m.add_tok(dst, fc);
            }
        }
        TokKind::Top => {}
    }
}

/// `tok_load` of prototype `q` on the chain of `of` (`Cons::ChainUnion`).
/// `ObjectProto` collapses the builtin prototypes; where `of`'s kind put it
/// there it is one of them (`seed_family`), and the members it reads under
/// `constructor` or a computed key are that family's chain's (`members`,
/// as on a builtin object itself): only a function's chain reaches
/// `Function.prototype`, whose `constructor` is the Function constructor
/// (DECISIONS 213). A site builtin object's chain (a typed array's ...)
/// under a computed key stays `tok_load`'s.
pub fn chain_load(m: &mut Model, of: Tok, q: Tok, key: Key, dst: NodeId) {
    if !matches!(m.toks[q as usize], TokKind::ObjectProto) || !m.is_constructor_key(key) {
        return tok_load(m, q, key, dst);
    }
    let Some(fam) = seed_family(m, of) else {
        return tok_load(m, q, key, dst);
    };
    match key {
        Key::Named(a) => family_load(m, fam, a, dst),
        Key::Any if is_site_builtin(m, of) => tok_load(m, q, key, dst),
        Key::Any => family_any(m, fam, dst),
    }
}

/// The intrinsic family whose chain `ObjectProto` stands for on the chain
/// of `t`, when `t`'s kind puts it there (`proto_node` seeds it): a
/// function's is `Function.prototype` (with the async/generator function
/// prototypes above it), an array's `Array.prototype`, a site builtin
/// object's its own builtin's, a namespace's its chain's, anything else's
/// `Object.prototype`. `None`: the chain's prototypes are values (`new
/// f()`, `extends <value>`, `Object.create(p)`, whose seed is no prototype
/// it has) or a class's. A builtin prototype given as a value later adds ⊤
/// to the chain (`Cons::ProtoGiven`).
fn seed_family(m: &Model, t: Tok) -> Option<Family> {
    match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Class { .. } | TmplKind::FnInst { .. } | TmplKind::ObjCreate { .. } => None,
            _ if m.virt_protos.contains_key(&t) => None,
            TmplKind::Arr { .. } => Some(Family::Array),
            TmplKind::BObj { what, .. } => Some(members::bobj_family(what)),
            TmplKind::Lit { .. } => Some(Family::Object),
        },
        TokKind::Proto(tm) => {
            let tm = &m.tmpls[*tm as usize];
            (tm.parent.is_none() && tm.native_parent.is_none() && tm.parent_value.is_none())
                .then_some(Family::Object)
        }
        TokKind::NativeProto(_) => Some(Family::Object),
        TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn => Some(Family::Function),
        TokKind::ClassObj(tm) => {
            let tm = &m.tmpls[*tm as usize];
            (tm.parent.is_none() && tm.parent_value.is_none()).then_some(Family::Function)
        }
        TokKind::Builtin(ns) => Some(members::ns_chain(*ns)),
        _ => None,
    }
}

fn is_site_builtin(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { .. }))
}

/// `dst` ⊇ what a key that may name any member reads on family `fam`'s
/// intrinsic chain: its `constructor`, its own prototype's members (opaque
/// builtin functions, data kinds), `Object.prototype`'s methods
/// (`BuiltinNs::ObjectProtoMethod`), and what a store into ⊤ wrote. Not
/// `__proto__`: the prototype would be `ObjectProto`, which stands for
/// every builtin prototype, as a value (an open gap, as before).
fn family_any(m: &mut Model, fam: Family, dst: NodeId) {
    family_ctor(m, fam, dst);
    let (fns, prims) = members::any_member(fam);
    if fns {
        let bf = builtin_fn(m, "computed key on a described chain");
        m.add_tok(dst, bf);
    }
    m.add_prim(dst, prims | prim::UNDEF);
    let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoMethod));
    m.add_tok(dst, f);
    any_written(m, dst);
}

/// A computed read of a builtin prototype may find any key a store into ⊤
/// wrote, now or later in the solve (`native_written` of `STAR_KEY`).
fn any_written(m: &mut Model, dst: NodeId) {
    let star = m.atom(crate::STAR_KEY);
    if !intrinsic_miss(m, star, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// An intrinsic prototype on `origin`'s chain read or called under a key
/// that names none of its members (a number, DECISIONS 217): only a number
/// key a store into ⊤ wrote there, now or later in the solve
/// (`native_written` of `NUM_KEY`), as a program prototype gives its
/// numeric fields (`Model::num_fields`).
pub fn plain_key(m: &mut Model, origin: Tok, act: ChainAct) {
    let num = m.atom(NUM_KEY);
    match act {
        ChainAct::Load(dst) => {
            if !intrinsic_miss(m, num, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
        }
        ChainAct::Call(call) => {
            if !intrinsic_miss(m, num, NativeMiss::Call(origin, call)) {
                let prev = m.cur_this.replace(origin);
                unknown_method(m, call, "argument of a written intrinsic key");
                m.cur_this = prev;
            }
        }
        ChainAct::Store(_) => {}
    }
}

/// `p[n](...)` on primitives of kinds `prim` under a number key: an index
/// of a string is a one-character string, of anything else absent; only a
/// number key a store into ⊤ wrote on the prototype is a method (DECISIONS 217).
pub fn prim_plain_key(m: &mut Model, prim: u16, call: CallId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    let num = m.atom(NUM_KEY);
    if !intrinsic_miss(m, num, NativeMiss::PrimCall(prim, call)) {
        unknown_method(m, call, "argument of a written primitive method");
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
            if m.explain && !m.shared_replay {
                let kn = match _key {
                    Key::Named(a) => m.atoms[a as usize].clone(),
                    Key::Any => "[computed]".into(),
                };
                eprintln!(
                    "perry: wpfacts explain: E20 store on {} key {kn}",
                    m.tok_name(t)
                );
            }
            let r = m.reason("E20 stored on a builtin/global object");
            m.sink_node(src, r);
        }
        _ => {}
    }
}

// ------------------------------------------------------------- methods

pub fn prim_method(m: &mut Model, prim: u16, key: Key, call: CallId) {
    if let Key::Named(a) = key {
        if matches!(
            members::prim_member(prim, &m.atoms[a as usize]),
            Some(Mem::Fn)
        ) {
            m.diagnostic_builtin(call, |m| {
                format!("primitive({prim}).{}", m.atoms[a as usize])
            });
        }
    }

    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    let Key::Named(a) = key else {
        // A computed key may name any method of the chain.
        return prim_method_generic(m, call);
    };
    let name = m.atoms[a as usize].clone();
    // A key a store into ⊤ may have written onto the chain: the call runs
    // what was stored (a described method may be replaced too).
    if !intrinsic_miss(m, a, NativeMiss::PrimCall(prim, call)) {
        unknown_method(
            m,
            call,
            &format!("argument of a written primitive method {name}"),
        );
    }
    match members::prim_member(prim, &name) {
        // `Object.prototype`'s methods on a primitive (no own chain here).
        Some(Mem::Fn) if name == "valueOf" => ret_prim(m, call, prim),
        Some(Mem::Fn) if name == "toString" || name == "toLocaleString" => {
            implicit_args(m, call, "ToString radix", Protocol::ToPrimitive);
            ret_prim(m, call, prim::STR);
        }
        Some(Mem::Fn)
            if matches!(
                name.as_str(),
                "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf"
            ) =>
        {
            implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
            ret_prim(m, call, prim::BOOL);
        }
        // Defines on the wrapper, which is discarded; nothing of the
        // program runs.
        Some(Mem::Fn) if name == "__defineGetter__" || name == "__defineSetter__" => {}
        Some(Mem::Fn) if name == "__lookupGetter__" || name == "__lookupSetter__" => {
            ret_prim(m, call, prim::UNDEF)
        }
        Some(Mem::Fn) if string_methods::string_method(m, call, &name) => {}
        Some(Mem::Fn) => prim_method_generic(m, call),
        // A data member is no function: TypeError.
        Some(_) => {}
        // Absent: a TypeError, unless written (above).
        None => {}
    }
}

/// A method of a primitive's chain the model does not describe one by one:
/// callbacks are called by the runtime with primitives (replace), object
/// arguments are coerced, the result is a primitive or a container of them.
fn prim_method_generic(m: &mut Model, call: CallId) {
    implicit_args(m, call, "argument of a primitive method", Protocol::Any);
    ret_prim(m, call, prim::ALL);
    let pb = site_tmpl(m, call, "primbox");
    ret_tok(m, call, pb);
}

/// `name` called on `t`, a builtin object of family `fam`, where the model
/// describes no such method: a member of the family's own intrinsic
/// prototype runs as an unknown method (`why`: the arguments and the
/// receiver escape, the result is ⊤); a member of `Object.prototype` runs
/// through the chain (`proto_method`); a data member is no function; a name
/// no intrinsic of the chain defines is absent (a TypeError), unless it may
/// have been written onto a builtin prototype.
fn undescribed_method(m: &mut Model, fam: Family, t: Tok, name: &str, call: CallId, why: &str) {
    match members::own(fam, name) {
        Some(Mem::Fn) if fam == Family::Wrapper => prim_method_generic(m, call),
        Some(Mem::Fn) => unknown_method(m, call, why),
        Some(_) => {}
        None if members::member(Family::Object, name).is_some() => {}
        None => {
            let a = m.atom(name);
            if !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
                unknown_method(m, call, why);
            }
        }
    }
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
    // A builtin function's `constructor` is the Function constructor (E14);
    // a computed key may name it.
    if let TokKind::Builtin(ns) = m.toks[t as usize] {
        if members::ns_chain(ns) == Family::Function
            && matches!(name.as_deref(), Some("constructor") | None)
        {
            return call_builtin_ns(m, BuiltinNs::Function, call);
        }
    }
    // A well-known-symbol method is a named member of the intrinsic chain
    // where the chain is described (a function value, an array, a described
    // builtin object: absent unless the description names it, as the
    // iteration protocol of the builtin iterables is); on anything else it
    // is a computed-key method.
    let described = match &m.toks[t as usize] {
        TokKind::Func(..) | TokKind::Bound(_) => true,
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Arr { .. } => true,
            TmplKind::BObj { what, .. } => {
                members::bobj_family(what) != Family::Opaque
                    || matches!(*what, "Date" | "TypedArray" | "URL" | "URLSearchParams" | "TextCodec" | "Timer")
                    || (*what == "RegExp"
                        && name.as_deref().is_some_and(|n| {
                            string_methods::regexp_described(n) || collections::regexp_species(n)
                        }))
            }
            _ => false,
        },
        _ => false,
    };
    let name = match name {
        Some(n) if n.starts_with(WK_PREFIX) && !described => None,
        n => n,
    };
    // A described receiver's method under a key a store into ⊤ may have
    // written onto its chain runs what was stored, beside the description.
    if let (true, Some(n), Key::Named(a)) = (described, name.as_deref(), key) {
        if !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
            unknown_method(m, call, &format!("argument of a written method {n}"));
        }
    }
    match m.toks[t as usize].clone() {
        // `Function.prototype.call/apply/bind` of a described builtin
        // method: its call, the receiver given.
        TokKind::Builtin(BuiltinNs::ObjectProtoOwn(_))
            if matches!(name.as_deref(), Some("call" | "apply" | "bind")) =>
        {
            function_method(m, t, name.as_deref(), call)
        }
        TokKind::Builtin(ns) => ns_method(m, ns, name.as_deref(), call),
        TokKind::Func(_, _) | TokKind::Bound(_) => function_method(m, t, name.as_deref(), call),
        // A builtin function value's methods are `Function.prototype`'s
        // (and `Object.prototype`'s): what one does with its arguments is
        // unknown (`push.call(o, v)` stores). Any other name is absent.
        TokKind::BuiltinFn => match name.as_deref() {
            None => {}
            Some(n) => {
                let why = format!("method {n} of an opaque builtin function");
                match members::member(Family::Function, n) {
                    Some(Mem::Fn) => unknown_method_proto(m, call, &why, None, true),
                    Some(_) => {}
                    None => {
                        let a = m.atom(n);
                        if !intrinsic_miss(m, a, NativeMiss::Call(t, call)) {
                            unknown_method_proto(m, call, &why, None, true);
                        }
                    }
                }
            }
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
    if matches!(m.toks[p as usize], TokKind::ObjectProto) {
        if let Key::Named(a) = key {
            if matches!(
                members::member(Family::Object, &m.atoms[a as usize]),
                Some(Mem::Fn)
            ) {
                m.diagnostic_builtin(call, |m| {
                    format!("Object.prototype.{}", m.atoms[a as usize])
                });
            }
        }
    }
    let prev = m.cur_this.replace(origin);
    proto_method_inner(m, p, origin, key, call);
    m.cur_this = prev;
}

/// A call of `BuiltinNs::ObjectProtoMethod`: any method of
/// `Object.prototype`. On a receiver it is each method's call on it
/// (`proto_method`). Without one, `hasOwnProperty` / `propertyIsEnumerable`
/// convert their key before they throw, `isPrototypeOf` of a primitive is
/// `false`, `toString` is `"[object Undefined]"`; the others throw
/// (ToObject of `undefined`).
fn object_proto_method_call(m: &mut Model, call: CallId) {
    match (m.cur_this, m.calls[call as usize].this.clone()) {
        (Some(o), _) => {
            let op = m.tok(TokKind::ObjectProto);
            for name in OBJECT_PROTO_METHODS {
                let a = m.atom(name);
                proto_method(m, op, o, Key::Named(a), call);
            }
        }
        (None, ThisArg::Undef) => {
            if let Some(k) = arg(m, call, 0) {
                let reason = m.reason("ToPropertyKey");
                m.cons(
                    k,
                    Cons::Implicit {
                        reason,
                        proto: Protocol::ToPrimitive,
                    },
                );
            }
            ret_prim(m, call, prim::STR | prim::BOOL);
        }
        // A receiver node no token stands for: its values are unknown here.
        (None, _) => unknown_method(m, call, "call of an Object.prototype method"),
    }
}

/// A call of `BuiltinNs::ObjectProtoOwn(a)`, whatever its receiver
/// (ToObject(this) throws on `undefined`/`null`, reads nothing else):
/// `hasOwnProperty` / `propertyIsEnumerable` convert their key and answer
/// a boolean; `isPrototypeOf` answers a boolean (its argument is only
/// walked: no conversion).
fn object_proto_own_call(m: &mut Model, a: Atom, call: CallId) {
    if m.atoms[a as usize] != "isPrototypeOf" {
        // The key only: further arguments are ignored.
        let c = m.calls[call as usize].clone();
        let r = m.reason("ToPropertyKey");
        for &k in c.args.iter().take(1).chain(c.spread.iter()) {
            m.cons(k, Cons::Implicit { reason: r, proto: Protocol::ToPrimitive });
        }
    }
    ret_prim(m, call, prim::BOOL);
}

/// `Object.prototype`'s methods, one per distinct summary in
/// `proto_method_inner`: `propertyIsEnumerable` / `isPrototypeOf` run as
/// `hasOwnProperty` does, `toLocaleString` as `toString`, the setter forms
/// as the getter forms.
const OBJECT_PROTO_METHODS: [&str; 5] = [
    "hasOwnProperty",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__lookupGetter__",
];

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
                implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
                ret_prim(m, call, prim::BOOL);
            }
            "toString" | "toLocaleString" => {
                ret_prim(m, call, prim::STR);
            }
            "valueOf" => ret_tok(m, call, origin),
            // No function to install: TypeError before anything else.
            "__defineGetter__" | "__defineSetter__"
                if m.calls[call as usize].args.len() < 2
                    && m.calls[call as usize].spread.is_empty() => {}
            "__defineGetter__" | "__defineSetter__" => {
                let r = m.reason("E5 __defineGetter__/__defineSetter__");
                m.mark_hard(origin, Hard::E5DefineProperty, r);
                m.escape(origin, r);
                // ToPropertyKey of the key; the function becomes an accessor
                // of the escaped receiver, which unknown code runs.
                let c = m.calls[call as usize].clone();
                if let Some(&k) = c.args.first() {
                    let reason = m.reason("ToPropertyKey");
                    m.cons(
                        k,
                        Cons::Implicit {
                            reason,
                            proto: Protocol::ToPrimitive,
                        },
                    );
                }
                let r = m.reason("E5 __defineGetter__ argument");
                for &a in c.args.iter().skip(1).take(1).chain(&c.spread) {
                    m.sink_node(a, r);
                }
                ret_prim(m, call, prim::UNDEF);
            }
            "__lookupGetter__" | "__lookupSetter__" => {
                implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
                m.implicit(origin, 0, Protocol::Any);
                ret_tok(m, call, TOP);
            }
            _ => {}
        },
        TokKind::NativeProto(name_of) => native_method(m, p, &name_of, origin, &name, call),
        _ => {}
    }
}

/// The runtime's global functions [`ns_method`] describes under
/// [`BuiltinNs::Other`] (`gc` and the timers, the number/URI helpers, the
/// primitive conversions); each ignores its `this`.
fn runtime_global_fn(n: &str) -> bool {
    matches!(
        n,
        "gc" | "setTimeout"
            | "setInterval"
            | "setImmediate"
            | "queueMicrotask"
            | "clearTimeout"
            | "clearInterval"
            | "clearImmediate"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "encodeURI"
            | "decodeURI"
            | "escape"
            | "unescape"
            | "atob"
            | "btoa"
            | "String"
            | "Number"
            | "Boolean"
            | "BigInt"
    )
}

fn ns_method(m: &mut Model, ns: BuiltinNs, name: Option<&str>, call: CallId) {
    if let Some(name) = name {
        if matches!(members::ns_member(ns, name), Some(Some(Mem::Fn)))
            || ns == BuiltinNs::Other && runtime_global_fn(name)
        {
            m.diagnostic_builtin(call, |_| format!("{ns:?}.{name}"));
        }
    }

    let Some(name) = name else {
        return;
    };
    match (ns, name) {
        (BuiltinNs::Other, n) if CONSOLE_METHODS.contains(&n) => {
            implicit_args(
                m,
                call,
                "console argument (formatted by the runtime)",
                Protocol::Any,
            );
            ret_prim(m, call, prim::UNDEF);
        }
        (BuiltinNs::Console, _) => {
            implicit_args(
                m,
                call,
                "console argument (formatted by the runtime)",
                Protocol::Any,
            );
            ret_prim(m, call, prim::UNDEF);
        }
        (BuiltinNs::Other, "setTimeout" | "setInterval" | "setImmediate" | "queueMicrotask") => {
            let c = m.calls[call as usize].clone();
            if !c.spread.is_empty() { return unknown_method(m, call, "spread timer arguments"); }
            // HTML timers: ToLong(timeout); callback receives precisely
            // extra arguments, with the global this. Node uses the Timeout
            // as this, so retain that host possibility beside the global.
            let timer = site_tmpl(m, call, "Timer");
            let this = m.node_l("timer-this"); m.add_tok(this, timer);
            let global = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis)); m.add_tok(this, global);
            let timed = matches!(name, "setTimeout" | "setInterval");
            if timed { if let Some(&delay) = c.args.get(1) { string_methods::to_prim(m, delay, "timer timeout ToNumber"); } }
            if let Some(&cb) = c.args.first() {
                let args = if name == "queueMicrotask" { vec![] } else { c.args.iter().skip(if timed { 2 } else { 1 }).copied().collect() };
                let this_arg = if name == "queueMicrotask" { ThisArg::Undef } else { ThisArg::Node(this) };
                let _ = collections::cb_call(m, call, cb, this_arg, args);
                // HTML also accepts string handlers (ToString), which may
                // run arbitrary script. Perry/Node only accept functions;
                // no object handler is retained in either case.
            }
            if name == "queueMicrotask" { ret_prim(m, call, prim::UNDEF); } else { ret_tok(m, call, timer); ret_prim(m, call, prim::NUM); }
        }
        (BuiltinNs::Other, "clearTimeout" | "clearInterval" | "clearImmediate") => {
            // HTML's long id conversion; Node's Timeout handle also has a
            // primitive id. Neither calls/retains surplus operands.
            if let Some(id) = arg(m, call, 0) { string_methods::to_prim(m, id, "clear timer id ToNumber"); }
            ret_prim(m, call, prim::UNDEF);
        }
        // The runtime's `gc([force])`: the argument is ignored.
        (BuiltinNs::Other, "gc") => ret_prim(m, call, prim::UNDEF),
        // A method of the global object named by one of the runtime's global
        // functions is that function, which ignores `this`: the global object
        // is not handed to unknown code by the call (`global.gc()`). A key the
        // program may write onto the global object (now or later in the
        // solve) is the opaque call as well.
        (BuiltinNs::GlobalThis, n) if runtime_global_fn(n) => {
            let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            let a = m.atom(n);
            if native_written(m, g, a) {
                let prev = m.cur_this.replace(g);
                unknown_method(m, call, &format!("argument of builtin GlobalThis.{n}"));
                m.cur_this = prev;
            } else {
                m.native_missing.push((g, a, NativeMiss::Call(g, call)));
                ns_method(m, BuiltinNs::Other, Some(n), call);
            }
        }
        (
            BuiltinNs::Other,
            "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "encodeURIComponent"
            | "decodeURIComponent" | "encodeURI" | "decodeURI" | "escape" | "unescape" | "atob"
            | "btoa" | "String" | "Number" | "Boolean" | "BigInt",
        ) => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
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
        (BuiltinNs::Object, "create") => {
            unknown_method_proto(m, call, "argument of builtin Object.create", Some(0), true)
        }
        (BuiltinNs::Object | BuiltinNs::Reflect, "setPrototypeOf") => unknown_method_proto(
            m,
            call,
            &format!("argument of builtin {ns:?}.setPrototypeOf"),
            Some(1),
            true,
        ),
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
                    let ek = m.elem_key();
                    m.cons(
                        s,
                        Cons::Load {
                            key: ek,
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
            // The value is serialized; a replacer is called back.
            let c = m.calls[call as usize].clone();
            let r = m.reason("JSON.stringify value");
            let rr = m.reason("JSON.stringify replacer");
            for (i, &a) in c.args.iter().enumerate() {
                let (reason, proto) = if i == 0 {
                    (r, Protocol::Json)
                } else {
                    (rr, Protocol::Any)
                };
                m.cons(a, Cons::Implicit { reason, proto });
            }
            for &a in &c.spread {
                m.cons(
                    a,
                    Cons::Implicit {
                        reason: rr,
                        proto: Protocol::Any,
                    },
                );
            }
            ret_prim(m, call, prim::STR | prim::UNDEF);
        }
        (BuiltinNs::Json, "parse") => {
            implicit_args(m, call, "JSON.parse text", Protocol::ToPrimitive);
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
        // Every Math function returns a Number (ECMA-262 Math).
        (BuiltinNs::Math, _) => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::NUM);
        }
        (BuiltinNs::Number, _) | (BuiltinNs::String, _) if name == "raw" => {
            // `String.raw` reads `raw` and its elements.
            implicit_args(m, call, "String.raw template", Protocol::Any);
            ret_prim(m, call, prim::ALL);
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
        }
        (BuiltinNs::Number, _) | (BuiltinNs::String, _) => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::ALL);
            if name == "raw" || name == "split" {
                let pb = site_tmpl(m, call, "primbox");
                ret_tok(m, call, pb);
            }
        }
        (BuiltinNs::Symbol, _) => {
            implicit_args(m, call, "ToString", Protocol::ToPrimitive);
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
            implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
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
        (_, n) => unknown_method_proto(
            m,
            call,
            &format!("argument of builtin {ns:?}.{n}"),
            None,
            true,
        ),
    }
}

pub fn call_builtin_ns(m: &mut Model, ns: BuiltinNs, call: CallId) {
    m.diagnostic_builtin(call, |_| format!("{ns:?}"));
    match ns {
        BuiltinNs::Require => {
            // `require('module')` is the node:module namespace, which hands
            // out `Module._cache`: the cache reaches code nothing models.
            if m.call_lit
                .get(&call)
                .is_some_and(|a| matches!(m.atoms[*a as usize].as_str(), "module" | "node:module"))
            {
                let r = m.reason("node:module namespace hands out Module._cache");
                let cache = m.cjs_cache;
                m.sink_node(cache, r);
            }
            // A compiled module named by a constant path: its exports (the
            // registry record's `exports`, as `require.cache[path]` holds).
            let rec = m
                .call_lit
                .get(&call)
                .and_then(|a| m.cjs_paths.get(a))
                .copied();
            if m.explain && rec.is_none() {
                let lit = m.call_lit.get(&call).map(|a| m.atoms[*a as usize].clone());
                let known: Vec<String> = m
                    .cjs_paths
                    .keys()
                    .map(|a| m.atoms[*a as usize].clone())
                    .collect();
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
                    m.cons(
                        unknown,
                        Cons::Load {
                            key: Key::Named(ea),
                            dst: ret,
                        },
                    );
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
                    implicit_args(m, call, "require specifier", Protocol::ToPrimitive);
                    let ret = m.calls[call as usize].ret;
                    // The registry part stays inside that ⊤ as on main.
                    // It caches the registry records it loads.
                    let r = m.reason("module loaded by a computed specifier");
                    let (cache, _) = require_parts(m);
                    let fa = m.field_any(cache);
                    let reg = m.cjs_union;
                    m.edge(reg, fa);
                    if let Some(a0) = m.calls[call as usize].args.first().copied() {
                        m.cons(
                            a0,
                            Cons::TopIfAny {
                                dst: ret,
                                from: ret,
                                reason: Some(r),
                            },
                        );
                    }
                }
            }
        }
        BuiltinNs::Function => {
            set_dyn_eval(m, "E14 Function(...) call");
            unknown_method(m, call, "E14 dynamic code");
        }
        BuiltinNs::ObjectProtoMethod => object_proto_method_call(m, call),
        BuiltinNs::ObjectProtoOwn(a) => object_proto_own_call(m, a, call),
        BuiltinNs::Error | BuiltinNs::AggregateError | BuiltinNs::RegExp => {
            new_builtin_ns(m, ns, call);
            if ns == BuiltinNs::RegExp {
                // `RegExp(re)` can return the existing regexp object.
                if let Some(src) = arg(m, call, 0) {
                    let ret = m.calls[call as usize].ret;
                    m.edge(src, ret);
                }
            }
        }
        BuiltinNs::Date => ret_prim(m, call, prim::STR),
        BuiltinNs::String | BuiltinNs::Number | BuiltinNs::Boolean | BuiltinNs::Symbol => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
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
            let c = m.calls[call as usize].clone();
            match (c.args.as_slice(), c.spread.is_empty()) {
                // One argument: a Number is the length, not an element.
                (&[a], true) => m.cons(a, Cons::ArrayLenArg { dst: fa }),
                _ => {
                    let vals = all_arg_values(m, call);
                    m.edge(vals, fa);
                }
            }
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
