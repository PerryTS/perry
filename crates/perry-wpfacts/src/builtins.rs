//! Builtin summaries: what runtime-provided objects and functions do with
//! the values they receive (design 3.2, "runtime builtin call").
//!
//! Every summary over-approximates the runtime entry it stands for. Anything
//! without a summary falls back to `unknown_method`: arguments escape to ⊤
//! and the result is ⊤. A missing summary costs precision, never soundness.

use crate::model::*;
use perry_abi::lock::LockHolder;

mod collections;
mod constructors;
mod effects;
#[cfg(test)]
pub(crate) mod effects_for_tests {
    pub(crate) use super::effects::interpretable;
    /// `Array.prototype.<name>` is declared in forms the interpreter runs.
    pub(crate) fn declared_array_method(name: &str) -> bool {
        super::effects::declared(super::Family::Array, name).is_some()
    }
}
pub use constructors::{construct_named, new_builtin_ns};
mod members;
mod native_protocol;
mod realm_declared;
pub(crate) mod realm_shapes;
mod undeclared;
pub(crate) use native_protocol::{census_native_op_writes, census_native_written};
pub use undeclared::{builtin_function, native_may_be, realm_reads_store, undeclared};
pub(crate) mod string_methods;
pub(crate) mod typed_array_methods;
pub(crate) use collections::construct_into;
pub(crate) use effects::{declared_member, run_native};
use collections::{array_method, array_method_on, bobj_method, function_method, iterate};
pub(crate) use collections::{WK_ITERATOR, WK_PREFIX};
/// The template kind of an array iterator (`%ArrayIteratorPrototype%`).
pub(crate) const ARRAY_ITERATOR_KIND: &str = members::ARRAY_ITERATOR;
pub use members::Family;
use members::Mem;
pub use native_protocol::{
    global_field, global_user_key, global_value_written, is_builtin_module, native_parent_ctor,
    native_parent_keys, native_parent_summarized, native_parent_value, native_proto,
    native_written, native_written_by, note_global_in_top, note_native_write, require_parts,
    settle_native_given,
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
        "%Global%" | "globalThis" | "global" | "window" | "self" => BuiltinNs::GlobalThis,
        "process" => BuiltinNs::Process,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError"
        | "URIError" => BuiltinNs::Error,
        "AggregateError" => BuiltinNs::AggregateError,
        "Date" => BuiltinNs::Date,
        "RegExp" => BuiltinNs::RegExp,
        "%TypedArray%" | "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array"
        | "Uint32Array" | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array" | "Uint8ClampedArray" | "DataView" => {
            BuiltinNs::TypedArray
        }
        "ArrayBuffer" | "SharedArrayBuffer" => BuiltinNs::ArrayBuffer,
        "Symbol" => BuiltinNs::Symbol,
        "Number" | "BigInt" => BuiltinNs::Number,
        "String" => BuiltinNs::String,
        "Boolean" => BuiltinNs::Boolean,
        "Function" => BuiltinNs::Function,
        "Map" => BuiltinNs::Map,
        "Set" => BuiltinNs::Set,
        "WeakMap" => BuiltinNs::WeakMap,
        "WeakSet" => BuiltinNs::WeakSet,
        _ => return node_module(name),
    })
}

/// The node module object the realm declares as `name` (`node:fs`).
fn node_module(name: &str) -> Option<BuiltinNs> {
    if !name.starts_with("node:") {
        return None;
    }
    crate::native_effects::realm()
        .iter()
        .find(|e| e.name == name && e.kind == perry_hir::realm_manifest::RealmKind::Namespace)
        .map(|e| BuiltinNs::Module(e.name))
}

/// The node module object `require(m)` or a namespace import of `m` names,
/// when the realm declares it (`fs` and `node:fs` alike).
pub(crate) fn node_module_ns(m: &str) -> Option<BuiltinNs> {
    let bare = m.strip_prefix("node:").unwrap_or(m);
    node_module(&format!("node:{bare}"))
}

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
    // It reads any built-in object's program fields by name (rule R1).
    m.builtin_fields_interp();
    let gt = m.tok(crate::model::TokKind::Builtin(BuiltinNs::GlobalThis));
    note_native_write(m, gt, None, "interpreted code may run");
    let r = m.reason("thrown where interpreted code can catch it");
    let thrown = m.thrown;
    // It throws what it holds too.
    m.add_top(thrown, |_| "thrown by interpreted code".into());
    m.add_prim(thrown, prim::ALL);
    m.thrown_reason = Some(r);
    m.sink_node(thrown, r);
}

/// Without the interpreter linked (`Model::interp_linked`: the driver's
/// link decision, the one that installs the runtime's `dyn-eval`), a call or
/// construct of the Function-constructor token runs no program code, so it
/// gives nothing to unknown code. The Function constructor converts every
/// argument to a string first (`toString`/`valueOf` run: the ToPrimitive
/// protocol), then throws a TypeError (perry-runtime
/// `js_function_ctor_from_strings_impl`). The other constructors the token
/// stands for as a function's `constructor` (`%AsyncFunction%`,
/// `%GeneratorFunction%`, `%AsyncGeneratorFunction%`) run nothing and return
/// `undefined`, or under `new` an object of the runtime's. Six arguments or
/// a spread may be the runtime's depd `wrapfunction` shape, which returns a
/// runtime-made function: that call runs code (true), as does any call with
/// the interpreter linked. `skip`: leading arguments that are not the
/// constructor's (`Function.call`'s `this`).
pub(crate) fn function_ctor_runs_code(
    m: &mut Model,
    call: CallId,
    skip: usize,
    construct: bool,
) -> bool {
    let c = &m.calls[call as usize];
    if m.interp_linked || !c.spread.is_empty() || c.args.len().saturating_sub(skip) == 6 {
        return true;
    }
    m.diagnostic_builtin(call, |_| {
        "Function constructor without an interpreter".into()
    });
    let c = m.calls[call as usize].clone();
    let r = m.reason("ToPrimitive");
    for &a in c.args.iter().skip(skip) {
        m.cons(
            a,
            Cons::Implicit {
                reason: r,
                proto: Protocol::ToPrimitive,
            },
        );
    }
    if construct {
        m.add_top(c.ret, |_| "result: new of a function constructor".into());
    }
    m.add_prim(c.ret, prim::UNDEF);
    false
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
    let recv = if own { Recv::Own } else { Recv::Given };
    unknown_method_at(m, call, why, proto, recv)
}

/// `unknown_method` called on a primitive receiver (a primitive's method a
/// store into ⊤ may have written onto its prototype): the method runs with
/// the primitive as `this`, so no object of the receiver node reaches it.
/// The arguments escape, the result is ⊤.
fn unknown_prim_method(m: &mut Model, call: CallId, why: &str) {
    unknown_method_at(m, call, why, None, Recv::Primitive)
}

/// What the unknown method's `this` is: the receiver given (it escapes),
/// the runtime's own object (`unknown_method_proto`'s `own`), or a
/// primitive (nothing escapes).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recv {
    Given,
    Own,
    Primitive,
}

fn unknown_method_at(m: &mut Model, call: CallId, why: &str, proto: Option<usize>, recv: Recv) {
    if m.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
        let d = m.diagnostic.as_mut().unwrap();
        let c = d.census.as_mut().unwrap();
        let saved = (d.cur_call.replace(call), c.active_gate);
        c.active_gate = c
            .pending_gate
            .take()
            .filter(|(_, _, is_call, id)| *is_call && *id == call)
            .map(|(t, a, _, _)| (t, a));
        unknown_method_proto_inner(m, call, why, proto, recv);
        let d = m.diagnostic.as_mut().unwrap();
        d.cur_call = saved.0;
        d.census.as_mut().unwrap().active_gate = saved.1;
        return;
    }
    unknown_method_proto_inner(m, call, why, proto, recv);
}
fn unknown_method_proto_inner(
    m: &mut Model,
    call: CallId,
    why: &str,
    proto: Option<usize>,
    recv: Recv,
) {
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
    let kept = |m: &Model, t: Tok| recv == Recv::Own && m.named_shared(t).is_some();
    match (m.cur_this, &c.this) {
        _ if recv == Recv::Primitive => {}
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
    if t == TOP && m.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
        // All non-literal TOP returns here forward the native receiver.
        // Prefer its actual node over the native callee's active rule node.
        let this_node = match m.calls[call as usize].this {
            ThisArg::Node(n) => Some(n),
            _ => None,
        };
        let cause = m.diagnostic.as_ref().unwrap().cause_node;
        let source = this_node
            .into_iter()
            .chain([m.cur_src])
            .chain(cause)
            .find(|&n| {
                m.nodes
                    .get(n as usize)
                    .is_some_and(|v| v.pts.contains_tok(TOP))
            });
        if let Some(source) = source {
            let c = m.diagnostic.as_mut().unwrap().census.as_mut().unwrap();
            c.top_edges.insert((source, ret));
            let saved = std::mem::replace(&mut c.injecting, true);
            m.add_tok(ret, t);
            m.diagnostic
                .as_mut()
                .unwrap()
                .census
                .as_mut()
                .unwrap()
                .injecting = saved;
        } else {
            // A synthetic call can supply TOP as a token, with no value
            // node: the first node receiving that external input is here.
            m.add_top(ret, |_| {
                "implicit native receiver from unmodeled code".into()
            });
        }
        return;
    }
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
    site_tmpl_born(m, call, what, false)
}

/// A per-call-site Array born without holes: the builtin that makes it
/// writes every index below its length, or exactly the indices of a source
/// whose holes it carries over (`collections::array_method`). Later stores
/// mark their own holes as on any array.
fn site_dense_arr(m: &mut Model, call: CallId) -> Tok {
    site_tmpl_born(m, call, "DenseArray", true)
}

fn site_tmpl_born(m: &mut Model, call: CallId, what: &'static str, hole_free: bool) -> Tok {
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
    let kind = if what == "Array" || hole_free {
        TmplKind::Arr {
            module: u32::MAX,
            site,
            hole_free,
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
        chain_miss: false,
        chain_miss_open: false,
        species: None,
        per_evaluation: false,
        static_methods: Vec::new(),
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
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: u32::MAX,
        raise: u32::MAX,
        name: "<resolver>".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all,
        has_arguments: false,
        own_test: crate::model::OwnTestBody::Unknown,
        token: tok,
        ctor_of: None,
        lexical_this: true,
        no_summary: false,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
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
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: u32::MAX,
        raise: u32::MAX,
        name: "<another unit's accessor>".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all,
        has_arguments: false,
        own_test: crate::model::OwnTestBody::Unknown,
        token: tok,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
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
            region: m.thrown,
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

/// A family of builtin receivers a NativeMember verdict is over
/// (`derive::native_member`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeFamily {
    /// Primitive Strings.
    Str,
    /// Array exotic objects (`derive::exact_array`).
    Array,
    /// Collections of one kind (`Map`, `Set`, `WeakMap`, `WeakSet`).
    Collection(&'static str),
    /// RegExp objects. Their chain's members here are the methods the
    /// solve's RegExp call model describes (`regexp_described`): the one
    /// predicate both read.
    RegExp,
}

/// Does the intrinsic chain of every value of `fam` define `name` before
/// the program writes anything (`members`): a function for a call (`call`),
/// a function or a data member for a read? `constructor` and `__proto__`
/// are no member here (their value is no builtin function or primitive).
pub(crate) fn native_described(fam: NativeFamily, name: &str, call: bool) -> bool {
    let mem = match fam {
        NativeFamily::Str => members::intrinsic_member(Family::Str, name),
        NativeFamily::Array => members::intrinsic_member(Family::Array, name),
        NativeFamily::Collection(what) => members::collection_member(what, name),
        NativeFamily::RegExp => string_methods::regexp_described(name).then_some(Mem::Fn),
    };
    match mem {
        Some(Mem::Fn) => true,
        Some(Mem::Prim(_)) => !call,
        _ => false,
    }
}

/// `Object.prototype`'s intrinsic description (`members`) lacks `name`
/// (`derive::absent_key`): `constructor`, `toString`, `__proto__` and the
/// other members are not absent.
pub(crate) fn object_proto_lacks(name: &str) -> bool {
    members::member(members::Family::Object, name).is_none()
}

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
    if m.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
        let op = m.tok(TokKind::ObjectProto);
        m.census_native_read(op, a, miss.clone());
    }
    if intrinsic_written(m, a) {
        return false;
    }
    let op = m.tok(TokKind::ObjectProto);
    m.native_missing.push((op, a, miss));
    true
}

/// [`intrinsic_miss`] for a lookup on the chain of a value of family `fam`
/// (a collection of kind `what`): a final read (`members::locked_read`,
/// DEC 250) is never written and needs no replay.
fn intrinsic_unwritten(
    m: &mut Model,
    fam: Family,
    what: Option<&str>,
    a: Atom,
    miss: NativeMiss,
) -> bool {
    members::locked_read(fam, what, &m.atoms[a as usize]) || intrinsic_miss(m, a, miss)
}

/// DEC 250: is the read of named key `a` on the intrinsic chain of builtin
/// receiver `t` final (`members::locked_read` of its family)? A native
/// prototype (a native class a program class extends) is not: past it the
/// collapsed token may stand for an add-open prototype (`%TypedArray%`,
/// `Error.prototype`), not `Object.prototype`.
pub(crate) fn locked_chain_read(m: &Model, t: Tok, a: Atom) -> bool {
    if matches!(m.toks[t as usize], TokKind::NativeProto(_)) {
        return false;
    }
    if let TokKind::Builtin(ns) = m.toks[t as usize] {
        let description = members::ns_statics(ns);
        // A concrete Function.prototype -> Object.prototype chain passes
        // only locked prototypes. A constructor on the chain is mutable;
        // an undescribed chain supplies no final read. Unknown own statics
        // do not change the described prototype chain.
        return matches!(
            description.prototype,
            members::NsProto::Function | members::NsProto::Object
        ) && members::locked_read(Family::Object, None, &m.atoms[a as usize]);
    }
    // A generator object (its step closure): past its own keys and its
    // function's `prototype` (program objects, read through their fields)
    // the chain reaches `%GeneratorPrototype%` / `%AsyncGeneratorPrototype%`,
    // whose declared image members are locked (DEC 250; `constructor`
    // exempt). Every other key may be added there: not final.
    if let TokKind::Func(f, _) = m.toks[t as usize] {
        let fi = &m.funcs[f as usize];
        if fi.gen_step() {
            let holder = if fi.gen_async {
                ("%AsyncGeneratorPrototype%", LockHolder::AsyncGenerator)
            } else {
                ("%GeneratorPrototype%", LockHolder::Generator)
            };
            return image_member(m, holder, &m.atoms[a as usize]);
        }
    }
    let what = match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::BObj { what, .. } => Some(*what),
            _ => None,
        },
        TokKind::ClassObj(_) => Some(members::FN_PROTO_ONLY),
        // A function the prototype-kind predicate says is born with
        // `%Function.prototype%`; not a generator's step closure, which
        // stands for the generator object (`FuncInfo::gen_step`).
        TokKind::Func(f, _) => born_with_function_proto(m, *f).then_some(members::FN_PROTO_ONLY),
        _ => None,
    };
    // A Map or Set iterator: past its own keys the chain reaches
    // `%MapIteratorPrototype%` / `%SetIteratorPrototype%`, whose declared
    // image members are locked (DEC 250); every other key may be added
    // there: not final.
    if let Some(holder) = what.and_then(members::iterator_prototype) {
        return image_member(m, holder, &m.atoms[a as usize]);
    }
    lock_family(m, t).is_some_and(|f| members::locked_read(f, what, &m.atoms[a as usize]))
}

/// Is a read of `name` on the declared intrinsic prototype `holder` final?
/// `holder` is its realm-table name (`%GeneratorPrototype%`,
/// `%MapIteratorPrototype%` …, the same declaration the runtime seals) and
/// its lock holder; the lock's one decision (`perry_abi::lock::final_on`)
/// classes the key: `constructor` stays writable, an image member is locked,
/// an absent key may be added.
fn image_member(m: &Model, (holder, lock): (&str, LockHolder), name: &str) -> bool {
    use perry_abi::lock::{final_on, LockKey};
    let key = if name == "constructor" {
        LockKey::Constructor
    } else if m
        .declarations
        .named(holder, m)
        .is_some_and(|h| m.declarations.property(h, name, m).is_some())
    {
        LockKey::Image
    } else {
        LockKey::Absent
    };
    final_on(lock, key)
}

/// The family whose intrinsic chain the lock sees on `t`: `seed_family`'s,
/// and for an instance of a parentless class (an object literal's shape
/// class included) `Object`: past its class prototype the chain is
/// `Object.prototype`, as for that prototype itself (`TokKind::Proto`).
/// A class object whose `extends` chain is program classes, down to a root
/// `seed_family` gives `Function`, has `Function` too: its chain is those
/// class objects, read through their own keys, then the root's.
fn lock_family(m: &Model, t: Tok) -> Option<Family> {
    match &m.toks[t as usize] {
        TokKind::ClassObj(c) => program_class_chain(m, *c).then_some(Family::Function),
        TokKind::Obj(tm, _) if !m.virt_protos.contains_key(&t) => match &m.tmpls[*tm as usize] {
            tm if matches!(tm.kind, TmplKind::Class { .. }) => {
                (tm.parent.is_none() && tm.native_parent.is_none() && tm.parent_value.is_none())
                    .then_some(Family::Object)
            }
            _ => seed_family(m, t),
        },
        _ => seed_family(m, t),
    }
}

/// Is every link of class `c`'s `extends` chain a program class, down to a
/// root that `seed_family` gives `Function`: one that extends nothing or a
/// native constructor (`Error` ...), whose own chain the lookup reads as it
/// does for that root alone? A value parent (`parent_value`) ends the chain
/// in values the lock does not see.
fn program_class_chain(m: &Model, mut c: TmplId) -> bool {
    loop {
        let tp = &m.tmpls[c as usize];
        if tp.parent_value.is_some() {
            return false;
        }
        match tp.parent {
            Some(p) if p != c => c = p,
            Some(_) => return false,
            None => return true,
        }
    }
}

/// DEC 250 (Q2): no program write defines named key `name` on
/// `Object.prototype` (`members::locked_read` of the Object family: an add
/// there is refused, a member is locked, `constructor` is exempt).
pub(crate) fn object_final_read(name: &str) -> bool {
    members::locked_read(Family::Object, None, name)
}

/// DEC 250: is a read of key `a` on every value of native family `fam`
/// (`derive`'s NativeMember receivers) final?
pub(crate) fn native_locked(m: &Model, fam: NativeFamily, a: Atom) -> bool {
    let name = m.atoms[a as usize].as_str();
    match fam {
        NativeFamily::Str => members::locked_read(Family::Str, None, name),
        NativeFamily::Array => members::locked_read(Family::Array, None, name),
        NativeFamily::Collection(w) => members::locked_read(Family::Collection, Some(w), name),
        // RegExp.prototype is not in the lock: no member of it is final.
        NativeFamily::RegExp => false,
    }
}

/// The opaque builtin function value; in explain mode, counted by the
/// arm that minted it (`perry: wpfacts explain: builtin-fn minted`).
fn builtin_fn(m: &mut Model, why: &'static str) -> Tok {
    if m.explain {
        *m.bf_mints.entry(why).or_insert(0) += 1;
    }
    m.tok(TokKind::BuiltinFn(Native::Any))
}

/// Resolve the member's object reference before interning its token. The
/// receiver family locates the holder, but is never part of value identity.
fn member_native(m: &Model, fam: Family, what: Option<&'static str>, name: &str) -> Option<Native> {
    let holder = match what {
        Some("TypedArray") => m.declarations.named("%TypedArray%.prototype", m),
        // Every iterator kind resolves its members as `Iterator.prototype`'s.
        Some(w) if members::bobj_family(w) == Family::Iterator => {
            m.declarations.named("Iterator.prototype", m)
        }
        Some(w) => m.declarations.named(&format!("{w}.prototype"), m),
        None => m.declarations.family(fam, m),
    }?;
    let identity = m
        .declarations
        .property(holder, name, m)
        .and_then(|p| p.native)
        .or_else(|| {
            if members::own(fam, name).is_some() {
                return None;
            }
            let object = m.declarations.family(Family::Object, m)?;
            m.declarations.property(object, name, m)?.native
        })?;
    Some(Native::Member { identity })
}

/// Canonical coordinates are resolved with the value's shape, once.
fn prototype_model(
    owner: &'static str,
    key: &'static str,
) -> Option<(Family, Option<&'static str>, &'static str)> {
    let owner = owner.strip_suffix(".prototype")?;
    let (fam, what) = match owner {
        "Object" => (Family::Object, None),
        "BigInt" => (Family::BigInt, None),
        _ => match ns_by_name(owner).and_then(loads::proto_family) {
            Some((fam, what, _)) => (fam, what),
            None => (members::bobj_family(owner), Some(owner)),
        },
    };
    Some((fam, what, key))
}

pub(crate) fn native_model(
    m: &Model,
    t: Tok,
) -> Option<(Family, Option<&'static str>, &'static str)> {
    m.native_coordinates(t)?.prototype
}

/// The three receiver-independent Object.prototype summaries already have
/// namespace tokens. Intern every path to those objects under those tokens.
pub(crate) fn canonical_native(k: TokKind, whole: bool) -> TokKind {
    if let TokKind::BuiltinFn(Native::Member { identity }) = k {
        if whole {
            if let Some(ns) = ns_by_name(identity) {
                if members::ns_chain(ns) == Family::Function {
                    return TokKind::Builtin(ns);
                }
            }
        }
        if let Some(key) = identity.strip_prefix("Object.prototype.") {
            if let Some(own) = ProtoOwn::of_name(key) {
                return TokKind::Builtin(BuiltinNs::ObjectProtoOwn(own));
            }
        }
    }
    k
}

/// Does receiver token `r` have the family of native member `(fam, what)`,
/// so that the member called on it runs `own_method(r, key)`, the model a
/// call `r.key()` runs: a site builtin object of the same kind, a function
/// value (a class object too) for `Function.prototype`'s. `Array.prototype`'s methods are generic
/// over `this` (ECMA-262 23.1.3: ToObject(this), its `length` and indices
/// through [[Get]]/[[Set]], ArraySpeciesCreate): they run on any object,
/// an array or not (`native_prototype_call`).
pub(crate) fn native_receiver(m: &Model, fam: Family, what: Option<&'static str>, r: Tok) -> bool {
    if fam == Family::Object {
        return r != TOP;
    }
    if fam == Family::Array && what.is_none() {
        return matches!(m.toks[r as usize], TokKind::Obj(..));
    }
    match &m.toks[r as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Arr { .. } => fam == Family::Array && what.is_none(),
            TmplKind::BObj { what: w, .. } => what == Some(*w) && members::bobj_family(w) == fam,
            _ => false,
        },
        TokKind::Func(..) | TokKind::Bound(_) | TokKind::BuiltinFn(_) | TokKind::ClassObj(_) => {
            fam == Family::Function
        }
        TokKind::Builtin(_) => fam == Family::Function && builtin_function(m, r),
        _ => false,
    }
}

/// Invoke the prototype member's model, rather than redispatching a
/// callable namespace receiver as a static namespace call.
pub(crate) fn native_prototype_call(m: &mut Model, fam: Family, r: Tok, key: Atom, call: CallId) {
    let prev = m.cur_this.replace(r);
    match fam {
        Family::Function => {
            let name = m.atoms[key as usize].clone();
            function_method(m, r, Some(&name), call);
        }
        Family::Object => {
            let proto = m.tok(TokKind::ObjectProto);
            proto_method(m, proto, r, Key::Named(key), call);
        }
        // The method itself, whatever chain the receiver has.
        Family::Array => {
            let name = m.atoms[key as usize].clone();
            array_method_on(m, r, &name, call);
        }
        _ => own_method_inner(m, r, Key::Named(key), call),
    }
    m.cur_this = prev;
}

/// The primitive kinds whose chain is family `fam`'s (`prim_families`).
pub(crate) fn native_prims(fam: Family) -> u16 {
    match fam {
        Family::Str => prim::STR,
        Family::Num => prim::NUM,
        Family::Bool => prim::BOOL,
        Family::Sym => prim::SYMBOL,
        Family::BigInt => prim::BIGINT,
        _ => 0,
    }
}

/// The opaque member: what a key a store into ⊤ (or unmodeled code) may
/// have written onto a builtin prototype holds: any value (⊤) and any
/// primitive. No builtin function value beside ⊤: ⊤ already stands for any
/// builtin function.
fn opaque_member(m: &mut Model, dst: NodeId) {
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
        if let Some(own) = crate::model::ProtoOwn::of_name(&m.atoms[a as usize]) {
            let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoOwn(own)));
            m.add_tok(dst, f);
            if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            }
            return;
        }
    }
    undescribed_member(m, dst);
}

mod loads;
pub use loads::*;
/// The intrinsic family whose chain `ObjectProto` stands for on the chain
/// of `t`, when `t`'s kind puts it there (`proto_node` seeds it): a
/// function's is `Function.prototype` (with the async/generator function
/// prototypes above it), an array's `Array.prototype`, a site builtin
/// object's its own builtin's, a namespace's its chain's, anything else's
/// `Object.prototype`. `None`: the chain's prototypes are values (`new
/// f()`, `extends <value>`, `Object.create(p)`, whose seed is no prototype
/// it has) or a class's. A builtin prototype given as a value later adds ⊤
/// to the chain (`Cons::ProtoGiven`).
pub(crate) fn is_collection(what: &str) -> bool {
    members::bobj_family(what) == Family::Collection
}

/// A builtin object template kind whose iteration reads indices as an
/// Array's does (`Family::Box`: the arrays and records the runtime fills,
/// `Object.keys` results among them). Every other builtin object iterates
/// its internal entries, or has no `@@iterator`.
pub(crate) fn iterates_indices(what: &str) -> bool {
    members::bobj_family(what) == Family::Box
}

/// Does the realm give the namespace or constructor `ns` no enumerable own
/// property, so that a copy of its own enumerable properties (`{...ns}`,
/// `Object.assign`, `Object.values`) reads only what the program put on it
/// (`Model::own_props_modelled`)? ECMA-262 18: every property the
/// specification gives a built-in object is non-enumerable (its methods,
/// constants, `prototype`, `length`, `name`), and a [[Set]] of such a key
/// keeps it so. V8 adds one enumerable data property, `Error.stackTraceLimit`.
/// Host objects have enumerable own properties: `console`'s methods,
/// `process`'s, the global object's, `require.resolve`/`cache`, the stdio
/// streams', and a node module's exports (`{...require("fs")}` copies them).
pub(crate) fn realm_own_props_unenumerable(ns: BuiltinNs) -> bool {
    match ns {
        BuiltinNs::Object
        | BuiltinNs::Reflect
        | BuiltinNs::Array
        | BuiltinNs::Json
        | BuiltinNs::Math
        | BuiltinNs::Promise
        | BuiltinNs::AggregateError
        | BuiltinNs::Date
        | BuiltinNs::RegExp
        | BuiltinNs::TypedArray
        | BuiltinNs::ArrayBuffer
        | BuiltinNs::Symbol
        | BuiltinNs::Number
        | BuiltinNs::String
        | BuiltinNs::Boolean
        | BuiltinNs::Function
        | BuiltinNs::Map
        | BuiltinNs::Set
        | BuiltinNs::WeakMap
        | BuiltinNs::WeakSet
        | BuiltinNs::ObjectProtoMethod
        | BuiltinNs::ObjectProtoOwn(_) => true,
        BuiltinNs::Error
        | BuiltinNs::Console
        | BuiltinNs::GlobalThis
        | BuiltinNs::Process
        | BuiltinNs::Require
        | BuiltinNs::Stdout
        | BuiltinNs::Stderr
        | BuiltinNs::Stdin
        | BuiltinNs::Module(_)
        | BuiltinNs::Other => false,
    }
}

/// A builtin object whose template is a container the model fills
/// (`Family::Box`: `JSON.parse` values, `Object.keys` and `Reflect.ownKeys`
/// arrays, `Object.groupBy` and `Object.fromEntries` results, property
/// descriptors): its birth stores every value the runtime gives its own
/// enumerable properties under its any key, and none of them is an
/// accessor at birth (one the program defines later runs as on any
/// object: `Model::load_from`). A copy of its own properties reads those fields
/// (`Model::own_props_modelled`). A `primbox` is not: the result of an
/// undescribed primitive, Date or typed-array method (`generic_result`,
/// `prim_method_generic`) is born with an empty any key, its primitives
/// given by the load rule (`loads.rs`, "a container of primitives").
pub(crate) fn own_values_filled(what: &str) -> bool {
    members::bobj_family(what) == Family::Box && what != "primbox"
}

fn seed_family(m: &Model, t: Tok) -> Option<Family> {
    match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Class { .. } | TmplKind::FnInst { .. } | TmplKind::ObjCreate { .. } => None,
            _ if m.virt_protos.contains_key(&t) => None,
            TmplKind::Arr { .. } => Some(Family::Array),
            TmplKind::BObj { what, .. } => Some(members::bobj_family(what)),
            TmplKind::Lit { .. } | TmplKind::Args { .. } => Some(Family::Object),
        },
        TokKind::Proto(tm) => {
            let tm = &m.tmpls[*tm as usize];
            (tm.parent.is_none() && tm.native_parent.is_none() && tm.parent_value.is_none())
                .then_some(Family::Object)
        }
        TokKind::NativeProto(_) => Some(Family::Object),
        TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn(_) => Some(Family::Function),
        TokKind::ClassObj(tm) => {
            let tm = &m.tmpls[*tm as usize];
            (tm.parent.is_none() && tm.parent_value.is_none()).then_some(Family::Function)
        }
        TokKind::Builtin(ns) => Some(members::ns_chain(*ns)),
        _ => None,
    }
}

/// The builtin whose prototype object is the `ObjectProto` the chain of
/// `t` starts with (`proto_node` seeds it; `seed_family`'s kinds): an
/// array's `Array`, a site builtin object's own builtin, a function's
/// `Function`; a typed array prototype's goes on to `%TypedArray%.prototype`,
/// which the one `TypedArray` prototype stands for. `None`: the chain's
/// prototypes are values or a class's, or `t` is no builtin receiver.
/// Is program function `f` born with `%Function.prototype%` as its
/// `[[Prototype]]` (the prototype-kind predicate,
/// `perry_hir::fn_proto::FnProtoKind::Function`)? Not an async, generator
/// or async-generator function (`%AsyncFunction.prototype%` & co.), and not
/// a generator's step closure, which stands for the generator object
/// (`FuncInfo::gen_step`).
pub(crate) fn born_with_function_proto(m: &Model, f: FnIdx) -> bool {
    let fi = &m.funcs[f as usize];
    fi.proto == Some(perry_hir::fn_proto::FnProtoKind::Function) && !fi.gen_step()
}

pub(crate) fn chain_ns(m: &Model, t: Tok) -> Option<BuiltinNs> {
    match &m.toks[t as usize] {
        TokKind::Obj(_, _) if m.virt_protos.contains_key(&t) => None,
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Arr { .. } => Some(BuiltinNs::Array),
            TmplKind::BObj {
                what: "TypedArray", ..
            } => Some(BuiltinNs::TypedArray),
            TmplKind::BObj { what, .. } => ns_by_name(what),
            _ => None,
        },
        TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn(_) => {
            Some(BuiltinNs::Function)
        }
        TokKind::ClassObj(tm) => {
            let tm = &m.tmpls[*tm as usize];
            (tm.parent.is_none() && tm.parent_value.is_none()).then_some(BuiltinNs::Function)
        }
        TokKind::Builtin(ns) if members::ns_chain(*ns) == Family::Function => {
            Some(BuiltinNs::Function)
        }
        TokKind::NativeProto(n) if ns_by_name(n) == Some(BuiltinNs::TypedArray) => {
            Some(BuiltinNs::TypedArray)
        }
        _ => None,
    }
}

/// The token of builtin `ns`'s prototype object taken as a value: the
/// native prototype of its global (`native_proto`, resolved back through
/// `ns_by_name` and `loads::proto_family`), whose reads are that builtin's
/// described members. `None` for `Object.prototype`, which `ObjectProto`
/// is, and for a builtin whose prototype the model does not describe.
pub(crate) fn builtin_proto_tok(m: &mut Model, ns: BuiltinNs) -> Option<Tok> {
    match loads::proto_family(ns)? {
        (Family::Object, _, _) => None,
        (_, _, global) => Some(native_proto(m, global)),
    }
}

/// `dst` ⊇ builtin `ns`'s prototype object as a value (`F.prototype`;
/// `Object.getPrototypeOf(x)` of a builtin receiver `x`, `chain_ns`): the
/// collapsed `ObjectProto`, which every builtin receiver's chain reads (so
/// a write through the value stays a write of it, as a write of any
/// builtin prototype is), and the builtin's own token
/// (`builtin_proto_tok`), so that an object whose chain the value becomes
/// reads the builtin's members and accessors.
pub(crate) fn builtin_proto_value(m: &mut Model, ns: Option<BuiltinNs>, dst: NodeId) {
    let op = m.tok(TokKind::ObjectProto);
    m.add_tok(dst, op);
    if let Some(p) = ns.and_then(|ns| builtin_proto_tok(m, ns)) {
        m.add_tok(dst, p);
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
/// every builtin prototype, as a value (an open gap, as before). Where the
/// chain of `of` is final under any key (`locked_chain_read` of
/// `STAR_KEY`, DEC 250), only `constructor`, which the lock exempts, may
/// have been written there.
fn family_any(m: &mut Model, of: Tok, fam: Family, dst: NodeId) {
    family_ctor(m, fam, dst);
    let (fns, prims) = members::any_member(fam);
    if fns {
        let bf = builtin_fn(m, "computed key on a described chain");
        m.add_tok(dst, bf);
    }
    m.add_prim(dst, prims | prim::UNDEF);
    let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoMethod));
    m.add_tok(dst, f);
    let star = m.atom(crate::STAR_KEY);
    if !locked_chain_read(m, of, star) {
        return any_written(m, dst);
    }
    let ctor = m.atom("constructor");
    if !intrinsic_miss(m, ctor, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// A computed read of a builtin prototype may find any key a store into ⊤
/// wrote, now or later in the solve (`native_written` of `STAR_KEY`).
fn any_written(m: &mut Model, dst: NodeId) {
    let star = m.atom(crate::STAR_KEY);
    if !intrinsic_miss(m, star, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

mod accessor_lookup;
mod methods;
pub(crate) use accessor_lookup::{annexb_lookup, lookup_native_descriptor};
pub use methods::*;
pub(crate) use methods::{accessor_receiver, annexb_define, unknown_accessor_callable};
/// Whether the generated global object declares a callable under this key.
pub(crate) fn global_function(m: &Model, name: &str) -> bool {
    m.namespace_property(BuiltinNs::GlobalThis, name)
        .is_some_and(|p| p.native.is_some())
}

pub(crate) fn namespace_receiver_getter(m: &Model, t: Tok, key: &str) -> bool {
    m.native_property(t, key).is_some_and(|p| p.getter_this)
}

/// A pure native accessor is read with the original receiver, even when
/// an inherited lookup found it on another namespace holder.
fn namespace_getter_load(
    m: &mut Model,
    origin: Tok,
    holder: Tok,
    _ns: BuiltinNs,
    a: Atom,
    dst: NodeId,
) -> bool {
    if m.unit_mode || !namespace_receiver_getter(m, holder, &m.atoms[a as usize]) {
        return false;
    }
    if native_written(m, holder, a) {
        opaque_member(m, dst);
    } else {
        m.native_missing.push((holder, a, NativeMiss::Load(dst)));
        m.add_tok(dst, origin);
    }
    true
}

/// A declared own field of any identified function, observed with the
/// original receiver. The anonymous value uses this same observation via
/// native_may_be; an undescribed namespace adds no fabricated default.
pub(crate) fn native_own_load(
    m: &mut Model,
    origin: Tok,
    holder: Tok,
    key: Key,
    dst: NodeId,
) -> bool {
    if m.unit_mode {
        return false;
    }
    let Key::Named(a) = key else {
        return false;
    };
    let name = m.atoms[a as usize].clone();
    match m.toks[holder as usize].clone() {
        TokKind::Builtin(ns) => {
            if namespace_getter_load(m, origin, holder, ns, a, dst) {
                return true;
            }
            if m.native_property(holder, &name)
                .is_some_and(|p| p.native.is_some())
            {
                loads::ns_load(m, holder, ns, a, dst);
                return true;
            }
        }
        TokKind::BuiltinFn(Native::Member { .. }) => {
            let property = m.native_property(holder, &name);
            let value = property.and_then(|p| p.native);
            if value.is_none()
                && (!property.is_some_and(|p| p.own_namespace)
                    || members::member(Family::Function, &name).is_some())
            {
                return false;
            }
            if native_written(m, holder, a) {
                opaque_member(m, dst);
            } else {
                m.native_missing.push((holder, a, NativeMiss::Load(dst)));
                if let Some(identity) = value {
                    let value = m.tok(TokKind::BuiltinFn(Native::Member { identity }));
                    m.add_tok(dst, value);
                } else {
                    opaque_member(m, dst);
                }
            }
            return true;
        }
        _ => {}
    }
    false
}

/// Constructibility is the realm declaration's object shape, independent
/// of whether this solver has a specialized constructor call model.
pub(crate) fn native_constructible(m: &Model, t: Tok) -> bool {
    m.native_coordinates(t).is_some_and(|c| c.constructible)
}

/// Read the function reference from this receiver's declared property shape.
pub(crate) fn namespace_native(m: &Model, t: Tok, ns: BuiltinNs, name: &str) -> Option<Native> {
    if let Some(identity) = m.native_property(t, name).and_then(|p| p.native) {
        return Some(Native::Member { identity });
    }
    matches!(members::ns_member(ns, name), Some(Some(Mem::Fn)))
        .then(|| member_native(m, members::ns_chain(ns), None, name))
        .flatten()
}

/// The function value carries its canonical namespace call coordinates.
/// It runs on the receiver token `this_tok` the call is made for (a method
/// call binds one per receiver value), else on the call's own `this`: what
/// the namespace's model does to its receiver is decided per token, as for
/// a call by name on that token (`own_method`). A bare `fetch(u)` is made
/// on the unresolved-global sentinel, the namespace's own object, which
/// the native gives to no one (`unknown_method_proto`'s `own`).
pub(crate) fn native_namespace_call(
    m: &mut Model,
    t: Tok,
    call: CallId,
    this_tok: Option<Tok>,
) -> bool {
    if m.unit_mode {
        return false;
    }
    let Some((ns, name)) = m.native_coordinates(t).and_then(|c| c.namespace) else {
        return false;
    };
    if m.bind_done
        .insert((call, t, this_tok.unwrap_or(u32::MAX - 6)))
    {
        m.diagnostic_callee(call, t);
        let prev = std::mem::replace(&mut m.cur_this, this_tok);
        m.with_b(|m| ns_method_inner(m, ns, Some(name), call, true));
        m.cur_this = prev;
    }
    true
}

/// The host object a member access on a token of `ns` under `name` reads:
/// `console` and the stdio streams themselves, and `console` for a call of
/// one of its methods on the global sentinel (`console.log(x)` lowers to a
/// call on `GlobalGet(0)`).
pub(crate) fn host_of(ns: BuiltinNs, name: &str, call: bool) -> Option<BuiltinNs> {
    match ns {
        BuiltinNs::Other
            if call
                && matches!(
                    members::ns_member(BuiltinNs::Console, name),
                    Some(Some(Mem::Fn))
                ) =>
        {
            Some(BuiltinNs::Console)
        }
        ns if members::is_host(ns) => Some(ns),
        _ => None,
    }
}

/// DECISIONS 255 store funnel on a host object: may the program (or code
/// holding the object as ⊤) have stored into host `h` under key `a`? Until
/// then a described member is the runtime's own. The solver's host calls
/// and loads and `derive`'s site verdict read this one predicate.
pub(crate) fn host_written(m: &Model, h: BuiltinNs, a: Atom) -> bool {
    let t = m
        .tok_index
        .get(&TokKind::Builtin(h))
        .copied()
        .unwrap_or(TOP);
    native_written(m, t, a)
}

/// The site verdict's half of [`host_written`]: member `a` of host `h` is
/// described as a function (any access) or a data member (a read), and
/// unwritten.
pub(crate) fn host_native(m: &Model, h: BuiltinNs, a: Atom, call: bool) -> bool {
    let described = match members::ns_member(h, &m.atoms[a as usize]) {
        Some(Some(Mem::Fn)) => true,
        Some(Some(Mem::Prim(_))) => !call,
        _ => false,
    };
    described && !host_written(m, h, a)
}

/// A call of member `n` of host `h`: the runtime's own method (`native`)
/// until a store reaches `gate` (the member's key, or `STAR_KEY` for a method
/// that reads other members of its receiver), then an unknown method of the
/// host (`this`). A write later in the solve replays it (`NativeMiss::Call`).
fn host_call(
    m: &mut Model,
    h: BuiltinNs,
    n: &str,
    gate: &str,
    call: CallId,
    native: impl FnOnce(&mut Model),
) {
    let t = m.tok(TokKind::Builtin(h));
    let a = m.atom(gate);
    if host_written(m, h, a) {
        // Census: the call hangs off the write that opened the gate, as a
        // replay does.
        m.census_native_read(t, a, NativeMiss::Call(t, call));
        let prev = m.cur_this.replace(t);
        unknown_method(
            m,
            call,
            &format!("argument of written host member {h:?}.{n}"),
        );
        m.cur_this = prev;
    } else {
        m.native_missing.push((t, a, NativeMiss::Call(t, call)));
        native(m);
    }
}

/// A host stream's own `write(chunk, encoding?, cb?)` (node's Writable,
/// perry's `process_stdout_write_stub`): it returns a boolean and hands its
/// receiver to no one. It keeps a binary chunk and the callback in the
/// stream's state, which the program can read back (`writableBuffer`,
/// `_writableState`), so the arguments reach unknown code. It reads other
/// members of its receiver (`_write`, `_writableState`, `emit`, ...): a store
/// under any key of the stream ends this (`host_call`'s `STAR_KEY` gate).
fn stream_write(m: &mut Model, h: BuiltinNs, call: CallId) {
    let c = m.calls[call as usize].clone();
    let r = m.reason(&format!("argument of host {h:?}.write (kept by the stream)"));
    for &a in c.args.iter().chain(c.spread.iter()) {
        m.sink_node(a, r);
    }
    ret_prim(m, call, prim::BOOL);
}

/// A console method formats its arguments (`Protocol::Inspect`); a spread
/// hands it the elements, which the call site iterated.
fn console_args(m: &mut Model, call: CallId) {
    let c = m.calls[call as usize].clone();
    let r = m.reason("console argument (formatted by the runtime)");
    let ek = m.elem_key();
    let mut shown = c.args.clone();
    for &s in &c.spread {
        let dst = m.node_l("console-spread-elems");
        m.cons(s, Cons::Load { key: ek, dst });
        shown.push(dst);
    }
    for a in shown {
        m.cons(
            a,
            Cons::Implicit {
                reason: r,
                proto: Protocol::Inspect,
            },
        );
    }
    ret_prim(m, call, prim::UNDEF);
}

fn ns_method(m: &mut Model, ns: BuiltinNs, name: Option<&str>, call: CallId) {
    ns_method_inner(m, ns, name, call, false)
}

fn ns_method_inner(
    m: &mut Model,
    ns: BuiltinNs,
    name: Option<&str>,
    call: CallId,
    identified: bool,
) {
    if let Some(name) = name {
        if matches!(members::ns_member(ns, name), Some(Some(Mem::Fn)))
            || ns == BuiltinNs::Other && (identified || global_function(m, name))
        {
            m.diagnostic_builtin(call, |_| format!("{ns:?}.{name}"));
        }
    }

    // A computed key may name any member, one no rule describes included
    // (the last arm); on the global object any host global (an unknown
    // function) and `Function` (E14), as a read under it holds them.
    let Some(name) = name else {
        if ns == BuiltinNs::GlobalThis {
            call_builtin_ns(m, BuiltinNs::Function, call);
        }
        return unknown_method_proto(
            m,
            call,
            &format!("argument of builtin {ns:?}[computed]"),
            None,
            true,
        );
    };
    match (ns, name) {
        (BuiltinNs::Other | BuiltinNs::Console, n)
            if host_of(ns, n, true) == Some(BuiltinNs::Console) =>
        {
            host_call(m, BuiltinNs::Console, n, n, call, |m| console_args(m, call))
        }
        // A stream method: the runtime's own, which hands its receiver to no
        // one; a listener it registers is called by the runtime (escaped).
        (BuiltinNs::Stdout | BuiltinNs::Stderr | BuiltinNs::Stdin, "write") => {
            host_call(m, ns, "write", crate::STAR_KEY, call, |m| stream_write(m, ns, call))
        }
        (BuiltinNs::Stdout | BuiltinNs::Stderr | BuiltinNs::Stdin, n) => {
            host_call(m, ns, n, n, call, |m| {
                unknown_method_proto(m, call, &format!("argument of host {ns:?}.{n}"), None, true)
            })
        }
        (BuiltinNs::Other, "setTimeout" | "setInterval" | "setImmediate" | "queueMicrotask") => {
            let c = m.calls[call as usize].clone();
            if !c.spread.is_empty() {
                return unknown_method(m, call, "spread timer arguments");
            }
            // HTML timers: ToLong(timeout); callback receives precisely
            // extra arguments, with the global this. Node uses the Timeout
            // as this, so retain that host possibility beside the global.
            let timer = site_tmpl(m, call, "Timer");
            let this = m.node_l("timer-this");
            m.add_tok(this, timer);
            let global = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            m.add_tok(this, global);
            let timed = matches!(name, "setTimeout" | "setInterval");
            if timed {
                if let Some(&delay) = c.args.get(1) {
                    string_methods::to_prim(m, delay, "timer timeout ToNumber");
                }
            }
            if let Some(&cb) = c.args.first() {
                let args = if name == "queueMicrotask" {
                    vec![]
                } else {
                    c.args
                        .iter()
                        .skip(if timed { 2 } else { 1 })
                        .copied()
                        .collect()
                };
                let this_arg = if name == "queueMicrotask" {
                    ThisArg::Undef
                } else {
                    ThisArg::Node(this)
                };
                let _ = collections::cb_call(m, call, cb, this_arg, args);
                // HTML also accepts string handlers (ToString), which may
                // run arbitrary script. Perry/Node only accept functions;
                // no object handler is retained in either case.
            }
            if name == "queueMicrotask" {
                ret_prim(m, call, prim::UNDEF);
            } else {
                ret_tok(m, call, timer);
                ret_prim(m, call, prim::NUM);
            }
        }
        (BuiltinNs::Other, "clearTimeout" | "clearInterval" | "clearImmediate") => {
            // HTML's long id conversion; Node's Timeout handle also has a
            // primitive id. Neither calls/retains surplus operands.
            if let Some(id) = arg(m, call, 0) {
                string_methods::to_prim(m, id, "clear timer id ToNumber");
            }
            ret_prim(m, call, prim::UNDEF);
        }
        // A call of the realm's `eval` function under any spelling that is
        // not a direct eval (`(0, eval)(s)`, `const e = eval; e(s)`,
        // `globalThis.eval(s)`, `eval?.(s)`) is an indirect eval: with the
        // interpreter linked it runs its string argument as global code
        // (`global_this_eval_thunk`), which reads and calls what the program
        // stored on the global object (DECISIONS 150/235), as a call of the
        // Function constructor does. Without the interpreter it runs no
        // program code.
        (BuiltinNs::Other, "eval") if m.interp_linked => {
            set_dyn_eval(m, "E14 indirect eval call");
            unknown_method(m, call, "E14 dynamic code");
        }
        // The runtime's `gc([force])`: the argument is ignored.
        (BuiltinNs::Other, "gc") => ret_prim(m, call, prim::UNDEF),
        // A method of the global object named by one of the runtime's global
        // functions is that function, which ignores `this`: the global object
        // is not handed to unknown code by the call (`global.gc()`). A key the
        // program may write onto the global object (now or later in the
        // solve) is the opaque call as well.
        (BuiltinNs::GlobalThis, n) if global_function(m, n) => {
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
            // Own enumerable values only (EnumerableOwnProperties): no
            // inherited member is among them.
            if let Some(o) = arg(m, call, 0) {
                m.cons(o, Cons::OwnValues { dst: fa });
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
            if name == "of" {
                m.edge(vals, fa);
            } else {
                // ECMA-262 23.1.2.1: an iterable is iterated; an array-like
                // (no `@@iterator`) is read at its indices below `length`.
                let it = iterate(m, call, vals);
                m.edge(it, fa);
                let ek = m.elem_key();
                m.cons(vals, Cons::Load { key: ek, dst: fa });
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
                        region: m.thrown,
                    },
                );
                m.cons(f, Cons::Call { call: call2 });
                m.edge(ret, fa);
            }
            ret_tok(m, call, arr);
            // IsConstructor(C) (23.1.2.1 step 4, 23.1.2.3 step 4): the
            // result is Construct(C) of the `this` value, with «len» (an
            // array-like source, `of`) or no argument (an iterable).
            let c = m.calls[call as usize].clone();
            let this = match c.this {
                ThisArg::Node(n) => Some(n),
                ThisArg::Tok(t) => {
                    let n = m.node_l("from-this");
                    m.add_tok(n, t);
                    Some(n)
                }
                ThisArg::Undef => None,
            };
            if let Some(this) = this {
                let len = m.node_l("from-length");
                m.add_prim(len, prim::NUM | prim::UNDEF);
                let made = m.node_l("from-constructed");
                let mc = m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args: vec![len],
                        spread: vec![],
                        ret: made,
                        site: c.site,
                        region: m.thrown,
                    },
                );
                construct_into(m, this, mc, arr, made, c.ret);
            }
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
            // Its objects are ordinary objects, its arrays arrays
            // (`members::box_chain`), as `Gen::json_value` models them.
            let j = site_tmpl(m, call, "json");
            let ja = site_tmpl(m, call, "Array");
            let fa = m.field_any(j);
            for f in [fa, m.field_any(ja)] {
                m.add_prim(f, prim::ALL);
                m.add_tok(f, j);
                m.add_tok(f, ja);
            }
            ret_tok(m, call, j);
            ret_tok(m, call, ja);
            ret_prim(m, call, prim::ALL);
            if let Some(rv) = arg(m, call, 1) {
                // reviver(key, value) with `this` = the holder, an object
                // or an array
                let ret = m.calls[call as usize].ret;
                let s = m.node_l("str");
                m.add_prim(s, prim::STR);
                for holder in [j, ja] {
                    let call2 = m.calls.len() as CallId;
                    m.solver_call(
                        call,
                        CallSite {
                            this: ThisArg::Tok(holder),
                            args: vec![s, fa],
                            spread: vec![],
                            ret,
                            site: call2,
                            region: m.thrown,
                        },
                    );
                    m.cons(rv, Cons::Call { call: call2 });
                }
                m.edge(ret, fa);
                let fja = m.field_any(ja);
                m.edge(ret, fja);
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
            // ECMA-262 27.2.4.1 ff.: the iterable is iterated
            // (GetIterator); its values are the items.
            let vals = all_arg_values(m, call);
            let items = iterate(m, call, vals);
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
            if name == "any" {
                // Every item rejected: an AggregateError whose `errors` are
                // the reasons (rejections are what `thrown` holds).
                let e = error_tok(m, call);
                let errs = site_tmpl(m, call, "Array");
                let efa = m.field_any(errs);
                let thrown = m.thrown;
                m.edge(thrown, efa);
                let ea = m.atom("errors");
                let f = m.field(e, ea);
                m.add_tok(f, errs);
                m.add_tok(thrown, e);
            }
            ret_tok(m, call, p);
        }
        // An Array of the target's own keys.
        (BuiltinNs::Reflect, "ownKeys") => {
            implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
            let pb = site_tmpl(m, call, "arraybox");
            let fa = m.field_any(pb);
            m.add_prim(fa, prim::STR | prim::SYMBOL);
            ret_tok(m, call, pb);
        }
        (BuiltinNs::Reflect, "has") => {
            implicit_args(m, call, "ToPropertyKey", Protocol::ToPrimitive);
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
        // `Function.call(t, ...)` is the constructor's call on the rest.
        (BuiltinNs::Function, "call") if !function_ctor_runs_code(m, call, 1, false) => {}
        (BuiltinNs::Function, "call" | "apply" | "bind") => {
            // `Function.call(...)`: the Function constructor runs (E14).
            set_dyn_eval(m, "E14 Function(...) value call");
            unknown_method(m, call, "E14 dynamic code");
        }
        (BuiltinNs::Function, _) => {
            // Not a method of the Function constructor (TypeError).
        }
        // A key the namespace's description (its own keys and its chain)
        // declares absent is no method (TypeError); one the program added
        // is the receiver's own field or chain value (`method_on`).
        (_, n) if members::ns_member(ns, n) == Some(None) => {}
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
            // The path registry may run a lazy module body inside the call.
            let (lazy, region) = (m.lazy_raise, m.calls[call as usize].region);
            m.edge(lazy, region);
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
            if !function_ctor_runs_code(m, call, 0, false) {
                return;
            }
            set_dyn_eval(m, "E14 Function(...) call");
            unknown_method(m, call, "E14 dynamic code");
        }
        BuiltinNs::ObjectProtoMethod => object_proto_method_call(m, call),
        BuiltinNs::ObjectProtoOwn(own) => object_proto_own_call(m, own, call),
        BuiltinNs::Error | BuiltinNs::AggregateError | BuiltinNs::RegExp => {
            new_builtin_ns(m, ns, call);
            if ns == BuiltinNs::RegExp {
                // `RegExp(re)` can return the existing regexp object, after
                // comparing `re.constructor` with the active function.
                if let Some(src) = arg(m, call, 0) {
                    string_methods::read_only(m, src, "constructor");
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
