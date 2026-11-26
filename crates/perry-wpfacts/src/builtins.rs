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
pub(crate) use effects::is_internal_key;
pub(crate) use effects::strkey_set;
mod gen_members;
#[cfg(test)]
pub(crate) mod effects_for_tests {
    pub(crate) use super::effects::interpretable;
}
pub use constructors::{construct_named, new_builtin_ns};
pub(crate) mod members;
pub(crate) use members::realm_parent_constructor;
mod native_protocol;
mod realm_declared;
pub(crate) mod realm_shapes;
mod undeclared;
pub use native_protocol::sentinel_user_global;
pub(crate) use native_protocol::{
    array_methods_writes, census_native_op_writes, census_native_written, typed_metadata_writes,
};
pub use undeclared::{
    builtin_function, declared_data_global, global_object, global_self_key, global_store_escapes,
    native_may_be, plain_namespace_own_store, realm_reads_store, undeclared,
};
pub(crate) mod regex_tail;
pub(crate) mod string_methods;
pub(crate) mod typed_array_methods;
pub(crate) use collections::construct_into;
pub(crate) use effects::{
    construct_layout, declared_construct, declared_member, declared_proto_method,
    declared_receiver_method, declared_static, declared_super_construct, run_construct,
    run_function, run_member, run_native, run_super_construct,
};

use collections::{array_method, array_method_on, bobj_method, function_method, iterate};
pub(crate) use collections::{WK_ASYNC_ITERATOR, WK_ITERATOR, WK_PREFIX};
/// The template kind of an array iterator (`%ArrayIteratorPrototype%`).
pub(crate) const ARRAY_ITERATOR_KIND: &str = members::ARRAY_ITERATOR;
pub use members::Family;
use members::Mem;
pub use native_protocol::{
    global_any_field, global_any_names, global_any_new, global_def_field, global_field,
    global_user_key, global_user_method, global_value_written, is_builtin_module,
    native_accessor_known, native_accessor_written, native_nonplain_written, native_parent_ctor,
    native_parent_keys, native_parent_summarized, native_parent_value, native_proto,
    native_writers_why, native_written, native_written_by, note_foreign_holds, note_global_in_top,
    note_global_holder, note_handed_to_unknown, note_native_accessor, note_native_data_write, note_native_write,
    note_realm_held, program_global_fn, require_parts, runtime_global_call, settle_native_given,
    GlobalAnyRead, WriteValue,
};
use native_protocol::{native_method, native_skips, opaque_written};

/// The NativeError constructors (ECMA-262 20.5.5.1-6), each its own
/// namespace ([`BuiltinNs::NativeError`]).
pub(crate) const NATIVE_ERRORS: [&str; 6] = [
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
];

/// The concrete typed-array constructors (ECMA-262 23.2.7 Table 71, with
/// `Float16Array`), each its own namespace ([`BuiltinNs::TypedArrayKind`]).
pub(crate) const TYPED_ARRAYS: [&str; 12] = [
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

/// The builtin constructor that is `ns`'s [[Prototype]] (`Error` for
/// `TypeError`), as its statics describe it.
pub(crate) fn ns_parent(ns: BuiltinNs) -> Option<BuiltinNs> {
    match members::ns_statics(ns).prototype {
        members::NsProto::Constructor(parent) => Some(parent),
        _ => None,
    }
}

/// Global names the `GlobalGet(0)` sentinel resolves to.
pub(crate) fn ns_by_name(name: &str) -> Option<BuiltinNs> {
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
        "Error" => BuiltinNs::Error,
        _ if NATIVE_ERRORS.contains(&name) => {
            BuiltinNs::NativeError(NATIVE_ERRORS.into_iter().find(|n| *n == name)?)
        }
        "AggregateError" => BuiltinNs::AggregateError,
        "Date" => BuiltinNs::Date,
        "RegExp" => BuiltinNs::RegExp,
        "%TypedArray%" => BuiltinNs::TypedArray,
        _ if TYPED_ARRAYS.contains(&name) => {
            BuiltinNs::TypedArrayKind(TYPED_ARRAYS.into_iter().find(|n| *n == name)?)
        }
        "DataView" => BuiltinNs::DataView,
        "ArrayBuffer" => BuiltinNs::ArrayBuffer,
        "SharedArrayBuffer" => BuiltinNs::SharedArrayBuffer,
        "Symbol" => BuiltinNs::Symbol,
        "Number" => BuiltinNs::Number,
        "BigInt" => BuiltinNs::BigInt,
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

/// The host object the realm declares as `name`: a realm namespace, a node
/// module object (`node:fs`) or a global host object (`Intl`,
/// `performance`). Its token is `BuiltinNs::Module(name)`; its keys read
/// what its declaration (and its chain's, flattened into the realm rows)
/// names. An object a declared object holds (`process.stdin`) is not a
/// global name: only a node module or an undotted global host object is.
fn node_module(name: &str) -> Option<BuiltinNs> {
    if !name.starts_with("node:") && name.contains('.') {
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

/// The builtin module object `require(name)` returns: the global `process`
/// for `process` (node: `require("process") === globalThis.process`), else
/// the namespace the realm declares (`fs` and `node:fs` alike).
pub(crate) fn builtin_module_tok(m: &mut Model, name: &str) -> Option<Tok> {
    let bare = name.strip_prefix("node:").unwrap_or(name);
    let ns = if bare == "process" {
        BuiltinNs::Process
    } else {
        node_module_ns(bare)?
    };
    Some(m.tok(TokKind::Builtin(ns)))
}

/// Every name `require` resolves to a builtin module object: `process` and
/// the namespaces the realm declares.
fn builtin_module_names() -> impl Iterator<Item = &'static str> {
    std::iter::once("process").chain(
        crate::native_effects::realm()
            .iter()
            .filter(|e| e.kind == perry_hir::realm_manifest::RealmKind::Namespace)
            .map(|e| e.name),
    )
}

/// `spec` names the builtin `name` (`fs`, `node:fs`).
fn same_builtin(name: &str, spec: &str) -> bool {
    name.strip_prefix("node:").unwrap_or(name) == spec.strip_prefix("node:").unwrap_or(spec)
}

/// Does the realm declare `key` as an own member of the `node:vm` module
/// object (`Script`, `runInNewContext`, ...)?
fn vm_member(m: &Model, key: &str) -> bool {
    m.declarations
        .named("node:vm", m)
        .and_then(|h| m.declarations.property(h, key, m))
        .is_some()
}

/// E14: interpreted code may run; it can patch `Object.prototype`.
pub fn set_dyn_eval(m: &mut Model, why: &str) {
    interpreted_code_runs(m);
    if m.dyn_eval.is_none() {
        m.dyn_eval = Some(why.to_string());
        // Interpreted code may call async_hooks.createHook itself.
        async_hook_may_run(m, why);
        m.rel(|| format!("dyneval {}", why.replace(' ', "\\s")));
        if m.explain {
            eprintln!("perry: wpfacts explain: dynamic code reachable: {why}");
        }
        // The global object: `interpreted_code_runs` (code runs only with
        // a linked interpreter; without one the Function constructor
        // throws).
    }
}

/// An async_hooks hook may be enabled: a reached call may be the native
/// `createHook` (its token is a callee there), or interpreted code may run
/// (DECISIONS 150). Each value declared `async_resource`, made before or
/// after in the solve, goes to the hook's `init`.
pub fn async_hook_may_run(m: &mut Model, why: &str) {
    if m.async_hooks.is_some() {
        return;
    }
    m.async_hooks = Some(why.to_string());
    m.rel(|| format!("asynchook {}", why.replace(' ', "\\s")));
    for (n, name) in std::mem::take(&mut m.async_resources) {
        async_resource(m, n, name);
    }
}

/// `async_resource(slot)` of native `name`: the runtime gives the slot's
/// values to `init` of every enabled hook (unknown code), so they escape
/// while a hook may be enabled; until then they wait for one.
pub fn async_resource(m: &mut Model, n: NodeId, name: &'static str) {
    let Some(why) = m.async_hooks.clone() else {
        m.async_resources.push((n, name));
        return;
    };
    let r = m.reason(&format!(
        "{name} hands its resource to async_hooks init ({why})"
    ));
    m.sink_node(n, r);
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
    // It holds the global object (`Function('return this')()`, `this` of
    // indirect eval) and gives it to compiled code as ⊤: a store into ⊤ may
    // write any global and any built-in object (`global_in_top`).
    note_global_in_top(m);
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
    for &a in c.args_from(skip).iter() {
        m.cons(
            a,
            Cons::Implicit {
                reason: r,
                proto: Protocol::ToPrimitive,
                region: m.thrown,
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
/// the runtime's own object (`unknown_method_proto`'s `own`), a
/// primitive (nothing escapes), or not read at all: a static whose steps
/// never read the this value (`Object.create`), called as a value with any
/// receiver (`native_namespace_call`), hands it to no one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recv {
    Given,
    Own,
    Primitive,
    Ignored,
}

fn unknown_method_at(m: &mut Model, call: CallId, why: &str, proto: Option<usize>, recv: Recv) {
    // −0 tolerance: unknown code receives the arguments.
    m.zs_call_args(call);
    if !m.realm_held && m.prov.is_some() {
        m.why_hold("method", call, None);
    }
    if crate::unksrc::on() {
        let f = crate::unksrc::call_fn(m, call);
        crate::unksrc::note("method", why, call, f);
        if !m.realm_held {
            crate::unksrc::held("method", why, call, f);
        }
    }
    // Unknown code runs, handed values or not: it holds the realm where code
    // beside the program may name it (`Model::realm_nameable`).
    if !m.realm_held && m.realm_nameable() {
        let r = m.reason("an intrinsic of the realm: unknown code holds the realm");
        if m.keysnap.is_some() {
            m.keysnap_at(
                true,
                &format!("unknown method (unknown_method_at: {why})"),
                Some(call),
                r,
                None,
            );
        }
        if m.restart_on_hold && !m.hold_restart {
            m.hold_why = Some(why.to_string());
        }
        m.hold_realm(r);
    }
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
        _ if matches!(recv, Recv::Primitive | Recv::Ignored) => {}
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
        m.why_implicit(a, r, call);
        m.cons(
            a,
            Cons::Implicit {
                reason: r,
                proto,
                region: m.thrown,
            },
        );
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
    m.calls[call as usize].arg(i).copied()
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

/// RTREAD14: the event the runtime calls a handler it reads by global name
/// with (`Gen::runtime_global_handlers`), born at that call when a value
/// first reaches the handler's field: a MessageEvent (perry-runtime
/// event_target/message_event.rs `delivery_with_ports`, [[Prototype]]
/// %MessageEvent.prototype%, no own property) whose [[data]] is the
/// structured clone of what the parent posted (perry-stdlib
/// channel_pump.rs `deserialize_message`; thread/clone_write.rs Message
/// mode): fresh ordinary objects and arrays, Map, Set, Date, RegExp, Error,
/// ArrayBuffer and typed arrays, each holding the others and any primitive;
/// never a program object, function or prototype. The same model as the
/// main side's Worker message (`Gen::worker_host`).
pub(crate) fn runtime_global_event(m: &mut Model, call: CallId) {
    let Some(ev) = m.runtime_event else {
        return;
    };
    m.runtime_event_born = true;
    let mut tree = Vec::new();
    for what in [
        "json",
        "Array",
        "Map",
        "Set",
        "Date",
        "RegExp",
        "Error",
        "ArrayBuffer",
        "TypedArray",
    ] {
        tree.push(site_tmpl(m, call, what));
    }
    for &o in &tree {
        let fa = m.field_any(o);
        m.add_prim(fa, prim::ALL);
        for &x in &tree {
            m.add_tok(fa, x);
        }
    }
    let mev = site_tmpl(m, call, "MessageEvent.prototype");
    let da = worker_slot_atom(m, "data");
    let data = m.field_r(mev, da);
    m.add_prim(data, prim::ALL);
    for &x in &tree {
        m.add_tok(data, x);
    }
    m.add_tok(ev, mev);
}

/// A per-call-site template (solve-time births: promises, arrays …).
fn site_tmpl(m: &mut Model, call: CallId, what: &'static str) -> Tok {
    site_tmpl_in(m, call, what, None)
}

/// `site_tmpl` for the object `call` returns, and nothing else holds
/// first, whose contents come from the call operands `need`: born in
/// `Model::birth_ctx_result`'s context (rule SINKBIRTH).
pub(crate) fn site_tmpl_result(m: &mut Model, call: CallId, what: &'static str, need: u32) -> Tok {
    site_tmpl_in(m, call, what, Some(need))
}

fn site_tmpl_in(m: &mut Model, call: CallId, what: &'static str, result: Option<u32>) -> Tok {
    let born = |m: &mut Model, what: &'static str| {
        let site = m.calls[call as usize].site;
        let tm = site_template(m, site, what, false, false);
        let ctx = match result {
            Some(need) => m.birth_ctx_result(call, need),
            None => m.birth_ctx(call),
        };
        m.site_tok(tm, ctx)
    };
    let t = born(m, what);
    // A view born without a buffer argument allocates its own
    // (23.2.5.1.1 AllocateTypedArray, 23.2.5.1.6 AllocateTypedArrayBuffer):
    // one ArrayBuffer per birth site.
    if matches!(what, "TypedArray" | "Buffer" | "DataView") {
        let ab = born(m, "ArrayBuffer");
        let f = viewed_buffer(m, t);
        m.add_tok(f, ab);
    }
    t
}

/// Rule PXDEFINE: the descriptor object a `defineProperty` trap call
/// `call` is passed (10.5.6 step 6, 6.2.6.4 FromPropertyDescriptor of the
/// data descriptor of an ordinary [[Set]], 10.1.9.2 steps 2.d / 2.e): a new
/// ordinary object, one per trap call, with `value` ⊇ `value`, the Boolean
/// `writable`, `enumerable`, `configurable` (step 2.e; 2.d passes `value`
/// alone) and no `get` / `set`.
pub(crate) fn trap_descriptor(m: &mut Model, call: CallId, value: NodeId) -> Tok {
    let tm = site_template(m, call, "descriptor", false, false);
    let ctx = m.birth_ctx(call);
    let t = m.site_tok(tm, ctx);
    let va = m.atom("value");
    let fv = m.field(t, va);
    m.edge(value, fv);
    for name in ["writable", "enumerable", "configurable"] {
        let a = m.atom(name);
        let f = m.field(t, a);
        m.add_prim(f, prim::BOOL | prim::UNDEF);
    }
    for name in ["get", "set"] {
        let a = m.atom(name);
        let f = m.field(t, a);
        m.add_prim(f, prim::UNDEF);
    }
    t
}

/// Rule PXAPPLY: the argument list Array a Proxy trap is passed
/// (CreateArrayFromList, 7.3.16), born at `call`.
pub(crate) fn trap_args_array(m: &mut Model, call: CallId) -> Tok {
    site_tmpl_born(m, call, "Array", false, false)
}

/// The view of `what` born at `call` whose [[ViewedArrayBuffer]] its
/// birth writes itself (`Cons::ViewedBuffer`).
fn site_view(m: &mut Model, call: CallId, what: &'static str) -> Tok {
    site_tmpl_born(m, call, what, false, false)
}

/// The view `t`'s [[ViewedArrayBuffer]] (`VIEWED_BUFFER`): the ArrayBuffer
/// its birth made or was given, which `.buffer` returns (23.2.3.2,
/// 25.3.4.1), never the view itself.
pub(crate) fn viewed_buffer(m: &mut Model, t: Tok) -> NodeId {
    let a = m.atom(VIEWED_BUFFER);
    m.field(t, a)
}

/// The ArrayBuffer of Node's Buffer pool (lib/buffer.js `allocPool`):
/// `Buffer.from(string | array | buffer)`, `Buffer.allocUnsafe`,
/// `Buffer.concat` and `fs.readFileSync` may all view the same pool, so
/// one token stands for it and for the buffer a larger such birth makes.
pub(crate) fn buffer_pool(m: &mut Model) -> Tok {
    let tm = site_template(m, CallId::MAX, "ArrayBuffer", false, false);
    m.tok(TokKind::Obj(tm, NO_CTX))
}

/// A per-call-site Array born without holes: the builtin that makes it
/// writes every index below its length, or exactly the indices of a source
/// whose holes it carries over (`collections::array_method`). Later stores
/// mark their own holes as on any array.
fn site_dense_arr(m: &mut Model, call: CallId) -> Tok {
    site_tmpl_born(m, call, "DenseArray", true, false)
}

/// The Array `Array.prototype.slice` bears from an Array receiver, with
/// `Model::array_reps` on a plain one (`TmplKind::Arr::plain`): the default
/// species arm of `js_array_slice` (`array::species::
/// array_species_create`) allocates a fresh `GC_TYPE_ARRAY` with its elements
/// at the front (`js_array_alloc_with_length`), never a lazy header, never
/// frozen, and copies the receiver's slots (its holes with them). A program
/// species constructor's result is another value of the call
/// (`species_from`), with its own token; a realm constructor other than
/// `%Array%` makes this token's value itself, so `bindings::arr_rep` admits
/// the token only while `derive::exact_array` holds. No birth length is
/// recorded: the rep's floor is 0. `PERRY_FACTS_SLICE_BIRTHS=0` keeps the
/// dense, not plain, birth.
fn site_slice_arr(m: &mut Model, call: CallId) -> Tok {
    let reps = m.array_reps;
    site_tmpl_born(m, call, "DenseArray", true, reps)
}

/// `PERRY_FACTS_SLICE_BIRTHS=0` (`off`, `false`): a `slice` result is no
/// plain Array (`site_slice_arr`).
pub(crate) fn slice_births_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_SLICE_BIRTHS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The Array `new Array(...)` or `Array(...)` births (`BuiltinNs::Array`):
/// `js_array_constructor*` allocates a fresh `GC_TYPE_ARRAY` with its
/// elements at the front, of at least its recorded birth length
/// (`solve::arr_len`), so with `Model::array_reps` on it is plain
/// (`TmplKind::Arr::plain`). The receiver of a fused fill
/// (`SiteBirth::hole_free`, `Gen::call_with`) is born hole-free.
fn site_new_array(m: &mut Model, call: CallId) -> Tok {
    let site = m.calls[call as usize].site;
    let birth = m.arr_len_at_site.get(&site).copied().unwrap_or_default();
    let reps = m.array_reps;
    let hole_free = reps && (birth.hole_free || m.arr_dense_sabotage);
    site_tmpl_born(m, call, "Array", hole_free, reps)
}

/// The template of `what` born at `call`'s source site. The key holds
/// `plain` (REVIEW-p4l1 R1): a call whose callee holds two builtins
/// (`const f = c ? Array : JSON.parse; f(s)`) births one template per
/// kind, so a lazy JSON array never shares the plain `Array` birth's
/// template, and only a plain birth ever makes or finds a plain template.
fn site_tmpl_born(
    m: &mut Model,
    call: CallId,
    what: &'static str,
    hole_free: bool,
    plain: bool,
) -> Tok {
    // Keyed by the SOURCE call site: calls derived while solving (bindings
    // through relays, protocol touches) share their origin's template.
    let site = m.calls[call as usize].site;
    let tm = site_template(m, site, what, hole_free, plain);
    let ctx = m.birth_ctx(call);
    m.site_tok(tm, ctx)
}

/// The template of `what` born at source site `site` (`site_tmpl_born`).
fn site_template(
    m: &mut Model,
    site: CallId,
    what: &'static str,
    hole_free: bool,
    plain: bool,
) -> TmplId {
    let key = (site, what, plain);
    if let Some(&t) = m.site_tmpls.get(&key) {
        return m.tmpl_of(t).expect("site template");
    }
    let tm = m.tmpls.len() as TmplId;
    let kind = if what == "Array" || hole_free {
        TmplKind::Arr {
            module: u32::MAX,
            site,
            hole_free,
            plain,
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
        inert_definite: Vec::new(),
        inert_reads: Vec::new(),
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
    // The birth length is the `new Array(k)` birth's: with array reps on,
    // only the plain template's (`site_new_array`).
    if what == "Array" && plain == m.array_reps {
        if let Some(k) = m.arr_len_at_site.get(&site).and_then(|b| b.len) {
            m.arr_birth_len.insert(tm, k);
        }
        // A push-filled `new Array()` is born with its final length.
        if let Some(&fill) = m.push_fill_new_sites.get(&site) {
            crate::push_fills::born(m, tm, fill);
        }
    }
    let t = m.tok(TokKind::Obj(tm, NO_CTX));
    m.site_tmpls.insert(key, t);
    tm
}

/// ToObject(`v`) (ECMA-262 7.1.18) into a new node (`to_object_into`),
/// its wrapper objects born at `call` (one per primitive kind, as
/// `method_on_prim` names them).
pub(crate) fn to_object(m: &mut Model, v: NodeId, call: CallId) -> NodeId {
    let mut ws = [(0 as Tok, 0 as NodeId); 4];
    for (i, what) in ["BoxedNumber", "BoxedString", "BoxedBoolean", "Object(prim)"]
        .into_iter()
        .enumerate()
    {
        let t = site_tmpl(m, call, what);
        let w = m.node_l("to-object-wrapper");
        m.add_tok(w, t);
        ws[i] = (t, w);
    }
    let to = m.node_l("to-object");
    to_object_into(m, v, ws, to);
    to
}

/// `to` ⊇ ToObject(`v`) (ECMA-262 7.1.18): `v`'s objects as they are
/// (`Cons::Objects`), and for each primitive kind `v` may hold the wrapper
/// of that kind (`wrappers`: token and the node holding it, in the order
/// Number, String, Boolean, BigInt-or-Symbol, the guards `BOXED_NUM`,
/// `BOXED_STR`, `BOXED_BOOL`, `BOXED_OTHER`); `undefined` / `null` throw
/// and add nothing. The primitive itself never reaches `to`. A String
/// wrapper's own indices hold its characters (22.1.1.1 String exotic
/// objects): the String wrapper's elements are strings.
pub(crate) fn to_object_into(m: &mut Model, v: NodeId, wrappers: [(Tok, NodeId); 4], to: NodeId) {
    let chars = m.num_fields(wrappers[1].0);
    m.add_prim(chars, prim::STR);
    m.cons(v, Cons::Objects { dst: to });
    for (test, (_, from)) in [
        guard::BOXED_NUM,
        guard::BOXED_STR,
        guard::BOXED_BOOL,
        guard::BOXED_OTHER,
    ]
    .into_iter()
    .zip(wrappers)
    {
        m.cons(
            v,
            Cons::Guard {
                test,
                from,
                dst: to,
            },
        );
    }
}
/// A synthetic resolve function of the promise whose settled value is
/// `dst` (ECMA-262 27.2.1.3.2): its first parameter is adopted into `dst`
/// (`adopt`), so a thenable passes on what it resolves, never itself.
pub fn resolver(m: &mut Model, by: CallId, dst: NodeId) -> Tok {
    if !m.reach_thenable {
        return plain_resolver(m, dst);
    }
    if let Some(&t) = m.resolvers.get(&dst) {
        return t;
    }
    // Memoized before `adopt`, which asks for this resolver again.
    let (tok, p) = settle_fn(m, "<resolver>", dst, false);
    m.resolvers.insert(dst, tok);
    adopt(m, by, p, dst);
    tok
}

/// A synthetic reject function (27.2.1.3.1): its first parameter is the
/// reason, as it is (a reject function adopts nothing).
pub fn rejecter(m: &mut Model, dst: NodeId) -> Tok {
    if !m.reach_thenable {
        return plain_resolver(m, dst);
    }
    if let Some(&t) = m.rejecters.get(&dst) {
        return t;
    }
    let (tok, _) = settle_fn(m, "<rejecter>", dst, true);
    m.rejecters.insert(dst, tok);
    tok
}

/// `PERRY_FACTS_REACH_THENABLE=0` (`reach::thenable`): the previous model,
/// one synthetic function for both sides whose first parameter flows into
/// `dst` as it is.
pub(crate) fn plain_resolver(m: &mut Model, dst: NodeId) -> Tok {
    if let Some(&t) = m.resolvers.get(&dst) {
        return t;
    }
    let (tok, _) = settle_fn(m, "<resolver>", dst, true);
    m.resolvers.insert(dst, tok);
    tok
}

/// A resolve or reject function's token and first parameter; `plain`: the
/// parameter flows into `dst` as it is.
fn settle_fn(m: &mut Model, name: &str, dst: NodeId, plain: bool) -> (Tok, NodeId) {
    let fx = m.funcs.len() as FnIdx;
    let tok = m.tok(TokKind::Func(fx, NO_CTX));
    let this = m.node_l("this(resolver)");
    let ret = m.node_l("ret(resolver)");
    let args_all = m.node_l("args(resolver)");
    let p = m.node_l("resolver-param");
    if plain {
        m.edge(p, dst);
    }
    m.funcs.push(FuncInfo {
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: u32::MAX,
        raise: u32::MAX,
        name: name.into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all,
        has_arguments: false,
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token: tok,
        ctor_of: None,
        lexical_this: true,
        no_summary: false,
        foreign_params: false,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
        ret_param: None,
    });
    m.add_prim(ret, prim::UNDEF);
    (tok, p)
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
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token: tok,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        foreign_params: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
        ret_param: None,
    });
    m.other_accessor = Some(tok);
    tok
}

/// A promise's settled value is its [[PromiseResult]] internal slot
/// (ECMA-262 27.2.6), no property: no chain holds it, so a value that is
/// no promise reads nothing under it (`effects::is_internal_key`).
pub(crate) fn promise_result_atom(m: &mut Model) -> Atom {
    effects::internal_atom(m, "PromiseResult")
}

/// A host Worker's internal slots: `[[WorkerPayload]]` (the clone tree a
/// message delivers) and `[[WorkerEvent]]` (the event its listeners get).
pub(crate) fn worker_slot_atom(m: &mut Model, name: &str) -> Atom {
    effects::internal_atom(m, name)
}

/// W-HOST step 5: the construct queues the `worker` process event
/// (worker_surface.rs `queue_process_worker_event`: [[Get]](process,
/// "nextTick") called with a host callback capturing `w`, which does
/// [[Get]](process, "emit") and calls it with ("worker", w); this =
/// process). Unwritten, both are the realm's natives: `nextTick` queues
/// the callback, `emit` hands `w` to the process listeners
/// (`Model::process_event`). A key the program or unknown code wrote
/// (`opaque_written`): a stored function is called, an opaque write is the
/// unknown call (the replay). `tick` passes `w` itself for the callback
/// (its one capture). `emit` = ("worker", w), `tick` = (w).
pub(crate) fn process_worker_event(m: &mut Model, w: NodeId, emit: CallId, tick: CallId) {
    let p = m.tok(TokKind::Builtin(BuiltinNs::Process));
    let pe = m.process_event;
    m.edge(w, pe);
    for (k, call) in [("emit", emit), ("nextTick", tick)] {
        let a = m.atom(k);
        let _ = opaque_written(m, p, a, NativeMiss::Call(p, call));
    }
}

/// The settled-value field of a promise token.
fn promise_value(m: &mut Model, p: Tok) -> NodeId {
    let a = promise_result_atom(m);
    m.field(p, a)
}

/// R-JOBTHROW: a rejected promise's reason is its [[PromiseResult]] in the
/// rejected state (ECMA-262 27.2.1.7 RejectPromise). The model keeps it in
/// its own internal slot beside the fulfilled value: what the promise's
/// rejection handlers (27.2.5.4 PerformPromiseThen, 27.2.2.1) and an
/// `await` of it (27.7.5.3 Await) read, never a program `catch` directly.
/// A rejection the model cannot place on its promise stays in `thrown`,
/// which every rejection reader also reads.
pub(crate) fn promise_reason_atom(m: &mut Model) -> Atom {
    effects::internal_atom(m, "PromiseReason")
}

/// The rejection-reason field of a promise token (`promise_reason_atom`).
pub(crate) fn promise_reason(m: &mut Model, p: Tok) -> NodeId {
    let a = promise_reason_atom(m);
    let r = m.field(p, a);
    // R-HOSTREPORT: a rejection no handler takes is the host's to report
    // (promise/rejection.rs `report_unhandled`).
    let h = m.host_report;
    m.edge(r, h);
    r
}

/// R-HOSTREPORT: the runtime's readers of what it reports as uncaught
/// (exception.rs `print_uncaught`): Get `message` and Get `stack` (a getter
/// runs with this = the value), ToString of each, and ToString of the value.
pub(crate) fn host_report_readers(m: &mut Model, host: NodeId) {
    string_methods::read_prim(m, host, "message", "uncaught report");
    string_methods::read_prim(m, host, "stack", "uncaught report");
    string_methods::to_prim(m, host, "uncaught report");
}

/// `v` adopted by a promise: the value, the settled value of promise
/// tokens in it, and whatever a thenable's `then` resolves.
pub fn adopt(m: &mut Model, by: CallId, v: NodeId, dst: NodeId) {
    let thrown = m.thrown;
    adopt_r(m, by, v, dst, thrown)
}

/// `adopt` by a promise whose rejection reason is `rej`: an adopted
/// promise's reason and what a thenable's `then` rejects with reach `rej`
/// (27.2.1.3.2 step 13, 27.2.2.2 NewPromiseResolveThenableJob).
pub fn adopt_r(m: &mut Model, by: CallId, v: NodeId, dst: NodeId, rej: NodeId) {
    let ra = promise_reason_atom(m);
    m.cons(
        v,
        Cons::Load {
            key: Key::Named(ra),
            dst: rej,
        },
    );
    if m.reach_thenable {
        m.cons(v, Cons::Fulfil { dst });
    } else {
        m.edge(v, dst);
    }
    let a = promise_result_atom(m);
    m.cons(
        v,
        Cons::Load {
            key: Key::Named(a),
            dst,
        },
    );
    let res = resolver(m, by, dst);
    let rej = rejecter(m, rej);
    let rn = m.node_l("resolver");
    m.add_tok(rn, res);
    let jn = m.node_l("rejecter");
    m.add_tok(jn, rej);
    let ret = m.node_l("then-ret");
    let then = m.atom("then");
    let call = m.calls.len() as CallId;
    let _call = m.chain_call(
        by,
        0,
        v,
        CallSite {
            first_arg_fixed: false,
            this: ThisArg::Node(v),
            args: vec![rn, jn],
            spread: vec![],
            ret,
            site: call,
            region: m.thrown,
        },
        move |m, tg, call| {
            m.cons(
                tg,
                Cons::MCall {
                    key: Key::Named(then),
                    call,
                },
            )
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
    /// Buffers. Only the methods the realm declares OWN on
    /// `Buffer.prototype` (`buffer_own_method`: the numeric readers,
    /// `equals`): the runtime's Buffer name dispatch runs them while that
    /// prototype's own shadow bit is clear, which a write onto it sets.
    Buffer,
    /// Primitive Numbers: a member is `%Number.prototype%`'s (ECMA-262
    /// 7.3.2 GetV: ToObject, then [[Get]] on the wrapper, which owns no
    /// string key).
    Num,
    /// Native Promise objects (`TmplKind::BObj { what: "Promise" }`, made
    /// by the realm: their [[Prototype]] is `%Promise.prototype%`).
    Promise,
    /// Primitive Booleans: a member is `%Boolean.prototype%`'s (7.3.2 GetV;
    /// a Boolean wrapper owns no property, 20.3.4). Only the absent-key
    /// claim (`derive::family_absent`) is over it; no NativeMember.
    Bool,
    /// Typed arrays (integer-indexed exotic objects, ECMA-262 10.4.5) the
    /// realm made (`TmplKind::BObj { what: "TypedArray" }`; their kind is
    /// not kept, so their [[Prototype]] is one of the kinds' prototypes,
    /// each of whose [[Prototype]] is `%TypedArray%.prototype`, 23.2.7).
    /// R9BIRTH7: reads only (`typed_array_read_member`); a call keeps its
    /// other reason.
    TypedArray,
    /// An object the runtime births as a declaration's `fresh(P)` result
    /// (`TmplKind::BObj { what: P }`, P a realm prototype such as
    /// `Storage.prototype`): its [[Prototype]] is P, and a member of it is
    /// one P declares own (`realm_entry(P).own`). Its own keys are the
    /// stores the solve saw (`derive::native_own_free`).
    Declared(&'static str),
}

impl NativeFamily {
    /// The own prototype a lookup on a value of the family passes before
    /// `Object.prototype`, by the kind name `native_chain` takes (as
    /// `chain_holder` names it for a receiver token).
    pub(crate) fn chain_holder(self) -> &'static str {
        match self {
            NativeFamily::Str => "String",
            NativeFamily::Array => "Array",
            NativeFamily::Collection(w) | NativeFamily::Declared(w) => w,
            NativeFamily::RegExp => "RegExp",
            NativeFamily::Buffer => "Buffer",
            NativeFamily::Num => "Number",
            NativeFamily::Promise => "Promise",
            NativeFamily::Bool => "Boolean",
            NativeFamily::TypedArray => "TypedArray",
        }
    }
}

/// A realm prototype `what` whose declared fresh objects form a
/// `NativeFamily::Declared` (whole-program only: a unit does not see the
/// other units' stores onto it).
pub(crate) fn declared_family(m: &Model, what: &str) -> bool {
    !m.unit_mode && what.ends_with(".prototype") && members::realm_entry(what).is_some()
}

/// An object the runtime births as a declaration's `fresh(P)` result for a
/// `NativeFamily::Declared` prototype P: an ordinary object for its own
/// String keys (perry-runtime keeps them as ordinary own properties; a Web
/// Storage item is mirrored as one, web_storage.rs `sync_named_property`),
/// so a shaped key writes only the names it admits (`shape_splits`).
pub(crate) fn declared_box(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what, .. } if declared_family(m, what)))
}

/// Does the intrinsic chain of every value of `fam` define `name` before
/// the program writes anything (`members`): a function for a call (`call`),
/// a function or a data member for a read? `constructor` and `__proto__`
/// are no member here (their value is no builtin function or primitive).
pub(crate) fn native_described(m: &Model, fam: NativeFamily, name: &str, call: bool) -> bool {
    let mem = match fam {
        // A method call only: a read of the member is the runtime's bound
        // closure, no value the description names.
        NativeFamily::Buffer => return call && buffer_own_method(m, name),
        NativeFamily::Str => members::intrinsic_member(Family::Str, name),
        NativeFamily::Array => members::intrinsic_member(Family::Array, name),
        NativeFamily::Collection(what) => members::collection_member(what, name),
        NativeFamily::RegExp if string_methods::regexp_described(name) => Some(Mem::Fn),
        NativeFamily::RegExp => regexp_slot_getter(name).map(Mem::Prim),
        NativeFamily::Num => members::intrinsic_member(Family::Num, name),
        NativeFamily::Promise => members::intrinsic_member(Family::Promise, name),
        NativeFamily::Bool => members::intrinsic_member(Family::Bool, name),
        NativeFamily::TypedArray => return !call && typed_array_read_member(name),
        // A member P declares own: a method (a call or a read of it), or an
        // accessor with a getter for a read. `constructor` is no member.
        NativeFamily::Declared(p) => {
            return name != "constructor"
                && members::realm_entry(p).is_some_and(|e| {
                    e.own.iter().any(|o| {
                        !o.symbol
                            && o.key == name
                            && match o.value {
                                perry_hir::realm_manifest::RealmOwnValue::Reference(_) => true,
                                perry_hir::realm_manifest::RealmOwnValue::Accessor {
                                    get, ..
                                } => !call && get.is_some(),
                                _ => false,
                            }
                    })
                })
        }
    };
    match mem {
        Some(Mem::Fn) => true,
        // R9READ6: a read of a family prototype's `constructor` is the family's
        // intrinsic constructor (a data property, e.g. 23.1.3.4).
        Some(Mem::Prim(_) | Mem::Ctor) => !call,
        _ => false,
    }
}

/// R9BIRTH7: a key whose [[Get]] on every typed array names one builtin
/// member whatever its kind: `BYTES_PER_ELEMENT`, a non-writable,
/// non-configurable data property of every kind's prototype (23.2.7.2),
/// or a key the realm declares on `%TypedArray%.prototype` (its accessors
/// `length`, `byteLength`, `byteOffset`, `buffer`, 23.2.3; its methods)
/// that no kind's prototype holds (each holds only `constructor` and
/// `BYTES_PER_ELEMENT`; `Uint8Array.prototype` also its base64/hex
/// methods). Not `constructor` (each kind's own constructor).
fn typed_array_read_member(name: &str) -> bool {
    if name == "BYTES_PER_ELEMENT" {
        return true;
    }
    // R9READ8: each kind's prototype owns `constructor`, a data property
    // whose value is that kind's constructor (23.2.6.2 / 23.2.7.2): with no
    // own `constructor` on the receiver and no write onto a builtin
    // prototype (the caller's checks) the read is one of the kinds'
    // intrinsic constructors, a closed set of builtin function values.
    if name == "constructor" {
        return true;
    }
    if name.starts_with(WK_PREFIX) {
        return false;
    }
    let on = |e: &str| {
        members::realm_entry(e)
            .is_some_and(|e| e.own.iter().any(|p| members::realm_key_is(p, name)))
    };
    on("%TypedArray%.prototype") && !on("Uint8Array.prototype")
}

/// A read-only accessor of `%RegExp.prototype%` whose getter reads only
/// internal slots of its receiver (ECMA-262 22.2.6: `source` reads
/// [[OriginalSource]], each flag getter [[OriginalFlags]] through
/// RegExpHasFlag): no [[Get]] on the receiver, so no program value decides
/// it. Not `flags` (22.2.6.4 reads the flag keys through [[Get]], which an
/// own definition on the receiver changes), not `lastIndex` (an own
/// writable data property of each RegExp object).
fn regexp_slot_getter(name: &str) -> Option<u16> {
    use crate::model::prim;
    match name {
        "source" => Some(prim::STR),
        "global" | "ignoreCase" | "multiline" | "dotAll" | "unicode" | "unicodeSets" | "sticky"
        | "hasIndices" => Some(prim::BOOL),
        _ => None,
    }
}

/// A method the realm declares an OWN data member of `Buffer.prototype`
/// (its native, no accessor). A method past it is %TypedArray%'s, which
/// another prototype's writes decide: no Buffer NativeMember.
fn buffer_own_method(m: &Model, name: &str) -> bool {
    m.declarations
        .named("Buffer.prototype", m)
        .and_then(|h| m.declarations.property(h, name, m))
        .is_some_and(|p| p.native.is_some() && p.get.is_none() && p.set.is_none())
}

/// `Object.prototype`'s intrinsic description (`members`) lacks `name`
/// (`derive::absent_key`): `constructor`, `toString`, `__proto__` and the
/// other members are not absent.
/// DEC 250: is the read of key `a` on a chain that passes `Object.prototype`
/// only final (`members::locked_read` of a plain object, on
/// `members::store_admitted`)? Then no store onto `Object.prototype`, the
/// program's or one through ⊤, writes it (the runtime refuses the store).
pub(crate) fn object_chain_locked(m: &Model, a: Atom) -> bool {
    members::locked_read(Family::Object, None, &m.atoms[a as usize])
}

/// R9BIRTH4: neither %Function.prototype% nor %Object.prototype% (a class
/// constructor's chain end) describes `name`.
pub(crate) fn function_chain_lacks(name: &str) -> bool {
    members::member(members::Family::Function, name).is_none()
}

pub(crate) fn object_proto_lacks(name: &str) -> bool {
    members::member(members::Family::Object, name).is_none()
}

/// `%Array.prototype%`'s intrinsic description lacks `name`, and so does
/// `%Object.prototype%`'s, which follows it on an array's chain
/// (`derive::array_absent`).
pub(crate) fn family_chain_lacks(fam: NativeFamily, name: &str) -> bool {
    let f = match fam {
        NativeFamily::Array => members::Family::Array,
        NativeFamily::Str => members::Family::Str,
        NativeFamily::Num => members::Family::Num,
        NativeFamily::Bool => members::Family::Bool,
        _ => return false,
    };
    members::member(f, name).is_none()
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
    let op = m.tok(TokKind::ObjectProto);
    let written = match miss {
        NativeMiss::Lookup(_, _) => native_accessor_written(m, op, a),
        _ => native_written(m, op, a),
    };
    if written {
        return native_protocol::answer_stored(m, op, a, miss);
    }
    if let NativeMiss::Lookup(dst, setter) = miss {
        native_accessor_known(m, op, a, dst, setter);
    }
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
    let holder = what.or((fam == Family::Error).then_some("Error"));
    members::locked_read(fam, what, &m.atoms[a as usize]) || intrinsic_miss_on(m, holder, a, miss)
}

/// [`intrinsic_miss`] for a lookup that passes the own prototype of builtin
/// kind `holder` (`TmplKind::BObj::what`, `Array` for an array) before
/// `Object.prototype`: that prototype and every native prototype past it
/// ([`native_chain`]) are asked under their own identity
/// (`TokKind::NativeProto`), so a write onto any of them, or unknown code
/// holding the realm where the lock admits the store (`realm_replaced`),
/// reaches this lookup, and `Object.prototype` stays under its lock.
pub(crate) fn intrinsic_miss_on(
    m: &mut Model,
    holder: Option<&str>,
    a: Atom,
    miss: NativeMiss,
) -> bool {
    if let Some(what) = holder {
        for name in native_chain(what) {
            let np = m.tok(TokKind::native_proto(&name));
            let written = match miss {
                NativeMiss::Lookup(_, _) => native_accessor_written(m, np, a),
                _ => native_written(m, np, a),
            };
            if written {
                if !native_protocol::answer_stored(m, np, a, miss.clone()) {
                    return false;
                }
            } else {
                m.native_missing.push((np, a, miss.clone()));
            }
        }
    }
    intrinsic_miss(m, a, miss)
}

/// The native prototypes a lookup that passes the own prototype of builtin
/// kind `holder` passes before `Object.prototype`, each under its own
/// identity: `holder` itself, then every [[Prototype]] the realm declares
/// past it (a Buffer's: `Uint8Array.prototype`, `%TypedArray%.prototype`;
/// OrdinaryGet, ECMA-262 10.1.8.1, walks them all). A program store onto
/// any of them is a write this lookup sees, with or without the realm held.
pub(crate) fn native_chain(holder: &str) -> Vec<String> {
    // Each prototype under its constructor's name (`realm_proto_value`).
    let mut out = vec![holder
        .strip_suffix(".prototype")
        .unwrap_or(holder)
        .to_string()];
    let own = loads::view_chain(Some(holder)).or_else(|| match holder.ends_with(".prototype") {
        true => members::realm_entry(holder),
        false => members::realm_entry(&format!("{holder}.prototype")),
    });
    let mut at = own.and_then(|e| e.proto).and_then(members::realm_entry);
    while let Some(e) = at {
        if e.name == "Object.prototype" {
            break;
        }
        let name = e.name.strip_suffix(".prototype").unwrap_or(e.name);
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        at = e.proto.and_then(members::realm_entry);
    }
    out
}

/// The own prototype a lookup on builtin receiver `t` passes before
/// `Object.prototype`, by its kind: a builtin object's (`BObj::what`), an
/// array's `Array.prototype`.
pub(crate) fn chain_holder(m: &Model, t: Tok) -> Option<&'static str> {
    match &m.toks[t as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::BObj { what, .. } => Some(*what),
            TmplKind::Arr { .. } => Some("Array"),
            _ => None,
        },
        _ => None,
    }
}

/// DEC 250: is the read of key `a` on the own prototype of builtin kind
/// `what` final (the lock refuses every store that changes it)? Its image
/// members on a sealed prototype (`image_locked_prototype`), its family's
/// locked chain (`members::locked_read`); anything else is not.
pub(crate) fn proto_read_final(m: &Model, what: &str, a: Atom) -> bool {
    let name = &m.atoms[a as usize];
    if let Some(holder) = members::image_locked_prototype(what) {
        return image_member(m, holder, name);
    }
    members::locked_read(members::bobj_family(what), Some(what), name)
}

/// DEC 250: is the read of named key `a` on the intrinsic chain of builtin
/// receiver `t` final (`members::locked_read` of its family)? A native
/// prototype (a native class a program class extends) is not: past it the
/// collapsed token may stand for an add-open prototype (`%TypedArray%`,
/// `Error.prototype`), not `Object.prototype`.
pub(crate) fn locked_chain_read(m: &Model, t: Tok, a: Atom) -> bool {
    if matches!(m.toks[t as usize], TokKind::NativeProto(..)) {
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
    // Rule HI: `@@hasInstance` on a class object, whatever its `extends`.
    // The lookup passes the class's own keys and those of its parents
    // (program tokens or ⊤: the chain walk reads them, and a `setPrototypeOf`
    // or `__proto__` store is a parent there), then ends at
    // `%Function.prototype%`: a superclass is a constructor or null (ECMA-262
    // 15.7.14), and every constructor's chain ends there unless a program
    // set its [[Prototype]]. There `@@hasInstance` is non-writable and
    // non-configurable (20.2.3.6), an image member the runtime refuses to
    // replace (DEC 250); a plain store of the key on the class or a parent
    // fails against it (10.1.9.2 step 2.a), so only a define installs one,
    // as an own key the walk reads. The intrinsic end is final.
    if matches!(m.toks[t as usize], TokKind::ClassObj(_))
        && m.atoms[a as usize] == crate::model::WK_HAS_INSTANCE
    {
        return true;
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
    // A Map or Set iterator, a RegExp, a Date: past its own keys the chain
    // reaches `%MapIteratorPrototype%` / `%SetIteratorPrototype%` /
    // `RegExp.prototype` / `Date.prototype`, whose declared image members
    // are locked (DEC 250); every other key may be added there: not final.
    if let Some(holder) = what.and_then(members::image_locked_prototype) {
        return image_member(m, holder, &m.atoms[a as usize]);
    }
    lock_family(m, t).is_some_and(|f| members::locked_read(f, what, &m.atoms[a as usize]))
}

/// The read of named key `a` on builtin receiver `t` is final
/// (`locked_chain_read`) and its intrinsic chain defines `a`: a locked
/// native member is found, never the chain end's `undefined`
/// (`Promise.prototype.then` on a promise).
pub(crate) fn locked_chain_defines(m: &Model, t: Tok, a: Atom) -> bool {
    if !locked_chain_read(m, t, a) {
        return false;
    }
    // A RegExp, a Date, a Map/Set/String iterator: the locked prototype
    // (`members::image_locked_prototype`) defines `a` when its declaration
    // holds it (a locked absent key is final too, and defines nothing).
    if let TokKind::Obj(tm, _) = m.toks[t as usize] {
        if let TmplKind::BObj { what, .. } = m.tmpls[tm as usize].kind {
            if let Some((holder, _)) = members::image_locked_prototype(what) {
                return m.declarations.named(holder, m).is_some_and(|h| {
                    m.declarations
                        .property(h, &m.atoms[a as usize], m)
                        .is_some()
                });
            }
        }
    }
    lock_family(m, t).is_some_and(|f| members::member(f, &m.atoms[a as usize]).is_some())
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
        // `RegExp.prototype` is sealed (DEC 250) and takes adds: an image
        // member is final, an absent key is not (`locked_chain_read`'s
        // predicate for a RegExp object).
        NativeFamily::RegExp => members::image_locked_prototype("RegExp")
            .is_some_and(|holder| image_member(m, holder, name)),
        // `Buffer.prototype` is a host prototype the program may write: its
        // writes are obligation 4's (`derive::builtin_protos_unwritten`).
        NativeFamily::Buffer => false,
        NativeFamily::Num => members::locked_read(Family::Num, None, name),
        NativeFamily::Promise => members::locked_read(Family::Promise, None, name),
        NativeFamily::Bool => members::locked_read(Family::Bool, None, name),
        // `%TypedArray%.prototype` is no locked prototype: its writes are
        // obligation 4's (`derive::builtin_protos_unwritten`).
        NativeFamily::TypedArray => false,
        // A host prototype the program may write: obligation 4's.
        NativeFamily::Declared(_) => false,
    }
}

/// The opaque builtin function value; in explain mode, counted by the
/// arm that minted it (`perry: wpfacts explain: builtin-fn minted`).
fn builtin_fn(m: &mut Model, why: &'static str) -> Tok {
    crate::family_reach::generic(m, why);
    if m.explain {
        *m.bf_mints.entry(why).or_insert(0) += 1;
    }
    m.tok(TokKind::BuiltinFn(Native::Any))
}

/// `PERRY_WPF_ANYFALL=1`: one stderr line per place a builtin member or
/// builtin function value falls back to the opaque builtin function value or
/// to `Protocol::Any` on its arguments, naming the family and member.
pub(crate) fn anyfall(why: &str, fam: &dyn std::fmt::Debug, name: &dyn std::fmt::Debug) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("PERRY_WPF_ANYFALL").is_some()) {
        eprintln!("perry: wpfacts anyfall: {why} | {fam:?} | {name:?}");
    }
}

/// Resolve the member's object reference before interning its token. The
/// receiver family locates the holder, but is never part of value identity.
/// The realm prototype a builtin object of family `fam` (`what`) reads its
/// members from (past a Buffer's own prototype, `member_native`).
fn realm_holder(m: &Model, fam: Family, what: Option<&'static str>) -> Option<usize> {
    match what {
        Some("TypedArray" | "Buffer") => m.declarations.named("%TypedArray%.prototype", m),
        // Every iterator kind resolves its members as `Iterator.prototype`'s.
        Some(w) if members::bobj_family(w) == Family::Iterator => {
            m.declarations.named("Iterator.prototype", m)
        }
        // The prototype of an object a declaration made.
        Some(w) if w.ends_with(".prototype") => m.declarations.named(w, m),
        // An ordinary object its creator gave `Object.prototype`'s chain
        // (`box_chain`: a `JSON.parse` object, 25.5.1 / 10.1.13
        // OrdinaryObjectCreate): its members are `Object.prototype`'s.
        Some(w) if members::box_chain(w) == members::BoxChain::Object => {
            m.declarations.family(Family::Object, m)
        }
        Some(w) => m.declarations.named(&format!("{w}.prototype"), m),
        None => m.declarations.family(fam, m),
    }
}

/// Whether the realm declares key `name` an accessor (`accessor` row) where
/// a builtin object of family `fam` (`what`) reads it: a load runs the
/// getter, so its value is the getter's result, never a function of the
/// chain (`Response.prototype.body`).
fn realm_accessor(m: &Model, fam: Family, what: Option<&'static str>, name: &str) -> bool {
    realm_getter(m, fam, what, name).is_some()
}

/// The getter (its realm identity, `RegExp.prototype.flags.<get>`) of the
/// accessor `name` where a builtin object of family `fam` (`what`) reads it
/// (`realm_accessor`).
fn realm_getter(
    m: &Model,
    fam: Family,
    what: Option<&'static str>,
    name: &str,
) -> Option<&'static str> {
    let own = (what == Some("Buffer"))
        .then(|| m.declarations.named("Buffer.prototype", m))
        .flatten();
    [own, realm_holder(m, fam, what)]
        .into_iter()
        .flatten()
        .find_map(|h| m.declarations.property(h, name, m))?
        .get
}

/// The declaration of the getter accessor `name` runs where a builtin
/// object of family `fam` (`what`) reads it (`realm_accessor`), when the
/// runtime declares its result (`effects::declared_at`, the getter's own
/// `.<get>` entry): a load reads that result (`effects::getter_load`).
fn declared_accessor(
    m: &Model,
    fam: Family,
    what: Option<&'static str>,
    name: &str,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    let getter = realm_getter(m, fam, what, name)?;
    effects::declared_at(getter.strip_suffix(".<get>")?, "<get>")
}

/// What a load of an accessor whose getter the realm does not declare
/// (`realm_accessor`; a declared one: `effects::getter_load`) reads: the
/// host's result of the getter, unknown.
fn undeclared_getter_result(m: &mut Model, dst: NodeId) {
    m.add_top(dst, |_| {
        "result of a getter the realm does not declare".into()
    });
    m.add_prim(dst, prim::ALL);
}

fn member_native(m: &Model, fam: Family, what: Option<&'static str>, name: &str) -> Option<Native> {
    // A Buffer's own prototype first: past it the chain is a typed array's.
    if what == Some("Buffer") {
        if let Some(p) = m
            .declarations
            .named("Buffer.prototype", m)
            .and_then(|h| m.declarations.property(h, name, m))
        {
            return p.native.map(|identity| Native::Member {
                identity,
                holder: None,
            });
        }
    }
    // A typed array's chain starts at its kind's prototype
    // (`loads::view_chain`): a method only `Uint8Array.prototype` holds
    // (`toHex`, `toBase64`, `setFromHex`, `setFromBase64`; 23.3.2) is that
    // native (on another kind `undefined`: `loads::family_load`).
    if matches!(what, Some("TypedArray" | "Buffer")) {
        if let Some(identity) = m
            .declarations
            .named("Uint8Array.prototype", m)
            .and_then(|h| m.declarations.property(h, name, m))
            .and_then(|p| p.native)
        {
            return Some(Native::Member {
                identity,
                holder: None,
            });
        }
    }
    // The prototype of an object a declaration made
    // (`AbortController.prototype`): a method whose effects it declares
    // (`effects::declared_at`); any other member stays opaque.
    if let Some(w) = what.filter(|w| w.ends_with(".prototype")) {
        effects::declared_at(w, name)?;
        let identity = m
            .declarations
            .named(w, m)
            .and_then(|h| m.declarations.property(h, name, m))?
            .native?;
        return Some(Native::Member {
            identity,
            holder: None,
        });
    }
    let holder = realm_holder(m, fam, what)?;
    let identity = m
        .declarations
        .property(holder, name, m)
        .and_then(|p| p.native)
        .or_else(|| {
            // Past the holder, `Object.prototype`'s member, where no own
            // prototype of the family holds the key. A chain the realm
            // declares link by link (`loads::realm_chain_start`: a typed
            // array's, a Buffer's, an ArrayBuffer's, a DataView's, a
            // realm-born object's) is the realm's exactly: the key is
            // `Object.prototype`'s when that is where the chain finds it
            // (10.1.8.1: `valueOf`, `hasOwnProperty`).
            let past = match loads::realm_chain_start(what) {
                Some(p) => matches!(
                    members::realm_chain_find(p, name),
                    members::RealmFind::Own(_, at) if at.name == "Object.prototype"
                ),
                None => members::own(fam, name).is_none(),
            };
            if !past {
                return None;
            }
            let object = m.declarations.family(Family::Object, m)?;
            m.declarations.property(object, name, m)?.native
        })?;
    Some(Native::Member {
        identity,
        holder: None,
    })
}

/// Canonical coordinates are resolved with the value's shape, once.
fn prototype_model(
    owner: &'static str,
    key: &'static str,
) -> Option<(Family, Option<&'static str>, &'static str)> {
    let owner = owner.strip_suffix(".prototype")?;
    let (fam, what) = match owner {
        "Object" => (Family::Object, None),
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
    if let TokKind::BuiltinFn(Native::Member { identity, .. }) = k {
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
    // Every `Array.prototype` method is intentionally generic (ECMA-262
    // 23.1.3: `ToObject(this)`, then LengthOfArrayLike and Get/HasProperty of
    // its indices; the result of `map`/`filter`/`slice`/... of a non-Array is
    // ArraySpeciesCreate's ArrayCreate): a function, a bound or class
    // object, a builtin object or prototype, a module namespace object run it
    // as an array-like (`array_method_on`), as an object literal does.
    if fam == Family::Array && what.is_none() {
        return r != TOP;
    }
    // `Error.prototype.toString` is generic over an object `this`
    // (ECMA-262 20.5.3.4: steps 1-2 only require an Object; `name` and
    // `message` through [[Get]]): it runs on any object (`error_to_string`).
    if fam == Family::Error && what.is_none() {
        return r != TOP;
    }
    match &m.toks[r as usize] {
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            TmplKind::Arr { .. } => fam == Family::Array && what.is_none(),
            // An object a declaration made has the kind `X.prototype`
            // (`effects::Run::fresh`), its natives the owner `X`.
            TmplKind::BObj { what: w, .. } => {
                (what == Some(*w) || what.is_some() && w.strip_suffix(".prototype") == what)
                    && members::bobj_family(w) == fam
            }

            _ => false,
        },
        TokKind::Func(..) | TokKind::Bound(_) | TokKind::BuiltinFn(_) | TokKind::ClassObj(_) => {
            fam == Family::Function
        }
        TokKind::Builtin(_) => fam == Family::Function && builtin_function(m, r),
        _ => false,
    }
}

/// Is receiver token `r` a value that is never callable: an object the
/// program or the runtime made that is no function (a literal, a class
/// instance or prototype, an array, a builtin object of a non-function
/// kind), or `Object.prototype`? `false` for a function value, a parameter
/// marker (it stands for the caller's values), ⊤, and every other token.
/// A `Function.prototype` method called with such a `this` throws a
/// TypeError before it reads `this` or an argument (`Model::native_call`).
pub(crate) fn never_callable(m: &Model, r: Tok) -> bool {
    if m.is_marker(r) {
        return false;
    }
    match &m.toks[r as usize] {
        TokKind::Obj(tm, _) => !matches!(&m.tmpls[*tm as usize].kind,
            TmplKind::BObj { what, .. } if members::bobj_family(what) == Family::Function),
        TokKind::Proto(_) | TokKind::ObjectProto => true,
        // A realm namespace object the realm describes with [[Prototype]]
        // `Object.prototype` (`globalThis`, `Math`, `JSON`, `Reflect`,
        // `process`, a node module namespace): an ordinary object, no
        // function (`members::ns_ordinary_object`).
        TokKind::Builtin(ns) => members::ns_ordinary_object(*ns),
        _ => false,
    }
}

/// Whether a `Number`/`Boolean`/`Symbol`/`BigInt` prototype method
/// (family `fam`) finds no [[NumberData]]/[[BooleanData]]/[[SymbolData]]/
/// [[BigIntData]] on receiver `r`: `r` is an object that is no primitive
/// wrapper (a program object, a prototype, a builtin function; not a
/// builtin namespace: `Number.prototype` is itself a Number object, 21.1.3;
/// only `Family::Wrapper` objects carry such a slot, 21.1.1.1, 20.3.1.1,
/// 20.4.1, 21.2.1, 7.1.18). A marker or an unknown value may be one.
///
/// A `%TypedArray%.prototype` method (`what` `TypedArray`; a concrete
/// typed array prototype's own methods too) starts with ValidateTypedArray
/// or ValidateUint8Array (ECMA-262 23.2.4.4, 23.2.3): [[TypedArrayName]]
/// is missing from every object no typed array constructor made (a program
/// object or array, a function, a builtin function or namespace, a
/// prototype, a builtin object of a described kind that is no typed
/// array); a `TypedArray`/`Buffer` object, an undescribed (`Box`/`Opaque`
/// family) builtin object, a marker or an unknown value may have it. The
/// runtime's check: `typed_array_proto_thunks.rs` `ta_receiver_or_throw`.
pub(crate) fn no_this_value(m: &Model, fam: Family, what: Option<&str>, r: Tok) -> bool {
    let typed = fam == Family::Opaque && what == Some("TypedArray");
    // `ArrayBuffer.prototype.{resize, slice, transfer, transferToFixedLength}`
    // and `SharedArrayBuffer.prototype.{grow, slice}` start with
    // RequireInternalSlot(this, [[ArrayBufferData]]) (or
    // [[ArrayBufferMaxByteLength]]) and IsSharedArrayBuffer, before any
    // argument coercion (ECMA-262 25.1.6.6 step 2, 25.1.6.7 steps 2-4,
    // 25.1.6.8-9 via ArrayBufferCopyAndDetach step 2, 25.2.5.3 step 2,
    // 25.2.5.6 steps 2-3; the runtime: typed_array.rs
    // `array_buffer_method`, `array_buffer_slice_thunk`,
    // `shared_array_buffer_slice_thunk`). Only AllocateArrayBuffer and
    // AllocateSharedArrayBuffer make the slot: the solver's
    // BObj{ArrayBuffer | SharedArrayBuffer} (an Opaque kind, kept open below).
    let buffer = fam == Family::Opaque && matches!(what, Some("ArrayBuffer" | "SharedArrayBuffer"));
    let slot = typed || buffer;
    if !(slot
        || matches!(
            fam,
            Family::Num | Family::Bool | Family::Sym | Family::BigInt
        ))
        || m.is_marker(r)
    {
        return false;
    }
    match &m.toks[r as usize] {
        // A builtin object of an undescribed kind (`primbox`) may be one. A
        // box its creator makes an ordinary object, an array or a
        // null-prototype object (`members::box_chain`: a `JSON.parse`
        // object, a descriptor, an own-keys array) is none: only
        // TypedArrayCreate makes an object with [[TypedArrayName]] (ECMA-262
        // 10.4.5), and a later prototype change adds no internal slot.
        TokKind::Obj(tm, _) => match &m.tmpls[*tm as usize].kind {
            // A view (a typed array, a Node `Buffer`: a Uint8Array, a
            // DataView) holds [[ViewedArrayBuffer]], never [[ArrayBufferData]]
            // or [[ArrayBufferMaxByteLength]] (ECMA-262 10.4.5, 23.2.5.1.1
            // AllocateTypedArray, 25.3.2.1): a buffer method on it throws.
            TmplKind::BObj {
                what: "TypedArray" | "Buffer" | "DataView",
                ..
            } if buffer => true,
            // A box whose creator makes an ordinary object, an array or a
            // null-prototype object (`members::box_chain` known: a
            // `JSON.parse` object, ECMA-262 25.5.1 / 10.1.12
            // OrdinaryObjectCreate, 10.4.2.2 ArrayCreate) has no
            // [[NumberData]], [[BooleanData]], [[SymbolData]] or
            // [[BigIntData]] either: only the wrapper constructors and
            // ToObject make those (21.1.1.1, 20.3.1.1, 20.4.1, 21.2.1,
            // 7.1.18), so thisNumberValue & co. throw a TypeError on it.
            TmplKind::BObj { what: w, .. } if !slot => {
                members::bobj_family(w) == Family::Box
                    && members::box_chain(w) != members::BoxChain::Unknown
            }
            TmplKind::BObj { what: w, .. } => {
                slot && !matches!(*w, "TypedArray" | "Buffer")
                    && match members::bobj_family(w) {
                        Family::Opaque => false,
                        Family::Box => members::box_chain(w) != members::BoxChain::Unknown,
                        _ => true,
                    }
            }
            // An instance of a class whose chain may reach a built-in
            // constructor (`extends ArrayBuffer`, `extends Number`, an
            // `extends` value) was allocated by that constructor's
            // `super()` with its internal slots (ECMA-262 10.1.13
            // OrdinaryCreateFromConstructor in the base constructor): it may
            // carry the brand. Every other program object is ordinary.
            _ => !builtin_heritage(m, *tm),
        },
        TokKind::Proto(_) | TokKind::ObjectProto | TokKind::BuiltinFn(_) => true,
        // A function value (a function, a bound function, a class object)
        // is an ordinary function object or a bound function exotic object
        // (ECMA-262 10.2, 10.4.1): none has [[NumberData]], [[BooleanData]],
        // [[SymbolData]], [[BigIntData]], [[TypedArrayName]] or
        // [[ArrayBufferData]], so thisNumberValue & co. throw a TypeError
        // before any argument is used; the runtime agrees
        // (primitive_proto_thunks.rs `number_receiver_or_throw` 235-257: a
        // closure or class ref is no boxed Number and not Number.prototype).
        // A console `%s` / ToPrimitive lookup that finds such a native on a
        // class (three.core.js `error()`, rule CONSOLE-PCT) only throws.
        TokKind::Func(..) | TokKind::Bound(_) | TokKind::ClassObj(_) => true,
        // A built-in function object (a constructor such as `Object`, a
        // namespace method value) is made by CreateBuiltinFunction (ECMA-262
        // 10.3.4) with no [[NumberData]], [[BooleanData]], [[SymbolData]]
        // or [[BigIntData]]; a typed array or buffer slot only a non-function
        // namespace may stand for (`Number.prototype` is itself a Number
        // object, 21.1.3: no function).
        TokKind::Builtin(_) => slot || builtin_function(m, r),
        _ => false,
    }
}

/// Does template `tm` or a parent template it extends have a parent that is
/// no class of the program (a native parent, or an `extends` value), whose
/// constructor may allocate its instances?
fn builtin_heritage(m: &Model, tm: TmplId) -> bool {
    let mut at = Some(tm);
    while let Some(t) = at {
        let k = &m.tmpls[t as usize];
        if k.native_parent.is_some() || k.parent_value.is_some() {
            return true;
        }
        at = k.parent.filter(|&p| p != t);
    }
    false
}

/// The primitive kinds whose ToObject wrapper an `Array.prototype` method
/// called with that primitive as `this` runs on (`method_on_prim`).
pub(crate) const ARRAY_BOXED_PRIMS: u16 =
    prim::STR | prim::NUM | prim::BOOL | prim::SYMBOL | prim::BIGINT;

/// The `Object.prototype` methods whose steps convert `this` with
/// ToObject (ECMA-262 20.1.3.2-4, 20.1.3.6-7, B.2.2.2-5): every function
/// member but `toLocaleString` (20.1.3.5, Invoke(this, "toString") on the
/// value itself).
pub(crate) fn object_proto_method_to_object(name: &str) -> bool {
    matches!(
        name,
        "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}

/// A `fam` (`Array` or `Object`) prototype method `key` called with `this` a
/// primitive of kind `k` (one of `ARRAY_BOXED_PRIMS`) whose first use of
/// `this` is ToObject (7.1.18): it runs on a new wrapper object of `k`
/// (22.1.1.1 String exotic objects, 21.1.1.1, 20.3.1.1, 20.4.1, 21.2.1),
/// born at the call, as on any object of that family's model.
pub(crate) fn method_on_prim(m: &mut Model, fam: Family, k: u16, key: Atom, call: CallId) {
    let what = match k {
        prim::STR => "BoxedString",
        prim::NUM => "BoxedNumber",
        prim::SYMBOL => "BoxedSymbol",
        prim::BIGINT => "BoxedBigInt",
        _ => "BoxedBoolean",
    };
    let w = site_tmpl(m, call, what);
    if k == prim::STR {
        let chars = m.num_fields(w);
        m.add_prim(chars, prim::STR);
    }
    native_prototype_call(m, fam, w, key, call);
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
        Family::Error if m.atoms[key as usize] == "toString" => error_to_string(m, r, call),
        _ => own_method_inner(m, r, Key::Named(key), call),
    }
    m.cur_this = prev;
}

/// ECMA-262 20.5.3.4 `Error.prototype.toString` on object `r`:
/// `Get(O, "name")` and `Get(O, "message")` (a getter there runs with
/// `this` = `r`), each ToString'd unless undefined (ToPrimitive, hint
/// string: the value's own `@@toPrimitive`/`toString`/`valueOf` run); the
/// result is a string. Nothing else is called, nothing is kept.
pub(crate) fn error_to_string(m: &mut Model, r: Tok, call: CallId) {
    let reason = m.reason("ToString of an Error.prototype.toString operand");
    let recv = m.node_l("error-tostring-recv");
    for k in ["name", "message"] {
        let key = Key::Named(m.atom(k));
        let dst = m.node_l("error-tostring-read");
        m.cons(recv, Cons::Load { key, dst });
        m.cons(
            dst,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
                region: m.thrown,
            },
        );
    }
    m.add_tok(recv, r);
    ret_prim(m, call, prim::STR);
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
#[track_caller]
fn undescribed_member(m: &mut Model, dst: NodeId) {
    let bf = builtin_fn(m, "undescribed member");
    m.add_tok_arm(dst, bf, "undescribed_member");
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
    anyfall("undescribed intrinsic prototype member", &"intrinsic", &key);
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

/// Rule RP2-false (`Model::array_arm_may_see`): may a builtin object of kind
/// `what` be an Array object? A box whose creator allocates an ordinary or a
/// null-prototype object (`members::box_chain`) is not, nor is a value of a
/// family the model describes as no Array (a collection, an iterator, a
/// promise, an error, a plain object); an array box, an undescribed box, a
/// wrapper (`Object(v)` may be `v`) and an opaque kind may be.
pub(crate) fn bobj_may_be_array(what: &str) -> bool {
    match members::bobj_family(what) {
        Family::Box => !matches!(
            members::box_chain(what),
            members::BoxChain::Object | members::BoxChain::Null
        ),
        fam => family_may_be_array(fam),
    }
}

/// Rule RP2-false: may the builtin namespace value `ns` be an Array object
/// (`members::ns_chain`: the family of its chain)?
pub(crate) fn ns_may_be_array(ns: BuiltinNs) -> bool {
    family_may_be_array(members::ns_chain(ns))
}

fn family_may_be_array(fam: Family) -> bool {
    matches!(
        fam,
        Family::Array | Family::Box | Family::Wrapper | Family::Opaque
    )
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
        | BuiltinNs::NativeError(_)
        | BuiltinNs::AggregateError
        | BuiltinNs::Date
        | BuiltinNs::RegExp
        | BuiltinNs::TypedArray
        | BuiltinNs::TypedArrayKind(_)
        | BuiltinNs::DataView
        | BuiltinNs::ArrayBuffer
        | BuiltinNs::SharedArrayBuffer
        | BuiltinNs::Symbol
        | BuiltinNs::Number
        | BuiltinNs::BigInt
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

/// Does the realm give the builtin function value `identity` no enumerable
/// own property, so that a copy of its own enumerable properties
/// (`{...[].map}`, `Object.assign(t, Array.prototype.map)`) reads only what
/// the program put on it (`Model::own_props_modelled`)? A function the
/// specification gives a realm namespace or prototype is a built-in function
/// object (ECMA-262 10.3.4 CreateBuiltinFunction): its own `length` and
/// `name` are defined non-enumerable (10.2.9 SetFunctionName, 10.2.10
/// SetFunctionLength), as is a constructor's `prototype`. Its owner decides:
/// a function of a host object (`fs.realpath` owns an enumerable `native`)
/// keeps its own properties out of the model, unless the realm's
/// declaration of the function itself gives it none: the runtime makes a
/// function object it declares with the own properties of that
/// declaration (`RealmEntry::own`, as rule R1 reads them), so a callable
/// declaration (`parseFloat`, 19.2.5: `length` and `name`, non-enumerable)
/// whose own properties are all non-enumerable, string and symbol keys
/// alike (20.1.2.1 step 3.a.ii copies both), has none to copy.
pub(crate) fn realm_function_own_props_unenumerable(identity: &str) -> bool {
    let owner = identity.split(['.', '[']).next().unwrap_or("");
    ns_by_name(owner).is_some_and(realm_own_props_unenumerable)
        || members::realm_entry(identity).is_some_and(|e| {
            use perry_hir::realm_manifest::RealmKind as K;
            matches!(e.kind, K::Function | K::Method | K::Constructor)
                && e.own.iter().all(|p| !p.enumerable)
        })
}

/// Does the realm create a builtin object of kind `what` with no enumerable
/// own property, so that a copy of its own enumerable properties reads only
/// what the program stored on it (its fields, `Model::load_from`)? A RegExp
/// object owns only `lastIndex`, defined `[[Enumerable]]: false` (ECMA-262
/// 22.2.3.1 RegExpAlloc); a Promise owns none (27.2.3.1, 27.2.4.7: internal
/// slots only).
pub(crate) fn realm_object_own_props_unenumerable(what: &str) -> bool {
    matches!(what, "RegExp" | "Promise")
}

/// Does the realm's constructor create a builtin object of kind `what` with
/// no own property at all, only internal slots: Map, Set, WeakMap, WeakSet
/// (ECMA-262 24.1.1.1, 24.2.2.1, 24.3.1.1, 24.4.1.1), Date (21.4.2.1),
/// ArrayBuffer, SharedArrayBuffer, DataView (25.1.4.1, 25.2.3.1, 25.3.2.1),
/// WeakRef, FinalizationRegistry (26.1.1.1, 26.2.1.1)? A copy of its own
/// enumerable properties reads only what the program stored on it (its
/// fields). Only for a birth of this unit's code (`site != u32::MAX`).
pub(crate) fn realm_born_without_own_props(what: &str) -> bool {
    matches!(
        what,
        "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Date"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "WeakRef"
            | "FinalizationRegistry"
    )
}

/// Is a builtin object of kind `what` a primitive's wrapper
/// (`Family::Wrapper`: `Object(x)` and `new Object(x)` (ECMA-262 20.1.1.1),
/// the ToObject of `Object.assign`'s target (20.1.2.1, 7.1.18), `new
/// Number/String/Boolean(x)` (21.1.1.1, 22.1.1.1, 20.3.1.1), a method's
/// ToObject(this))? It is born with no own property but a String
/// wrapper's indices and its non-enumerable `length` (10.4.3.3
/// StringCreate); for a non-primitive argument `Object(x)` is `x` itself,
/// a value of its own, and for undefined or null a new ordinary object with
/// none. A copy of its own enumerable properties reads what the program
/// stored on it (its fields) and a String wrapper's indices
/// (`wrapper_may_wrap_string`). Only for a birth of this unit's code.
pub(crate) fn wrapper_own_props_are_stores(what: &str) -> bool {
    members::bobj_family(what) == Family::Wrapper
}

/// May a wrapper of kind `what` wrap a String (`wrapper_own_props_are_stores`):
/// its own index properties are the characters of the string (10.4.3.5).
pub(crate) fn wrapper_may_wrap_string(what: &str) -> bool {
    matches!(what, "BoxedString" | "Object()" | "Object(prim)")
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
        TokKind::NativeProto(..) => Some(Family::Object),
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
                what: "TypedArray" | "Buffer",
                ..
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
        TokKind::NativeProto(_, ns)
            if matches!(
                ns,
                Some(BuiltinNs::TypedArray | BuiltinNs::TypedArrayKind(_))
            ) =>
        {
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

/// F-B: a builtin function value of the realm (its identity rooted at a
/// language namespace or constructor of `enum_free_ns`: `Object.prototype.
/// valueOf`, `Math.max`, `%TypedArray%.prototype.indexOf`) has no
/// enumerable built-in own property: `length` and `name` are made by
/// SetFunctionLength / SetFunctionName with [[Enumerable]] false (ECMA-262
/// 10.3.4 CreateBuiltinFunction), a constructor's `prototype` and statics
/// are non-enumerable data (clause 18: "every other data property ...
/// [[Enumerable]]: false"). A for-in at such a link visits only what the
/// program wrote on it. A host function is not one (`fs.realpath.native`
/// is enumerable data): its root is a module, not a realm namespace.
pub(crate) fn enum_free_fn(identity: &str) -> bool {
    let root = identity.split('.').next().unwrap_or(identity);
    root != identity && ns_by_name(root).is_some_and(enum_free_ns)
}

/// `dst` ⊇ builtin `ns`'s prototype object as a value (`F.prototype`;
/// `Object.getPrototypeOf(x)` of a builtin receiver `x`, `chain_ns`): the
/// collapsed `ObjectProto`, which every builtin receiver's chain reads (so
/// a write through the value stays a write of it, as a write of any
/// builtin prototype is), and the builtin's own token
/// (`builtin_proto_tok`), so that an object whose chain the value becomes
/// reads the builtin's members and accessors.
/// ENUM1: a constructor, namespace or prototype of the language none of
/// whose built-in properties is enumerable. Not a host object (`console`,
/// `process`, the global object, `require`, a module's exports: their
/// members are enumerable), not the Error constructors (V8's own
/// `Error.stackTraceLimit` is enumerable).
pub(crate) fn enum_free_ns(ns: BuiltinNs) -> bool {
    use BuiltinNs::*;
    matches!(
        ns,
        Object
            | Reflect
            | Array
            | Json
            | Math
            | Promise
            | Date
            | RegExp
            | TypedArray
            | TypedArrayKind(_)
            | DataView
            | ArrayBuffer
            | SharedArrayBuffer
            | Symbol
            | Number
            | BigInt
            | String
            | Boolean
            | Function
            | Map
            | Set
            | WeakMap
            | WeakSet
            | ObjectProtoMethod
            | ObjectProtoOwn(_)
    )
}

/// ENUM2: the collapsed `ObjectProto` that is the prototype of chain link
/// `from` is an intrinsic prototype none of whose built-in properties is
/// enumerable: `from` is an `enum_free_ns` builtin (its prototype is
/// `Object.prototype`, `Function.prototype` or `%TypedArray%.prototype`), or
/// `from`'s kind seeds its chain with `Object.prototype`, `Array.prototype`
/// or `Function.prototype` (`seed_family`). Not an object a builtin made
/// other than an ordinary box (a host object: `Buffer.prototype`,
/// `URL.prototype` hold enumerable members), not a generator's step closure
/// (its chain holds what `f.prototype` holds). A prototype given to `from`
/// that may be `ObjectProto` puts ⊤ on the chain (`Cons::ProtoGiven`).
pub(crate) fn enum_free_seed(m: &Model, from: Tok) -> bool {
    match &m.toks[from as usize] {
        TokKind::Builtin(_) | TokKind::NativeProto(..) => m.enum_free(from),
        TokKind::Obj(tm, _) if matches!(m.tmpls[*tm as usize].kind, TmplKind::BObj { .. }) => {
            ordinary_box(m, from) || enum_free_link_obj(m, from)
        }
        TokKind::Func(f, _) if m.funcs[*f as usize].gen_step() => false,
        _ => matches!(
            seed_family(m, from),
            Some(Family::Object | Family::Array | Family::Function)
        ),
    }
}

/// ENUM2: what a store into ⊤ may have written under any key onto a
/// built-in link of an enumerable-key read of the chain of `of`: what a
/// computed read of that chain finds (`family_any`'s `any_written`).
pub(crate) fn enum_top_written_any(m: &mut Model, of: Tok, dst: NodeId) {
    let star = m.atom(crate::STAR_KEY);
    if !locked_chain_read(m, of, star) {
        any_written(m, dst);
    }
}

/// ENUM1: what a store into ⊤ may have written under named key `a` onto a
/// built-in object an enumerable-key read reads (`Model::enum_read_tok`):
/// the opaque answer once the key is written there, a replay before.
pub(crate) fn enum_top_written(m: &mut Model, a: Atom, dst: NodeId) {
    if !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

pub(crate) fn builtin_proto_value(m: &mut Model, ns: Option<BuiltinNs>, dst: NodeId) {
    // R9READ11: the value is the builtin's own prototype object only
    // (ECMA-262 clause 18: `Array.prototype` is %Array.prototype%, whose
    // [[Prototype]] is %Object.prototype%; it is not %Object.prototype%).
    // A read of a key on it reads the builtin's described members, as a
    // receiver of its family reads them (`load_builtin`'s `NativeProto`
    // arm); every other operation on it is also the collapsed
    // builtin-prototype operation (`Model::native_proto_alias`), so a
    // write through the value stays a write of `ObjectProto`, as before.
    // `ObjectProto` alone is the exact `Object.prototype`; a prototype the
    // model does not describe is that and a native prototype of its own
    // (`loads::family_proto_value`), so its reads stay apart from
    // `Object.prototype`'s.
    match ns.and_then(|ns| builtin_proto_tok(m, ns)) {
        Some(p) => m.add_tok(dst, p),
        None => {
            let op = m.tok(TokKind::ObjectProto);
            m.add_tok(dst, op);
            if let Some(ns) = ns.filter(|ns| *ns != BuiltinNs::Object) {
                let name = format!("{ns:?}.prototype");
                let p = native_proto(m, &name);
                m.add_tok(dst, p);
            }
        }
    }
}

/// An ordinary object a builtin makes (`members::box_chain` Object: a
/// `JSON.parse` object, a property descriptor, an `Object.fromEntries`
/// result): its chain is `Object.prototype`, or a prototype the program
/// gave it, never another builtin prototype.
pub(crate) fn ordinary_box(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what, .. }
            if members::bobj_family(what) == Family::Box
                && members::box_chain(what) == members::BoxChain::Object))
}

/// ENUM1: an instance of a language builtin none of whose properties is
/// enumerable: a `Map`, `Set`, `WeakMap`, `WeakSet` or `Promise` (ECMA-262
/// 24.1.1.1, 24.2.2.1, 24.3.1.1, 24.4.1.1, 27.2.3.1: OrdinaryCreateFromConstructor
/// with internal slots only, no own property; the properties of its
/// prototype are non-enumerable, clause 18). A for-in over it visits only
/// keys the program wrote on it or its chain, so `o[k]` of that for-in
/// reads no builtin member. Not an iterator, an Error (the V8 `stack` own
/// property), a typed array (its indices are enumerable) or a host object.
pub(crate) fn enum_free_instance(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what, .. }
            if matches!(members::bobj_family(what), Family::Collection | Family::Promise)))
}

/// SKINROOT7: a built-in instance on a chain an enumerable-key read walks
/// whose enumerable own properties are what the program wrote on it, plus,
/// for a typed array, its elements (the EnumChain arm adds the element
/// primitive), and whose built-in prototype holds no enumerable built-in
/// property (ECMA-262 clause 18: "every other data property ...
/// [[Enumerable]]: false"; accessors alike):
/// - `ArrayBuffer`, `SharedArrayBuffer`, `DataView`: no own property
///   (25.1.3.1 AllocateArrayBuffer, 25.2.2.1 AllocateSharedArrayBuffer,
///   25.3.2.1 DataView: OrdinaryCreateFromConstructor + internal slots);
/// - `TypedArray`: own properties are the integer-indexed elements
///   (10.4.5.1 [[GetOwnProperty]]: {[[Enumerable]]: true}, a Number or
///   BigInt, 10.4.5.15 TypedArrayGetElement), nothing else (23.2.5.1);
/// - the `enum_free_instance` kinds (Map, Set, WeakMap, WeakSet, Promise).
/// Not a host `Buffer` (Node's `Buffer.prototype` members are enumerable),
/// not an Error, not a Date or RegExp (their prototype link is a
/// `NativeProto` that `enum_free_ns` does not name), not a marker (it stands for a caller's value). A prototype
/// the program gave the instance is the next link (`proto_node`); a
/// program-made member of the built-in prototype is `enum_top_written`.
pub(crate) fn enum_free_link_obj(m: &Model, t: Tok) -> bool {
    !m.is_marker(t)
        && (enum_free_instance(m, t)
            || matches!(m.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj {
                what: "ArrayBuffer" | "SharedArrayBuffer" | "DataView" | "TypedArray",
                ..
            })))
}

/// SKINROOT14: a for-in subject (`for (k in b) b[k]`) that is an
/// `ArrayBuffer`, `SharedArrayBuffer` or `DataView` has no own property
/// (ECMA-262 25.1.3.1 AllocateArrayBuffer, 25.2.2.1, 25.3.2.1:
/// OrdinaryCreateFromConstructor with internal slots only) and the members
/// of its prototype are non-enumerable (clause 18): `k` is a key the
/// program wrote on `b` or its chain (14.7.5.9). A typed array is not one
/// (its indices are enumerable own properties); not a marker.
pub(crate) fn enum_free_buffer(m: &Model, t: Tok) -> bool {
    !m.is_marker(t)
        && matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj {
            what: "ArrayBuffer" | "SharedArrayBuffer" | "DataView",
            ..
        }))
}

fn is_site_builtin(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { .. }))
}

/// Is `t` an array: an array literal or result (`TmplKind::Arr`), or an
/// array a builtin makes (`members::box_chain` Array: `Object.keys`, the
/// `arraybox` results)? Its chain is `%Array.prototype%` ->
/// `%Object.prototype%`, or a prototype the program gave it
/// (`Model::load_from` reads that one on the chain).
pub(crate) fn array_chain(m: &Model, t: Tok) -> bool {
    matches!(m.toks[t as usize], TokKind::Obj(tm, _)
    if match &m.tmpls[tm as usize].kind {
        TmplKind::Arr { .. } => true,
        TmplKind::BObj { what, .. } => members::bobj_family(what) == Family::Box
            && members::box_chain(what) == members::BoxChain::Array,
        _ => false,
    })
}

/// `dst` ⊇ what computed key `key` reads on family `fam`'s intrinsic chain
/// (of a value of kind `what`): the member under every name of the chain
/// the key may spell, as its named read finds it (`loads::family_member`:
/// the exact natives, `constructor`, data kinds), the methods of
/// `Object.prototype` the family does not shadow
/// (`BuiltinNs::ObjectProtoMethod`), and what a store into ⊤ wrote. Not
/// `__proto__`: the prototype would be `ObjectProto`, which stands for
/// every builtin prototype, as a value (an open gap, as before). Where
/// the chain of `of` (a primitive's: `None`) is final under any key
/// (`locked_chain_read` of `STAR_KEY`, DEC 250), only `constructor`, which
/// the lock exempts, may have been written there. A family with no member
/// table (Box, Wrapper, Opaque) reads an undescribed builtin function.
pub(super) fn family_any(
    m: &mut Model,
    of: Option<Tok>,
    fam: Family,
    what: Option<&'static str>,
    key: Key,
    dst: NodeId,
) {
    match members::chain_names(fam) {
        Some(_) => {
            let n = chain_any_node(m, fam, what, key);
            m.edge(n, dst);
            // An accessor's getter runs on the value read (`of`), as its
            // named read does: its result depends on the receiver, so it
            // is no part of the shared chain node.
            for a in chain_accessors(m, fam, what, key) {
                loads::family_member(m, of, fam, what, a, dst);
            }
        }
        None => {
            family_ctor(m, fam, dst);
            let bf = builtin_fn(m, "computed key on a described chain");
            m.add_tok(dst, bf);
            m.add_prim(dst, prim::ALL);
            let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoMethod));
            m.add_tok(dst, f);
        }
    }
    m.add_prim(dst, prim::UNDEF);
    let star = m.atom(crate::STAR_KEY);
    let locked = match of {
        Some(of) => locked_chain_read(m, of, star),
        None => members::locked_read(fam, what, crate::STAR_KEY),
    };
    if !locked {
        return any_written(m, dst);
    }
    if !m.key_may_name_str(key, "constructor") {
        return;
    }
    let ctor = m.atom("constructor");
    if !intrinsic_miss(m, ctor, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

/// A computed read of a builtin prototype may find any key a store into ⊤
/// wrote, now or later in the solve (`native_written` of `STAR_KEY`).
/// The node of family `fam`'s intrinsic chain (of a value of kind `what`)
/// under computed key `key`: the member under every name of the chain the
/// key may spell, as its named read finds it (`loads::family_member`), and
/// the methods of `Object.prototype` the family does not shadow. It depends
/// on the chain and the key only, never on the value read: one node, filled
/// once, every read of every such value an edge from it.
fn chain_any_node(m: &mut Model, fam: Family, what: Option<&'static str>, key: Key) -> NodeId {
    if let Some(&n) = m.chain_any_nodes.get(&(fam, what, key)) {
        return n;
    }
    let n = m.node_l("builtin-chain-any");
    m.chain_any_nodes.insert((fam, what, key), n);
    let mut object_method = false;
    for name in members::chain_names(fam).into_iter().flatten() {
        if Model::proto_link(name) || !m.key_may_name_str(key, name) {
            continue;
        }
        if members::own(fam, name).is_none()
            && matches!(members::member(Family::Object, name), Some(Mem::Fn))
        {
            object_method = true;
            continue;
        }
        if matches!(members::member(fam, name), Some(Mem::Fn)) && realm_accessor(m, fam, what, name)
        {
            continue;
        }
        let a = m.atom(name);
        loads::family_member(m, None, fam, what, a, n);
    }
    if object_method {
        let f = m.tok(TokKind::Builtin(BuiltinNs::ObjectProtoMethod));
        m.add_tok(n, f);
    }
    n
}

/// The accessors of family `fam`'s intrinsic chain (of a value of kind
/// `what`) whose names computed key `key` may spell: their getters run per
/// receiver (`family_any`), never in the shared `chain_any_node`.
fn chain_accessors(m: &mut Model, fam: Family, what: Option<&'static str>, key: Key) -> Vec<Atom> {
    let mut out = vec![];
    for name in members::chain_names(fam).into_iter().flatten() {
        if Model::proto_link(name) || !m.key_may_name_str(key, name) {
            continue;
        }
        if members::own(fam, name).is_none()
            && matches!(members::member(Family::Object, name), Some(Mem::Fn))
        {
            continue;
        }
        if matches!(members::member(fam, name), Some(Mem::Fn)) && realm_accessor(m, fam, what, name)
        {
            out.push(m.atom(name));
        }
    }
    out
}

/// The OPTIMISTIC JSON-key read (`Model::json_key_optimistic`) of the
/// intrinsic chain of `t`: no member, no `constructor`; only what a store
/// into ⊤ may have written there (`family_any`'s unlocked case).
pub(crate) fn chain_written_only(m: &mut Model, t: Tok, dst: NodeId) {
    let star = m.atom(crate::STAR_KEY);
    if !locked_chain_read(m, t, star) {
        any_written(m, dst);
    }
}

fn any_written(m: &mut Model, dst: NodeId) {
    let star = m.atom(crate::STAR_KEY);
    if !intrinsic_miss(m, star, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
}

mod accessor_lookup;
mod methods;
pub(crate) mod object_statics;
pub(crate) use accessor_lookup::{
    annexb_lookup, annexb_lookup_prim, is_realm_accessor, lookup_native_descriptor,
    realm_accessor_call,
};
pub use methods::*;
pub(crate) use methods::{accessor_callable, accessor_receiver, annexb_define};
/// Whether the generated global object declares a callable under this key.
/// The objects `JSON.parse` makes at `call`: ordinary objects and arrays
/// (`members::box_chain`), as `Gen::json_value` models them, whose fields
/// and elements are such objects or primitives.
///
/// `positions`: the objects are position classes (`solve::json_pos`); a
/// reviver parse keeps one object token for the whole tree.
/// Without a reviver the primitives are strings, numbers, booleans and
/// null (25.5.1 ParseJSON: JSON text has no other value); a reviver's
/// results are any value (25.5.1.1 InternalizeJSONProperty).
pub(crate) fn json_value(
    m: &mut Model,
    call: CallId,
    positions: bool,
    reviver: bool,
) -> (Tok, Tok) {
    let j = site_tmpl(m, call, "json");
    let ja = site_tmpl(m, call, "Array");
    if positions {
        m.json_root(j, ja);
        return (j, ja);
    }
    let vals = if reviver { prim::ALL } else { prim::JSON };
    // Without a reviver the result `j` / `ja` is no value inside its own
    // tree (`Model::json_inner`): every nested object or array is `io` /
    // `ia`. A reviver runs with every object of the tree as its `this` and
    // its results replace values anywhere (25.5.1.1
    // InternalizeJSONProperty): one token for the whole tree.
    let (io, ia) = match reviver {
        true => (j, ja),
        false => m.json_inner(j, ja),
    };
    for o in [j, ja, io, ia] {
        let f = m.field_any(o);
        m.add_prim(f, vals);
        m.add_tok(f, io);
        m.add_tok(f, ia);
    }
    (j, ja)
}

pub(crate) fn global_function(m: &Model, name: &str) -> bool {
    m.namespace_property(BuiltinNs::GlobalThis, name)
        .is_some_and(|p| p.native.is_some())
}

/// [[Set]] of `key` on the declared holder `t` meets the realm's accessor
/// with a getter and no setter (`navigator` on the global object, Node 26:
/// `get navigator`, no `set`): no setter runs and nothing is written; strict
/// code throws a TypeError, the runtime error every `catch` already receives
/// (`Model::thrown`).
pub(crate) fn getter_only(m: &Model, t: Tok, key: &str) -> bool {
    m.native_property(t, key)
        .is_some_and(|p| p.get.is_some() && p.set.is_none())
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
    if opaque_written(m, holder, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    } else {
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
        TokKind::BuiltinFn(Native::Member { identity, .. })
            if name == "prototype" && realm_ctor_prototype(identity).is_some() =>
        {
            // ECMA-262 10.2.5 MakeConstructor: a realm constructor's own
            // `prototype` is the realm prototype it declares
            // (`EventTarget.prototype`), a value of its own whose lookups
            // read that declaration's chain (`loads::tok_load`).
            let born = realm_ctor_prototype(identity).expect("guarded");
            if opaque_written(m, holder, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            } else {
                let p = realm_proto_value(m, born);
                m.add_tok(dst, p);
            }
            return true;
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
            if opaque_written(m, holder, a, NativeMiss::Load(dst)) {
                opaque_member(m, dst);
            } else {
                if let Some(identity) = value {
                    let value = m.tok(TokKind::BuiltinFn(Native::Member {
                        identity,
                        holder: None,
                    }));
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

/// The realm prototype realm constructor `identity` holds as its own
/// `prototype` (a reference the realm declares) when the realm declares
/// every link of that prototype's chain (`members::realm_born`).
pub(crate) fn realm_ctor_prototype(
    identity: &str,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    use perry_hir::realm_manifest::{RealmKind, RealmOwnValue};
    let e = members::realm_entry(identity).filter(|e| e.kind == RealmKind::Constructor)?;
    let p = e.own.iter().find(|p| !p.symbol && p.key == "prototype")?;
    let RealmOwnValue::Reference(r) = p.value else {
        return None;
    };
    members::realm_born(r)
}

/// The token of realm prototype `born` taken as a value: a native
/// prototype under its declaration's name. Its declared accessors run per
/// receiver (`realm_proto_getter`), so their keys are accessor keys
/// (`Model::add_accessor_atom`: the lookups walk their holders).
pub(crate) fn realm_proto_value(
    m: &mut Model,
    born: &'static perry_hir::realm_manifest::RealmEntry,
) -> Tok {
    // One object, one token: the name a builtin object of the kind reads
    // its prototype under (`loads` proto of `what`, `native_chain`), the
    // constructor's (`URL` for `URL.prototype`).
    let p = native_proto(m, born.name.strip_suffix(".prototype").unwrap_or(born.name));
    let mut at = Some(born);
    while let Some(e) = at {
        for o in e.own.iter() {
            if realm_chain_getter(born, o).is_some() || realm_chain_setter(born, o).is_some() {
                let n = match (o.symbol, o.key.strip_prefix("@@")) {
                    (true, Some(wk)) => format!("{WK_PREFIX}{wk}"),
                    (true, None) => continue,
                    (false, _) => o.key.to_string(),
                };
                let a = m.atom(&n);
                m.add_accessor_atom(a);
            }
        }
        at = e.proto.and_then(members::realm_entry);
    }
    p
}

/// The realm declaration of native prototype token `t` when it is a
/// realm-born prototype (`realm_proto_value`).
pub(crate) fn realm_proto_of(
    m: &Model,
    t: Tok,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    match &m.toks[t as usize] {
        TokKind::NativeProto(n, None) => realm_proto_named(n),
        _ => None,
    }
}

/// The realm prototype a builtin kind `what` (`URL`, or a declaration's
/// `fresh` kind `EventTarget.prototype`) reads its chain from, when the
/// realm declares every link of it (`members::realm_born`).
pub(crate) fn realm_proto_named(
    what: &str,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    let base = what.strip_suffix(".prototype").unwrap_or(what);
    members::realm_born(&format!("{base}.prototype"))
}

/// The declared getter of own property `o` when the lookup of its key on
/// `born`'s chain finds `o` (an accessor whose getter the realm declares
/// effects for).
fn realm_chain_getter(
    born: &'static perry_hir::realm_manifest::RealmEntry,
    o: &perry_hir::realm_manifest::RealmOwn,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    use perry_hir::realm_manifest::RealmOwnValue;
    let RealmOwnValue::Accessor { get: Some(g), .. } = o.value else {
        return None;
    };
    let name = match (o.symbol, o.key.strip_prefix("@@")) {
        (true, Some(wk)) => format!("{WK_PREFIX}{wk}"),
        (true, None) => return None,
        (false, _) => o.key.to_string(),
    };
    match members::realm_chain_find(born, &name) {
        members::RealmFind::Own(p, _) if std::ptr::eq(p, o) => {
            members::realm_entry(g).filter(|e| !e.effects.is_empty())
        }
        _ => None,
    }
}

/// What a store through own accessor `o` of `born`'s chain runs when the
/// lookup of its key finds `o` (ECMA-262 10.1.9.2 OrdinarySetWithOwnDescriptor
/// steps 3-7): its setter. `Object.prototype`'s (`__proto__`) is not one:
/// that object is its own token on every chain, whose accessor the Annex B
/// lookups run (`accessor_lookup`).
fn realm_chain_setter(
    born: &'static perry_hir::realm_manifest::RealmEntry,
    o: &perry_hir::realm_manifest::RealmOwn,
) -> Option<RealmSetter> {
    use perry_hir::realm_manifest::RealmOwnValue;
    let RealmOwnValue::Accessor { set, .. } = o.value else {
        return None;
    };
    let name = match (o.symbol, o.key.strip_prefix("@@")) {
        (true, Some(wk)) => format!("{WK_PREFIX}{wk}"),
        (true, None) => return None,
        (false, _) => o.key.to_string(),
    };
    let members::RealmFind::Own(p, owner) = members::realm_chain_find(born, &name) else {
        return None;
    };
    if !std::ptr::eq(p, o) || owner.name == "Object.prototype" {
        return None;
    }
    let Some(set) = set else {
        return Some(RealmSetter::GetterOnly);
    };
    Some(
        match members::realm_entry(set)
            .filter(|e| !e.effects.is_empty() && effects::interpretable(e.effects))
        {
            Some(e) => RealmSetter::Declared(e),
            None => RealmSetter::Undeclared(format!("{}.{name}", owner.name)),
        },
    )
}

/// The setter a store finds on a realm prototype's chain.
pub(crate) enum RealmSetter {
    /// Declared effects: they run with the receiver as `this` and the
    /// stored value as the argument (`realm_setter_store`).
    Declared(&'static perry_hir::realm_manifest::RealmEntry),
    /// No setter (a getter-only accessor): OrdinarySetWithOwnDescriptor
    /// (10.1.9.2 step 7.b) returns false; strict code's PutValue throws a
    /// TypeError (13.15.2 step 5.d), the runtime error every `catch` already
    /// receives (`Model::thrown`), sloppy code does nothing. Either way
    /// nothing is written and the stored value goes nowhere.
    GetterOnly,
    /// A setter without declared effects: the accessor's name.
    Undeclared(String),
}

/// The setter of key `a` on the chain of realm prototype token `holder`.
pub(crate) fn realm_proto_setter(m: &Model, holder: Tok, a: Atom) -> Option<RealmSetter> {
    let born = realm_proto_of(m, holder)?;
    let members::RealmFind::Own(p, _) = members::realm_chain_find(born, &m.atoms[a as usize])
    else {
        return None;
    };
    realm_chain_setter(born, p)
}

/// The setter a store of key `name` finds on the chain of realm prototype
/// `born` (`realm_chain_setter`).
pub(crate) fn realm_born_setter(
    born: &'static perry_hir::realm_manifest::RealmEntry,
    name: &str,
) -> Option<RealmSetter> {
    let members::RealmFind::Own(p, _) = members::realm_chain_find(born, name) else {
        return None;
    };
    realm_chain_setter(born, p)
}

/// The keys of `holder`'s realm chain a store finds a realm setter under
/// (`realm_proto_setter`), as atoms.
pub(crate) fn realm_proto_setter_keys(m: &mut Model, holder: Tok) -> Vec<Atom> {
    let Some(born) = realm_proto_of(m, holder) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in members::realm_chain_keys(born) {
        let a = m.atom(&k);
        if realm_proto_setter(m, holder, a).is_some() {
            out.push(a);
        }
    }
    out
}

/// Setter `e` runs on receiver `origin` with `src` as its argument.
pub(crate) fn realm_setter_store(
    m: &mut Model,
    e: &'static perry_hir::realm_manifest::RealmEntry,
    origin: Tok,
    a: Atom,
    src: NodeId,
) {
    effects::setter_run(m, e, origin, a, src);
}

/// The getter the realm declares for key `a` on the chain of realm
/// prototype token `holder`, which runs with each receiver of the lookup
/// as `this` (ECMA-262 10.1.8.1 OrdinaryGet step 7), never with the
/// prototype's shared chain value (`loads::tok_load` leaves it out).
pub(crate) fn realm_proto_getter(
    m: &Model,
    holder: Tok,
    a: Atom,
) -> Option<&'static perry_hir::realm_manifest::RealmEntry> {
    let born = realm_proto_of(m, holder)?;
    let members::RealmFind::Own(p, _) = members::realm_chain_find(born, &m.atoms[a as usize])
    else {
        return None;
    };
    realm_chain_getter(born, p)
}

/// `dst` ⊇ what getter `e` returns on receiver `origin`.
pub(crate) fn realm_getter_load(
    m: &mut Model,
    e: &'static perry_hir::realm_manifest::RealmEntry,
    origin: Tok,
    a: Atom,
    dst: NodeId,
) {
    effects::getter_load(m, e, origin, a, dst);
}

/// Constructibility is the realm declaration's object shape, independent
/// of whether this solver has a specialized constructor call model.
pub(crate) fn native_constructible(m: &Model, t: Tok) -> bool {
    m.native_coordinates(t).is_some_and(|c| c.constructible)
}

/// Does builtin namespace object `t` hold `a` as an own property
/// (ECMA-262 10.1.8.1 OrdinaryGet: a lookup ends at it, its prototype is
/// not read)? A static the realm declares on the object itself
/// (`native_property`, data or accessor) or the namespace's own static
/// table (`Promise.all`, 27.2.4.1), not its chain's member. A store that
/// may replace or delete it is the namespace's own written key
/// (`native_written` of `t`), which `ns_load` replays. The global object
/// is not one: its own keys are the program's globals (`global_value_written`).
pub(crate) fn namespace_owns(m: &Model, t: Tok, a: Atom) -> bool {
    let TokKind::Builtin(ns) = m.toks[t as usize] else {
        return false;
    };
    if m.unit_mode || global_object(m, t) {
        return false;
    }
    let name = m.atoms[a as usize].as_str();
    m.native_property(t, name).is_some() || members::ns_own_member(ns, name)
}

/// Read the function reference from this receiver's declared property shape.
pub(crate) fn namespace_native(m: &Model, t: Tok, ns: BuiltinNs, name: &str) -> Option<Native> {
    if let Some(identity) = m.native_property(t, name).and_then(|p| p.native) {
        return Some(Native::Member {
            identity,
            holder: None,
        });
    }
    matches!(members::ns_member(ns, name), Some(Some(Mem::Fn)))
        .then(|| member_native(m, members::ns_chain(ns), None, name))
        .flatten()
}

/// REALM-PRESENT: is key `a` of builtin object `t` a property the realm
/// creates (ECMA-262 9.1.1.4, 19.1-19.4: the global object's own
/// properties, its constructors and namespaces; 21.3.2 and the like: a
/// namespace's or constructor's own function properties), whose read
/// `tok_load` answers with the realm's value, and with every primitive and
/// ⊤ once a write or delete may reach the key? Then the read is never
/// `undefined` by absence. The unknown-code half is the model's write
/// record: for a global, `global_value_written` (`native_protocol::
/// written_by`: an unmodeled writer, an accessor, a write under any key,
/// the global object in ⊤) with its replay `NativeMiss::Global`
/// (`loads::declared_global_load`), and a program store is in the
/// global's field, which the read takes; for a namespace member,
/// `opaque_written` (`native_written`) with its replay `NativeMiss::Load`
/// (`loads::ns_load`). A unit sees no other unit's writes
/// (`declared_global_load` tracks none in unit mode): never there.
pub(crate) fn declared_present(m: &Model, t: Tok, a: Atom) -> bool {
    if m.unit_mode {
        return false;
    }
    let n = m.atoms[a as usize].as_str();
    match m.toks[t as usize] {
        TokKind::Builtin(BuiltinNs::Other) => ns_by_name(n).is_some(),
        TokKind::Builtin(BuiltinNs::GlobalThis) => {
            !global_user_key(m, n) && ns_by_name(n).is_some()
        }
        TokKind::Builtin(
            BuiltinNs::Require
            | BuiltinNs::Process
            | BuiltinNs::Module(_)
            | BuiltinNs::ObjectProtoMethod
            | BuiltinNs::ObjectProtoOwn(_),
        ) => false,
        TokKind::Builtin(ns) => {
            !n.starts_with("@@")
                && n != "prototype"
                && n != "constructor"
                && namespace_native(m, t, ns, n).is_some()
                && !namespace_receiver_getter(m, t, n)
        }
        _ => false,
    }
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
        m.with_native(call, |m, call| {
            ns_method_inner(m, ns, Some(name), call, true)
        });
        m.cur_this = prev;
    }
    true
}

/// The host object a member access on a token of `ns` under `name` reads:
/// a member of `console` or a stdio stream the host owns, and `console` for
/// a call of one of its own methods on the global sentinel (`console.log(x)`
/// lowers to a call on `GlobalGet(0)`; the console lowering takes the names
/// `perry_hir::method_call::console_method_name` accepts). A key the host's
/// description places on its chain (`console.toString`, `Object.prototype`'s)
/// or declares absent (`console[Symbol.toPrimitive]`) is no host member: it
/// reads as on any described namespace. A stream key its description does
/// not list may be on its undeclared TTY layout: the host's.
pub(crate) fn host_of(ns: BuiltinNs, name: &str, call: bool) -> Option<BuiltinNs> {
    match ns {
        BuiltinNs::Other
            if call
                && perry_hir::method_call::console_method_name(name)
                && members::ns_own_fn(BuiltinNs::Console, name) =>
        {
            Some(BuiltinNs::Console)
        }
        ns if members::is_host(ns)
            && (members::ns_own_member(ns, name) || members::ns_member(ns, name).is_none()) =>
        {
            Some(ns)
        }
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

/// BNSRECV: the site verdict's half of `loads::ns_load` on a builtin
/// namespace or constructor that is no host object. Key `a` of namespace
/// token `t` reads the realm's own value (ECMA-262 9.3.2 CreateIntrinsics:
/// an own property the realm gives the object): its native function
/// (`namespace_native`), a described primitive, or for `prototype` the
/// family's prototype object, and no write the solve sees reached the key
/// (`native_written`: a store of the key, a computed-key store, a write
/// through ⊤). No namespace object is a lock holder (perry-abi lock.rs
/// `LockHolder`): the runtime admits every replacement, so the caller adds
/// that no unknown or interpreted code runs. Whole-program solves only.
pub(crate) fn ns_native(m: &Model, t: Tok, ns: BuiltinNs, a: Atom, call: bool) -> bool {
    if m.unit_mode || members::is_host(ns) {
        return false;
    }
    let name = &m.atoms[a as usize];
    if m.native_property(t, name).is_some_and(|p| p.get.is_some()) {
        return false;
    }
    let own = match members::ns_member(ns, name) {
        Some(Some(Mem::Fn)) => namespace_native(m, t, ns, name).is_some(),
        Some(Some(Mem::Prim(_))) => !call,
        Some(Some(Mem::Proto)) => !call && name == "prototype",
        _ => false,
    };
    own && !native_written(m, t, a)
}

/// R9READ9: named key `a` is absent on builtin function value `t` and its
/// whole chain: a described language constructor or namespace
/// (`members::ns_member`: neither its own statics nor its chain,
/// `Function.prototype` / `Object.prototype`, list the key), or a builtin
/// method value (its own `length` and `name`, 10.3.4 CreateBuiltinFunction;
/// no `prototype`; its chain `Function.prototype` then `Object.prototype`,
/// `members::member(Function, _)`). No accessor and no write the solve sees
/// reached the key on `t` (its own field values; `native_written`: a
/// named or computed store, a write through ⊤). The caller adds the chain's own writes
/// (`builtin_protos_unwritten`, `object_proto_unwritten`). Unknown code
/// holding the realm or interpreted code may add the key: refused.
/// 10.1.8.1 OrdinaryGet: `undefined`. Whole-program solves only.
pub(crate) fn builtin_value_absent(m: &Model, t: Tok, a: Atom) -> bool {
    if m.unit_mode || m.realm_held || m.interp_runs || m.is_private_atom(a) {
        return false;
    }
    let name = m.atoms[a as usize].as_str();
    let absent = builtin_described_absent(m, t, name);
    // The solve's own value of the key on `t`: a store onto the function
    // object itself (a named store, a computed one, `__proto__`).
    absent && m.native_property(t, name).is_none() && builtin_value_untouched(m, t, a)
}

/// R9READ10: is `a` the own `length` of the builtin function value `t` (a
/// builtin constructor or namespace whose described chain is a function's,
/// or a builtin method value), untouched by the program? Every builtin
/// function has an own `length` data property, a Number (20.2.4.1, 10.2.9
/// SetFunctionLength: { [[Writable]]: false, [[Enumerable]]: false,
/// [[Configurable]]: true }). Being configurable, `delete` and
/// `Object.defineProperty` may remove or replace it: either marks the token
/// hard (E7Delete / E5DefineProperty), an accessor lands in its accessor
/// keys. A plain store cannot change it (10.1.9.2 OrdinarySet step 2.a:
/// non-writable: false; PutValue throws in strict code): the model still
/// refuses any store that reaches the key on the token. Unknown code
/// holding the realm and interpreted code refuse.
pub(crate) fn builtin_fn_own_length(m: &Model, t: Tok, a: Atom) -> bool {
    if m.unit_mode || m.realm_held || m.interp_runs || m.is_private_atom(a) {
        return false;
    }
    let name = m.atoms[a as usize].as_str();
    if name != "length" {
        return false;
    }
    let present = match &m.toks[t as usize] {
        TokKind::Builtin(ns) => {
            !members::is_host(*ns)
                && matches!(
                    members::ns_member(*ns, name),
                    Some(Some(Mem::Prim(p))) if p == crate::model::prim::NUM
                )
        }
        TokKind::BuiltinFn(Native::Member { .. }) => matches!(
            members::member(Family::Function, name),
            Some(Mem::Prim(p)) if p == crate::model::prim::NUM
        ),
        _ => false,
    };
    // The realm declaration of the own `length` (`native_property`) is that
    // same property: it does not refuse here.
    present && builtin_value_untouched(m, t, a)
}

/// R9READ11: is `a` a method of `Function.prototype` (`call`, `apply`,
/// `bind`, ...: `members::member(Function, a)` a builtin function) that
/// the builtin method value `t` reads off its chain, untouched by the
/// program? A builtin method value is a builtin function object whose
/// [[Prototype]] is %Function.prototype% (10.3.4 CreateBuiltinFunction
/// step 6) with only `length` and `name` own (10.2.9, 10.2.8): no own
/// `call`; no store, accessor, escape or hard operation reached the key on
/// `t` (`builtin_value_untouched`). The caller adds that no builtin
/// prototype is written under `a`. Unknown code holding the realm and
/// interpreted code refuse.
pub(crate) fn builtin_fn_proto_method(m: &Model, t: Tok, a: Atom) -> bool {
    if m.unit_mode || m.realm_held || m.interp_runs || m.is_private_atom(a) {
        return false;
    }
    let name = m.atoms[a as usize].as_str();
    matches!(
        m.toks[t as usize],
        TokKind::BuiltinFn(Native::Member { .. })
    ) && matches!(members::member(Family::Function, name), Some(Mem::Fn))
        && builtin_value_untouched(m, t, a)
}

/// No store, accessor, `__proto__` store, escape or hard operation reaches
/// key `a` on the builtin function value `t` in the solve.
fn builtin_value_untouched(m: &Model, t: Tok, a: Atom) -> bool {
    let holds_nothing = |n: Option<&NodeId>| {
        n.is_none_or(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.dropped && nd.pts.is_empty() && nd.prim == 0
        })
    };
    holds_nothing(m.fields.get(&(t, a)))
        && holds_nothing(m.field_any.get(&t))
        && !m.super_relink.contains(&t)
        && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !native_written(m, t, a)
}

/// The intrinsic description of builtin function value `t` lists no
/// property `name`, own or on its chain (`builtin_value_absent`).
fn builtin_described_absent(m: &Model, t: Tok, name: &str) -> bool {
    match &m.toks[t as usize] {
        TokKind::Builtin(ns) => {
            !members::is_host(*ns) && matches!(members::ns_member(*ns, name), Some(None))
        }
        TokKind::BuiltinFn(Native::Member { .. }) => {
            name != "prototype" && members::member(Family::Function, name).is_none()
        }
        _ => false,
    }
}

/// R9STORE5 (i): a named store of key `a` onto builtin function value `t`
/// adds or overwrites an own data property of `t` and runs no user code.
/// The description lists `a` neither as an own property of `t` (so not the
/// non-writable `length` / `name`, 10.3.4) nor on its chain
/// (`Function.prototype` / `Object.prototype`: not the `caller` /
/// `arguments` %ThrowTypeError% accessors, 10.2.4); no accessor, definition,
/// delete or unknown writer reached `a` on `t` (a program store makes a
/// writable data property, so it may), `t` is never HARD (freeze, seal,
/// preventExtensions, defineProperty), never escaped and never relinked.
/// ECMA-262 10.1.9.2 step 2.c-e: an own writable data property is set in
/// place, else CreateDataProperty on the extensible `t` (10.3.4). The caller
/// checks the builtin prototypes' writes under `a` (`builtin_chain_plain`).
pub(crate) fn builtin_value_sets(m: &Model, t: Tok, a: Atom) -> bool {
    let name = m.atoms[a as usize].as_str();
    builtin_described_absent(m, t, name)
        && m.native_property(t, name).is_none()
        && !m.super_relink.contains(&t)
        && !m.accessors.contains_key(&(t, a))
        && m.accessor_keys.get(&t).is_none_or(|k| !k.contains(&a))
        && !m.is_escaped(t)
        && !m.hard.contains_key(&t)
        && !native_nonplain_written(m, t, a)
}

/// A call of member `n` of host `h`: the runtime's own method (`native`)
/// until a store reaches `gate` (the member's key, or `STAR_KEY` for a method
/// that reads other members of its receiver), then an unknown method of the
/// host (`this`). A write later in the solve replays it (`NativeMiss::Call`).
/// A write of the member's own key whose stored values every record names
/// calls those values (one that is not callable throws: ECMA-262 7.3.14
/// Call, its arguments reach no one), beside the native where the write did
/// not happen (`native_protocol::answer_stored`, as the replay does).
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
    if gate == n
        && host_written(m, h, a)
        && native_protocol::answer_stored(m, t, a, NativeMiss::Call(t, call))
    {
        native(m);
        return;
    }
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
/// stream's state and writes no key of them (`contract_reason`: Phase B,
/// owner 2026-11-23); beside foreign code they reach code that may write
/// them (`writes_any`). It reads other
/// members of its receiver (`_write`, `_writableState`, `emit`, ...): a store
/// under any key of the stream ends this (`host_call`'s `STAR_KEY` gate).
fn stream_write(m: &mut Model, h: BuiltinNs, call: CallId) {
    let c = m.calls[call as usize].clone();
    // The stream's contract: it keeps the chunk and the callback, and the
    // program reads them back; no one writes them under any key.
    let r = m.contract_reason(&format!(
        "argument of host {h:?}.write (kept by the stream)"
    ));
    for &a in c.args.iter().chain(c.spread.iter()) {
        m.sink_node(a, r);
    }
    // K3 (A): its invoke points, which the contract escape above no longer
    // runs. A chunk that is no string and no binary view is converted by
    // ToString (perry `with_write_bytes` -> `js_jsvalue_to_string`); the
    // callback (the third argument, else the second) is queued as a tick and
    // called with no receiver and no argument (`schedule_write_callback`),
    // its throw reported by the host.
    if !c.spread.is_empty() {
        return unknown_method(m, call, "spread stream write arguments");
    }
    if let Some(&chunk) = c.arg(0) {
        string_methods::to_prim(m, chunk, "stream write chunk ToString");
    }
    let host = m.host_report;
    for i in [1, 2] {
        if let Some(&cb) = c.arg(i) {
            let _ = collections::cb_call_in(m, call, cb, ThisArg::Undef, vec![], host);
        }
    }
    ret_prim(m, call, prim::BOOL);
}

/// A console method's arguments. `log`, `info`, `debug`, `error`, `warn`
/// and `trace` print `js_util_format` of them (perry-runtime
/// builtins/console.rs, formatting/util_format.rs; Node
/// util.formatWithOptions): every value is shown (`Protocol::InspectShown`,
/// `format_jsvalue`: the `[util.inspect.custom]` hook only). The specifiers
/// (`%s` ToPrimitive, `%d` `%i` `%f` ToNumber, `%j` JSON.stringify, `%o`
/// `%O` inspect, `%c`) are parsed only in a String first argument and only
/// at a `%`, and they consume only later arguments: those take
/// `Protocol::Inspect` once a value that may be the first may hold a String
/// with `%` (rule CONSOLE-PCT, `Cons::ConsoleFormat`). The first argument is
/// `args[0]` when no spread precedes it (`CallSite::first_arg_fixed`); else
/// a spread's elements may be the first as well as later ones. Every other
/// method keeps `Protocol::Inspect` on each argument but a lone first one.
/// Rule R2: builtin namespace `ns` is an ordinary object (its chain ends
/// at `Object.prototype`), which the formatter shows only within the inspect
/// depth limit; any other is a function or of unknown kind.
pub(crate) fn ns_is_ordinary(ns: BuiltinNs) -> bool {
    members::ns_chain(ns) == Family::Object
}

fn console_args(m: &mut Model, call: CallId, name: &str) {
    let c = m.calls[call as usize].clone();
    let r = m.reason("console argument (formatted by the runtime)");
    let ek = m.elem_key();
    let mut elems = Vec::new();
    for &s in &c.spread {
        let dst = m.node_l("console-spread-elems");
        m.cons(s, Cons::Load { key: ek, dst });
        elems.push(dst);
    }
    if !matches!(name, "log" | "info" | "debug" | "error" | "warn" | "trace") {
        // Rule R2: `console.dir(x, opts)` formats under the options' depth
        // (console.rs:1738 `decode_dir_depth_option`): any value up to
        // Infinity, which the solver keeps no constant of, so the hard cap.
        // Every other method sets no limit of its own (ambient).
        let dir_opts = name == "dir" && (c.args.len() >= 2 || !c.spread.is_empty());
        let (floor, ambient) = if dir_opts {
            (INSPECT_HARD_CAP, false)
        } else {
            (INSPECT_DEFAULT_LIMIT, true)
        };
        for (i, &a) in c.args.iter().enumerate() {
            let first = i == 0 && c.spread.is_empty();
            m.console_inspect(a, r, first, c.region, floor, ambient);
        }
        for &d in &elems {
            m.console_inspect(d, r, false, c.region, floor, ambient);
        }
        ret_prim(m, call, prim::UNDEF);
        return;
    }
    let fixed = c.first_arg_fixed || c.spread.is_empty();
    // `PERRY_WPF_IMPLSRC=1`: a formatting call whose first argument may
    // come from a spread (the call, its site and the function holding it).
    if !fixed && crate::solve::implsrc_on() {
        let f = m.call_place.get(&c.site).map(|&(f, _)| f);
        let fname = f
            .and_then(|f| m.funcs.get(f as usize))
            .map(|fi| fi.name.clone());
        eprintln!(
            "perry: wpfacts console-unfixed: call={call} site={} name={name} args={} spread={} fn={fname:?} site_call=({}, args={}, spread={}, ret={})",
            c.site,
            c.args.len(),
            c.spread.len(),
            m.calls.get(c.site as usize).map_or(false, |s| s.first_arg_fixed),
            m.calls.get(c.site as usize).map_or(0, |s| s.args.len()),
            m.calls.get(c.site as usize).map_or(0, |s| s.spread.len()),
            m.calls.get(c.site as usize).map_or(0, |s| s.ret),
        );
    }
    let mut firsts = Vec::new();
    let mut later = Vec::new();
    for (i, &a) in c.args.iter().enumerate() {
        if i == 0 {
            firsts.push(a);
        }
        if i > 0 || !fixed {
            later.push(a);
        }
    }
    for &d in &elems {
        if !fixed {
            firsts.push(d);
        }
        later.push(d);
    }
    for &a in c.args.iter().chain(&elems) {
        m.console_inspect(a, r, true, c.region, INSPECT_DEFAULT_LIMIT, true);
    }
    if !later.is_empty() {
        let agg = m.node_l("console-consumable");
        for &a in &later {
            m.edge(a, agg);
        }
        for &f in &firsts {
            m.cons(
                f,
                Cons::ConsoleFormat {
                    reason: r,
                    agg,
                    region: c.region,
                },
            );
        }
    }
    ret_prim(m, call, prim::UNDEF);
}

/// %Promise%.resolve / %Promise%.reject with this = %Promise% (ECMA-262
/// 27.2.4.7 PromiseResolve, 27.2.4.6 NewPromiseCapability and its reject
/// function): a fresh promise; `resolve` adopts the argument (a thenable's
/// `then` is called in a later job), `reject` makes the argument the
/// rejection value a `.catch` handler or an `await`'s throw arm reads.
fn promise_resolve_reject(m: &mut Model, name: &str, call: CallId) {
    let p = site_tmpl(m, call, "Promise");
    let v = promise_value(m, p);
    if let Some(a) = arg(m, call, 0) {
        if name == "resolve" {
            let r = promise_reason(m, p);
            adopt_r(m, call, a, v, r);
        } else {
            let r = promise_reason(m, p);
            m.edge(a, r);
        }
    } else {
        m.add_prim(v, prim::UNDEF);
    }
    ret_tok(m, call, p);
}

fn ns_method(m: &mut Model, ns: BuiltinNs, name: Option<&str>, call: CallId) {
    crate::family_reach::namespace(m, ns, name);
    ns_method_inner(m, ns, name, call, false)
}

/// R-HOSTREPORT: a call that may register a process listener (`process.on`
/// and the other EventEmitter registrations on the one process object;
/// any event, a computed key may name the method) hands the listener what
/// the host reports and the process events.
pub(crate) fn process_listener_registered(m: &mut Model, call: CallId) {
    let (h, e) = (m.host_report, m.esc_raise);
    m.edge(h, e);
    // W-HOST: the listener is handed the process events too
    // (`Model::process_event`).
    let pe = m.process_event;
    m.edge(pe, e);
    // The listener itself runs with them (os_process_emitter.rs
    // `emit_process_uncaught_exception`: listener(err, origin), this =
    // process); another event name, or a computed one, may be any of
    // them. Its own throw is the host's to report.
    let c = m.calls[call as usize].clone();
    if let Some(&cb) = c.arg(1) {
        let arg = m.node_l("process-listener-arg");
        m.edge(h, arg);
        m.edge(pe, arg);
        let p = m.tok(TokKind::Builtin(BuiltinNs::Process));
        let this = m.node_l("process-listener-this");
        m.add_tok(this, p);
        let _ = collections::cb_call_in(m, call, cb, ThisArg::Node(this), vec![arg], h);
    }
}

fn ns_method_inner(
    m: &mut Model,
    ns: BuiltinNs,
    name: Option<&str>,
    call: CallId,
    identified: bool,
) {
    // The step driver of an async function (perry-transform
    // generator/lower/async_step.rs, resume.rs) settles its promise with
    // `GlobalGet(0).reject(e)`. Codegen lowers a `resolve` / `reject` call on
    // the global sentinel to the runtime's function itself
    // (lower_call/console_promise.rs `try_lower_promise_static_call`:
    // `js_promise_resolved` / `js_promise_rejected`): it reads no binding of
    // the global object, so no store replaces it. That is %Promise%.reject /
    // .resolve with this = %Promise% (ECMA-262 27.2.4.6, 27.2.4.7): a fresh
    // promise settled by the argument; no program code runs at the call.
    if ns == BuiltinNs::Other {
        if let Some(n @ ("resolve" | "reject")) = name {
            m.diagnostic_builtin(call, |_| format!("Promise.{n}"));
            return promise_resolve_reject(m, n, call);
        }
    }
    if let Some(name) = name {
        if matches!(members::ns_member(ns, name), Some(Some(Mem::Fn)))
            || ns == BuiltinNs::Other && (identified || global_function(m, name))
        {
            m.diagnostic_builtin(call, |_| format!("{ns:?}.{name}"));
        }
    }
    // A solved call through the `node:vm` module object (`const vm =
    // require("vm"); (0, vm.runInNewContext)(src, ctx)`): its functions
    // compile and run source text, so interpreted code runs where this call
    // runs (DECISIONS 225/235), as at the HIR's direct `node:vm` call
    // (`gen_rest.rs`). Only a member the realm declares on `node:vm` is one
    // of them: a key it does not declare (`platform`, read through a
    // computed `require` that holds every builtin module object) is no vm
    // function. The realm does not say which of its members take a source,
    // so every declared member counts (`createContext` and `isContext`
    // too). A computed key may name any of them.
    if matches!(ns, BuiltinNs::Module(n) if same_builtin(n, "vm"))
        && name.is_none_or(|k| vm_member(m, k))
    {
        set_dyn_eval(m, "E14 node:vm");
    }

    // R-HOSTREPORT: a process listener ("uncaughtException",
    // "unhandledRejection": os_process_emitter.rs
    // `emit_process_uncaught_exception`, promise/rejection.rs
    // `report_unhandled`) is handed what the host reports. A call that may
    // register one (any event; a computed key may name the method) gives
    // the report to unknown code, as the listener is.
    if ns == BuiltinNs::Process
        && match name {
            None => true,
            Some(n) => matches!(
                n,
                "on" | "once" | "addListener" | "prependListener" | "prependOnceListener"
            ),
        }
    {
        process_listener_registered(m, call);
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
    // Unknown code holds the realm: it may have replaced this member of the
    // namespace (`native_protocol::realm_replaced`, the one write predicate).
    // The call is the realm's function below or unknown code, now or when
    // the realm comes to be held later in the solve (the replay). Every
    // namespace member call passes here, method and value call alike.
    {
        let t = m.tok(TokKind::Builtin(ns));
        let a = m.atom(name);
        if !builtin_function(m, t) && opaque_written(m, t, a, NativeMiss::Call(t, call)) {
            let prev = m.cur_this.replace(t);
            unknown_method(
                m,
                call,
                &format!("argument of builtin {ns:?}.{name} (unknown code may have replaced it)"),
            );
            m.cur_this = prev;
        }
    }
    // MATHB1B: the generic-arm call of a namespace pair naming this member
    // runs no model of the original: the pair's intrinsic arm is that call
    // (`Gen::namespace_pair`). A write that may have replaced the member
    // (above, and the stored values bound as callees) still binds here.
    if m.ns_pair_of(call)
        .is_some_and(|(pns, k)| pns == ns && m.atoms[k as usize] == name)
    {
        return;
    }
    // VALUE-CALL-MODELS: the eight Object statics run the direct call's steps.
    if object_statics::value_call(m, ns, name, call) {
        return;
    }
    match (ns, name) {
        (BuiltinNs::Other | BuiltinNs::Console, n)
            if host_of(ns, n, true) == Some(BuiltinNs::Console) =>
        {
            host_call(m, BuiltinNs::Console, n, n, call, |m| {
                console_args(m, call, n)
            })
        }
        // A stream method: the runtime's own, which hands its receiver to no
        // one; a listener it registers is called by the runtime (escaped).
        (BuiltinNs::Stdout | BuiltinNs::Stderr | BuiltinNs::Stdin, "write") => {
            host_call(m, ns, "write", crate::STAR_KEY, call, |m| {
                stream_write(m, ns, call)
            })
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
                if let Some(&delay) = c.arg(1) {
                    string_methods::to_prim(m, delay, "timer timeout ToNumber");
                }
            }
            if let Some(&cb) = c.arg(0) {
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
                // R-JOBTHROW / R-HOSTREPORT: a throw of the callback goes to
                // the host (HTML 8.1.4.6 "report the exception"; perry
                // timer.rs `with_timer_uncaught_trap`), which no program
                // `catch` reads (`Model::host_report`).
                let host = m.host_report;
                let _ = collections::cb_call_in(m, call, cb, this_arg, args, host);
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
            if opaque_written(m, g, a, NativeMiss::Call(g, call)) {
                let prev = m.cur_this.replace(g);
                unknown_method(m, call, &format!("argument of builtin GlobalThis.{n}"));
                m.cur_this = prev;
            } else {
                ns_method(m, BuiltinNs::Other, Some(n), call);
            }
        }
        // A process method whose effects the runtime declares at the body its
        // dispatch arm calls (`process.cwd`), reached through its function
        // value (`process.cwd?.()`, `const f = process.cwd; f()`): the
        // native is the bound method of the one process object, so the
        // declaration runs whatever `this` is, as the direct call does.
        (BuiltinNs::Process, n)
            if !m.unit_mode && effects::declared_member(&format!("process.{n}")).is_some() =>
        {
            let e = effects::declared_member(&format!("process.{n}")).expect("the guard found it");
            effects::run_function(m, e, call);
        }
        // WGPOST: `postMessage(v, t)`, a bare call on the global sentinel.
        // S8 (gen_call.rs) keeps it a member call only in a module a Worker
        // may run (`Model::worker_side`); on the main realm the name is
        // [[Get]](globalThis, "postMessage"), absent: a ReferenceError that
        // calls nothing. In the Worker it is the Worker global's own
        // `postMessage` (perry-stdlib worker_surface.rs
        // `install_web_worker_globals`, `web_worker_post_message`), which
        // ignores `this` and is Worker.postMessage's structured clone: its
        // model (`worker_post_message`). A write under the name by code the
        // model does not see (now, or later in the solve: the replay) is the
        // unknown call again. A program store under the name (named, or
        // under a key that may spell it: R-GLOBAL, program stores are
        // exact) replaces the native's property: [[Get]](globalThis,
        // "postMessage") reads it (`loads::tok_load`, the global's field of
        // a program-owned key), and the call calls it with `this` undefined
        // (ECMA-262 13.3.6.1 EvaluateCall, 9.1.1.4.6 GetBindingValue), as
        // the bare call of a module no Worker runs does (gen_call.rs S8).
        (BuiltinNs::Other, "postMessage")
            if !m.unit_mode
                && native_protocol::sentinel_user_global(m, "postMessage")
                && m.calls[call as usize].spread.is_empty() =>
        {
            let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            let a = m.atom("postMessage");
            if opaque_written(m, g, a, NativeMiss::Call(g, call)) {
                let prev = m.cur_this.replace(g);
                unknown_method(m, call, "argument of builtin Other.postMessage");
                m.cur_this = prev;
            } else {
                collections::worker_post_message(m, call);
            }
            let c = m.calls[call as usize].clone();
            let stored = m.node_l("worker-global-stored");
            loads::tok_load(m, g, Key::Named(a), stored);
            let r = {
                let thrown = m.thrown;
                collections::cb_call_in(m, call, stored, ThisArg::Undef, c.args, thrown)
            };
            m.edge(r, c.ret);
        }
        // A global function the realm declares (`fetch`), while the global
        // holds it: what its declaration says. A write under the name (now,
        // or later in the solve: the replay) is the unknown call again.
        (BuiltinNs::Other, n) if !m.unit_mode && effects::declared_function(n).is_some() => {
            let e = effects::declared_function(n).expect("the guard found it");
            let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
            let a = m.atom(n);
            if opaque_written(m, g, a, NativeMiss::Call(g, call)) {
                let prev = m.cur_this.replace(g);
                unknown_method(m, call, &format!("argument of builtin Other.{n}"));
                m.cur_this = prev;
            } else {
                effects::run_function(m, e, call);
            }
        }
        (
            BuiltinNs::Other,
            "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "encodeURIComponent"
            | "decodeURIComponent" | "encodeURI" | "decodeURI" | "escape" | "unescape" | "atob"
            | "btoa" | "String" | "Number" | "Boolean" | "BigInt",
        ) => {
            // ECMA-262 19.2.2-19.2.6, B.2.1, 20.3.1.1, 21.1.1.1, 21.2.1.1,
            // 22.1.1.1; HTML atob/btoa (DOMString): each converts its
            // arguments by ToString / ToNumber (ToPrimitive on an object),
            // `Boolean` by ToBoolean (no call); the result kind is fixed.
            if name != "Boolean" {
                implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            }
            let p = match name {
                "parseInt" | "parseFloat" | "Number" => prim::NUM,
                "isNaN" | "isFinite" | "Boolean" => prim::BOOL,
                "BigInt" => prim::BIGINT,
                _ => prim::STR,
            };
            ret_prim(m, call, p);
        }
        // EnumerableOwnProperties(key) and GetOwnPropertyKeys(string) are
        // Strings, GetOwnPropertyKeys(symbol) Symbols (ECMA-262 20.1.2.18,
        // 20.1.2.10, 20.1.2.11; a Proxy's `ownKeys` result is filtered by
        // type, 10.5.11).
        (BuiltinNs::Object, "keys" | "getOwnPropertyNames" | "getOwnPropertySymbols") => {
            let pb = site_tmpl(m, call, "keys");
            let fa = m.field_any(pb);
            match (name, arg(m, call, 0)) {
                // Rule PK: the own names of the argument.
                ("getOwnPropertyNames", Some(o)) => m.cons(o, Cons::OwnNames { dst: fa }),
                // Rule EK: its own enumerable String keys (7.3.23).
                ("keys", Some(o)) => m.cons(o, Cons::OwnKeys { dst: fa }),
                ("getOwnPropertySymbols", _) => m.add_prim(fa, prim::SYMBOL),
                _ => m.add_prim(fa, prim::STR),
            }
            ret_tok(m, call, pb);
        }
        (BuiltinNs::Object, "values") => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            // Own enumerable values only (EnumerableOwnProperties): no
            // inherited member is among them.
            if let Some(o) = arg(m, call, 0) {
                m.cons(o, Cons::OwnValues { dst: fa });
            }
            m.add_prim(fa, prim::STR);
            ret_tok(m, call, arr);
        }
        // Each entry is CreateArrayFromList(« key, value ») (ECMA-262 7.3.23
        // step 4.a.ii.2.b), a new ordinary Array: index 0 the key (rule EK,
        // `Cons::OwnKeys`), index 1 the value, each its own slot, as
        // `Gen` models a direct `Object.entries` call.
        (BuiltinNs::Object, "entries") => {
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            if let Some(o) = arg(m, call, 0) {
                let pair = site_tmpl_born(m, call, "entry", true, false);
                if let Some(tm) = m.tmpl_of(pair) {
                    m.ordinary_arrays.insert(tm);
                }
                let ks = m.node_l("ownkeys");
                m.cons(o, Cons::OwnKeys { dst: ks });
                let k0 = m.index_slot(pair, 0);
                m.edge(ks, k0);
                let vs = m.node_l("ownvalues");
                m.cons(o, Cons::OwnValues { dst: vs });
                let v1 = m.index_slot(pair, 1);
                m.edge(vs, v1);
                m.add_tok(fa, pair);
            }
            ret_tok(m, call, arr);
        }
        (BuiltinNs::Object, "freeze" | "seal" | "preventExtensions") => {
            if let Some(o) = arg(m, call, 0) {
                let r = m.reason("E8 Object.freeze (as a value call)");
                let kind = match name {
                    "freeze" => Hard::E8Freeze,
                    "seal" => Hard::E8Seal,
                    _ => Hard::E8PreventExtensions,
                };
                m.cons(o, Cons::Hard { kind, reason: r });
                let ret = m.calls[call as usize].ret;
                m.edge(o, ret);
            }
        }
        (BuiltinNs::Object, "is") => {
            for i in 0..2 {
                if let Some(a) = arg(m, call, i) {
                    m.identity(a, IdentityRoute::ObjectIs);
                }
            }
            ret_prim(m, call, prim::BOOL);
        }
        (BuiltinNs::Object, "isFrozen" | "isSealed" | "isExtensible") => {
            if let Some(o) = arg(m, call, 0) {
                is_extensible_ops(m, o, name != "isExtensible");
            }
            ret_prim(m, call, prim::BOOL);
        }
        // 20.1.2.13: ToObject(O), ToPropertyKey(P), then O.[[GetOwnProperty]](P):
        // a Proxy's `getOwnPropertyDescriptor` trap runs.
        (BuiltinNs::Object, "hasOwn") => {
            if let Some(o) = arg(m, call, 0) {
                let idx = arg(m, call, 1).unwrap_or(NO_NODE);
                m.cons(
                    o,
                    Cons::OwnProp {
                        key: Key::any(),
                        idx,
                    },
                );
            }
            ret_prim(m, call, prim::BOOL);
        }
        // Only a call with a spread argument (`object_statics::value_call`
        // models every other). Neither reads the this value (ECMA-262
        // 20.1.2.2, 20.1.2.23, 28.1.13): it is given to no one.
        (BuiltinNs::Object, "create") => unknown_method_at(
            m,
            call,
            "argument of builtin Object.create",
            Some(0),
            Recv::Ignored,
        ),
        (BuiltinNs::Object | BuiltinNs::Reflect, "setPrototypeOf") => unknown_method_at(
            m,
            call,
            &format!("argument of builtin {ns:?}.setPrototypeOf"),
            Some(1),
            Recv::Ignored,
        ),
        (BuiltinNs::Object, "getPrototypeOf") | (BuiltinNs::Reflect, "getPrototypeOf") => {
            if let Some(o) = arg(m, call, 0) {
                let ret = m.calls[call as usize].ret;
                m.cons(o, Cons::ProtoOf { dst: ret });
            }
        }
        (BuiltinNs::Object, "assign") => {
            let c = m.calls[call as usize].clone();
            if let Some(&target) = c.arg(0) {
                // ECMA-262 20.1.2.1 step 1: `to` is ToObject(target), the
                // result; the sources copy into it.
                let t = to_object(m, target, call);
                let site = m.anon_post_site("Object.assign (value call)");
                m.stores[site as usize].assign = true;
                for &s in c.args_from(1).iter() {
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
                if m.solver_call_keyed(
                    call,
                    (crate::summary::MK_FROM_MAP, 0, 0),
                    Some(f),
                    CallSite {
                        first_arg_fixed: false,
                        this: ThisArg::Undef,
                        args: vec![fa, num],
                        spread: vec![],
                        ret,
                        site: call2,
                        region: m.calls[call as usize].region,
                    },
                )
                .is_some()
                {
                    m.cons(f, Cons::Call { call: call2 });
                }
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
                let mc = m.solver_call_keyed(
                    call,
                    (crate::summary::MK_FROM_CONSTRUCT, 0, 0),
                    Some(this),
                    CallSite {
                        first_arg_fixed: false,
                        this: ThisArg::Undef,
                        args: vec![len],
                        spread: vec![],
                        ret: made,
                        site: c.site,
                        region: m.calls[call as usize].region,
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
                m.cons(
                    a,
                    Cons::Implicit {
                        reason,
                        proto,
                        region: m.thrown,
                    },
                );
            }
            for &a in &c.spread {
                m.cons(
                    a,
                    Cons::Implicit {
                        reason: rr,
                        proto: Protocol::Any,
                        region: m.thrown,
                    },
                );
            }
            // A replacer function is called back (`json_replacer_calls`).
            if m.reach_json_replacer && (c.args.len() > 1 || !c.spread.is_empty()) {
                let holder = site_tmpl(m, call, "json");
                m.json_replacer_builtin_call(call, holder);
            }
            ret_prim(m, call, prim::STR | prim::UNDEF);
        }
        (BuiltinNs::Json, "parse") => {
            implicit_args(m, call, "JSON.parse text", Protocol::ToPrimitive);
            let reviver = arg(m, call, 1).is_some();
            let positions = !reviver && m.json_positions;
            let (j, ja) = json_value(m, call, positions, reviver);
            let fa = m.field_any(j);
            ret_tok(m, call, j);
            ret_tok(m, call, ja);
            ret_prim(m, call, if reviver { prim::ALL } else { prim::JSON });
            if let Some(rv) = arg(m, call, 1) {
                // reviver(key, value) with `this` = the holder, an object
                // or an array
                let ret = m.calls[call as usize].ret;
                let s = m.node_l("str");
                m.add_prim(s, prim::STR);
                for (hi, holder) in [j, ja].into_iter().enumerate() {
                    let call2 = m.calls.len() as CallId;
                    let _call2 = m.chain_call(
                        call,
                        hi as u32,
                        rv,
                        CallSite {
                            first_arg_fixed: false,
                            this: ThisArg::Tok(holder),
                            args: vec![s, fa],
                            spread: vec![],
                            ret,
                            site: call2,
                            region: m.calls[call as usize].region,
                        },
                        move |m, tg, call2| m.cons(tg, Cons::Call { call: call2 }),
                    );
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
        // The `Number.is*` predicates test the operand's type (ECMA-262
        // 21.1.2.2-21.1.2.5): a Boolean, nothing converted.
        (BuiltinNs::Number, "isFinite" | "isInteger" | "isNaN" | "isSafeInteger") => {
            ret_prim(m, call, prim::BOOL)
        }
        // 21.1.2.12/13 (%parseFloat%, %parseInt%): ToString, a Number.
        (BuiltinNs::Number, "parseFloat" | "parseInt") => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::NUM);
        }
        // 22.1.2.1/2: ToNumber of each code unit / point, a String.
        (BuiltinNs::String, "fromCharCode" | "fromCodePoint") => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::STR);
        }
        // 21.2.2.1/2: ToIndex of `bits`, ToBigInt of the value, a BigInt.
        (BuiltinNs::BigInt, "asIntN" | "asUintN") => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::BIGINT);
        }
        // `String.raw` (22.1.2.4, `string_methods::string_raw`): a String.
        (BuiltinNs::String, "raw") => {
            let c = m.calls[call as usize].clone();
            let cooked = m.node_l("String.raw-template");
            let subs = m.node_l("String.raw-substitution");
            if let Some(&a0) = c.args.first() {
                m.edge(a0, cooked);
            }
            for &a in c.args.iter().skip(1) {
                m.edge(a, subs);
            }
            let ek = m.elem_key();
            for &s in &c.spread {
                m.cons(s, Cons::Load { key: ek, dst: subs });
                if c.args.is_empty() {
                    m.cons(
                        s,
                        Cons::Load {
                            key: ek,
                            dst: cooked,
                        },
                    );
                }
            }
            string_methods::string_raw(m, cooked, subs);
            ret_prim(m, call, prim::STR);
        }
        (BuiltinNs::Number, _) if name == "raw" => {
            // `String.raw` reads `raw` and its elements.
            implicit_args(m, call, "String.raw template", Protocol::Any);
            ret_prim(m, call, prim::ALL);
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
        }
        (BuiltinNs::Number | BuiltinNs::BigInt, _) | (BuiltinNs::String, _) => {
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
        (BuiltinNs::Promise, "resolve" | "reject") => promise_resolve_reject(m, name, call),
        (BuiltinNs::Promise, "all" | "allSettled" | "race" | "any") => {
            let p = site_tmpl(m, call, "Promise");
            let v = promise_value(m, p);
            let arr = site_tmpl(m, call, "Array");
            let fa = m.field_any(arr);
            // ECMA-262 27.2.4.1 ff.: the iterable is iterated
            // (GetIterator); its values are the items.
            // `Promise.all([e0, …, en-1])` over a literal: the literal is
            // fresh, only the iteration holds it and %ArrayIteratorPrototype%
            // is locked, so item `i` is `ei`; its resolve element function
            // writes index `i` of the result only (27.2.4.1.2 step 8, 27.2.4.1.3).
            let lit = match (
                name,
                m.calls[call as usize].spread.is_empty(),
                arg(m, call, 0),
            ) {
                ("all", true, Some(a)) if promise_all_index() => m.lit_elems_at(call, a),
                _ => None,
            };
            let settled = m.node_l("settled");
            // R-JOBTHROW: an item's rejection goes where the combinator
            // sends it (27.2.4.1.2 / .2.2 / .3.1 / .5.1): `all` and `race`
            // reject their promise with it, `allSettled` records it, `any`
            // collects it into the AggregateError's errors; never the
            // shared thrown cell.
            let item_rej = m.node_l("item-reason");
            if let Some(els) = lit {
                for (i, e) in els.into_iter().enumerate() {
                    let s = m.node_l("settled");
                    adopt_r(m, call, e, s, item_rej);
                    m.edge(s, settled);
                    let slot = m.index_slot(arr, i as u32);
                    m.edge(s, slot);
                }
            } else {
                let vals = all_arg_values(m, call);
                let items = iterate(m, call, vals);
                adopt_r(m, call, items, settled, item_rej);
                m.edge(settled, fa);
            }
            if name == "allSettled" {
                // {status, value|reason} records
                let rec = site_tmpl(m, call, "settledRecord");
                let rfa = m.field_any(rec);
                m.edge(settled, rfa);
                let thrown = m.thrown;
                m.edge(thrown, rfa);
                m.edge(item_rej, rfa);
                m.add_prim(rfa, prim::STR);
                m.add_tok(fa, rec);
            }
            if name == "all" || name == "allSettled" {
                m.add_tok(v, arr);
            } else {
                m.edge(settled, v);
            }
            if name == "all" || name == "race" {
                let r = promise_reason(m, p);
                m.edge(item_rej, r);
            }
            if name == "any" {
                // Every item rejected: an AggregateError whose `errors` are
                // the reasons (rejections are what `thrown` holds).
                let e = error_tok(m, call);
                let errs = site_tmpl(m, call, "Array");
                let efa = m.field_any(errs);
                let thrown = m.thrown;
                m.edge(thrown, efa);
                m.edge(item_rej, efa);
                let ea = m.atom("errors");
                let f = m.field(e, ea);
                m.add_tok(f, errs);
                m.add_tok(thrown, e);
                let r = promise_reason(m, p);
                m.add_tok(r, e);
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
        // `TypeError.captureStackTrace` is `Error`'s, read through the chain.
        (
            BuiltinNs::Error | BuiltinNs::NativeError(_) | BuiltinNs::AggregateError,
            "captureStackTrace",
        ) => {
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
                        region: m.thrown,
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
        // `ArrayBuffer.isView(a)` (ECMA-262 25.1.5.1): a Boolean; it reads
        // only the argument's [[ViewedArrayBuffer]] slot and runs no code.
        (BuiltinNs::ArrayBuffer, "isView") => ret_prim(m, call, prim::BOOL),
        // The Date statics called through the constructor (a program that
        // writes one of them lowers each call as `Date.m(…)`, perry-hir
        // `try_patched_builtin_call`): the realm's own functions, the steps
        // of the intrinsic forms (`Expr::DateNow` / `DateParse` / `DateUtc`,
        // gen_rest.rs; perry-runtime date_proto_thunks.rs
        // `date_now_static` / `date_parse_static` / `date_utc_static`).
        // ECMA-262 21.4.3.1 `now`: a Number, no argument read, `this`
        // ignored. 21.4.3.2 `parse`: ToString of the one operand. 21.4.3.4
        // `UTC`: ToNumber of each component. A value the program stored
        // under the key is bound as the callee besides (the ordinary read),
        // and a write unknown code may make holds the realm where the read
        // gives ⊤.
        (BuiltinNs::Date, "now") => ret_prim(m, call, prim::NUM),
        (BuiltinNs::Date, "parse" | "UTC") => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            ret_prim(m, call, prim::NUM);
        }
        // A key the namespace's description (its own keys and its chain)
        // declares absent is no method (TypeError); one the program added
        // is the receiver's own field or chain value (`method_on`).
        (_, n) if members::ns_member(ns, n) == Some(None) => {}
        // A reached call of the native `createHook`: the hook it makes may
        // be enabled, and its `init` (unknown code: createHook is otherwise
        // undeclared) gets every value declared `async_resource`.
        (BuiltinNs::Module("node:async_hooks"), "createHook") => {
            async_hook_may_run(m, "a reached call of async_hooks.createHook");
            unknown_method_proto(
                m,
                call,
                "argument of builtin node:async_hooks.createHook",
                None,
                true,
            )
        }
        // %TypedArray%.from / .of (ECMA-262 23.2.2.1 step 2, 23.2.2.2
        // step 3): a this value that is no constructor (IsConstructor,
        // 7.2.4) is a TypeError before any step reads an argument: no code
        // runs, nothing is handed on, the realm is not held.
        (BuiltinNs::TypedArray | BuiltinNs::TypedArrayKind(_), "from" | "of")
            if this_no_constructor(m, call) => {}
        // A host object no namespace model describes (`ns_statics` lists no
        // members: the global object, `process`, `require`, a host namespace
        // or node module object such as `Atomics`, `Intl`, `node:fs`), under
        // a key no arm above models: a method call on it (a program call, a
        // protocol's lookup such as SerializeJSONProperty's `toJSON`,
        // ECMA-262 25.5.2.2 step 2) is EvaluateCall's GetV then Call
        // (13.3.6.1, 7.3.20): it calls what [[Get]] of the key reads, which
        // the realm declaration answers for every key in whole-program mode
        // (`loads::tok_load`, REALMVAL: a key it lacks reads `undefined`, or
        // what a write stored). A call of the member's own value
        // (`identified`) is that value's call, not a lookup.
        (_, n) if !identified && declaration_answers_keys(m, ns) => {
            let t = m.tok(TokKind::Builtin(ns));
            let a = m.atom(n);
            m.call_read_member(t, Key::Named(a), call)
        }
        (_, n) => {
            let recv = if statics_ignore_this(ns) && members::ns_own_fn(ns, n) {
                Recv::Ignored
            } else {
                Recv::Own
            };
            unknown_method_at(
                m,
                call,
                &format!("argument of builtin {ns:?}.{n}"),
                None,
                recv,
            )
        }
    }
}

/// A host object no namespace model describes (`ns_statics` lists no
/// members: the global object, `process`, `require`, a realm host namespace
/// or node module object such as `Atomics`, `Intl`, `node:fs`; not `Other`,
/// the HIR global sentinel): in whole-program mode the realm declaration
/// answers [[Get]] of every key on it, a well-known symbol key included
/// (`loads::tok_load`, REALMVAL: a key it lacks reads `undefined`, or what
/// a write stored).
pub(super) fn declaration_answers_keys(m: &Model, ns: BuiltinNs) -> bool {
    !m.unit_mode && ns != BuiltinNs::Other && members::ns_statics(ns).members.is_none()
}

/// A call of the Worker global's own native `n` (`%WorkerGlobal%.<n>`,
/// perry-stdlib worker_threads/worker_surface.rs; S8 binds a bare call in a
/// module a Worker may run to it, `loads::worker_global_member`). Each
/// ignores `this`:
/// - `postMessage(v, transfer)` is Worker.postMessage's structured clone
///   (`js_worker_threads_post_message`): its model (`worker_post_message`).
/// - `removeEventListener(type, cb)` compares `cb` with the parent port's
///   listeners (message_port.rs `remove_event_listener`; a string `type`
///   only, no conversion): calls nothing, `undefined`.
/// - `close()` sets the thread's close flag: calls nothing, `undefined`.
/// - `addEventListener(type, cb)` registers `cb` on the parent port, which
///   calls it with a message event of what the parent posts: the
///   worker-side message input, which no model describes (as a handler
///   stored under `onmessage`, `members::global_runtime_reads`): `cb` is
///   handed to code the model does not see.
pub(crate) fn worker_global_call(m: &mut Model, n: &str, call: CallId) {
    match n {
        "postMessage" => collections::worker_post_message(m, call),
        "removeEventListener" | "close" => ret_prim(m, call, prim::UNDEF),
        "addEventListener" => unknown_method_at(
            m,
            call,
            "argument of Worker global addEventListener (worker-side message input)",
            None,
            Recv::Ignored,
        ),
        _ => unreachable!("REALMVAL: Worker global native {n} has no model"),
    }
}

/// A namespace none of whose own static functions reads the this value:
/// `Object` (ECMA-262 20.1.2), `Reflect` (28.1), `Math` (21.3.2) and `JSON`
/// (25.5). Such a static called as a value with any receiver (a field
/// holding it, called as its object's method; the ToPrimitive call of
/// `toString` / `valueOf`) does what the plain call does: the receiver is
/// given to no one. Not `Array.from` / `of`, `Promise.*` or
/// `%TypedArray%.from` / `of`, which construct with the this value; and a
/// member found on the namespace's chain (`Function.prototype.call`) is no
/// own static.
/// The this value of `call` is no constructor: the receiver token the call
/// runs on (given to the call, or the call's own `ThisArg::Tok`, as a
/// model's call has it) is an object without [[Call]]
/// (`Model::no_call_tok`), or the call has no receiver (`undefined`).
fn this_no_constructor(m: &Model, call: CallId) -> bool {
    match (m.cur_this, &m.calls[call as usize].this) {
        (Some(t), _) | (None, &ThisArg::Tok(t)) => m.no_call_tok(t),
        (None, ThisArg::Undef) => true,
        (None, ThisArg::Node(_)) => false,
    }
}

fn statics_ignore_this(ns: BuiltinNs) -> bool {
    matches!(
        ns,
        BuiltinNs::Object | BuiltinNs::Reflect | BuiltinNs::Math | BuiltinNs::Json
    )
}

/// The realm identity of module member `module.method` (`async_hooks`,
/// `createHook`) whose call `ns_method_inner` models by name in the solve:
/// a direct HIR call of it is a call of that token.
pub(crate) fn solved_namespace_call(module: &str, method: &str) -> Option<&'static str> {
    let bare = module.strip_prefix("node:").unwrap_or(module);
    if (bare, method) != ("async_hooks", "createHook") {
        return None;
    }
    crate::native_effects::realm()
        .iter()
        .find(|e| e.name == "node:async_hooks.createHook")
        .map(|e| e.name)
}

/// What `require(spec)` returns for one value of its specifier (R-SPEC):
/// `spec` a string constant, resolved as the loader resolves it (Node
/// resolves builtins first, then paths), or `None`, a value no constant
/// spells (any string, a token, ⊤), which may name anything. A literal call
/// is the one-value case; `Cons::RequireSpec` runs it per value that reaches
/// a computed specifier.
pub(crate) fn require_by_spec(m: &mut Model, call: CallId, spec: Option<Atom>) {
    let name = spec.map(|a| m.atoms[a as usize].clone());
    // `require('module')` is the node:module namespace, which hands out
    // `Module._cache`: the cache reaches code nothing models. A value no
    // constant spells may name it too.
    if name
        .as_deref()
        .is_none_or(|s| matches!(s, "module" | "node:module"))
    {
        let r = m.reason("node:module namespace hands out Module._cache");
        let cache = m.cjs_cache;
        m.sink_node(cache, r);
    }
    // A compiled module named by a constant path: its exports (the
    // registry record's `exports`, as `require.cache[path]` holds).
    let rec = spec.and_then(|a| m.cjs_paths.get(&a)).copied();
    if m.explain && rec.is_none() {
        let known: Vec<String> = m
            .cjs_paths
            .keys()
            .map(|a| m.atoms[*a as usize].clone())
            .collect();
        eprintln!("perry: wpfacts explain: require call spec {name:?} not in {known:?}");
    }
    if let Some(rec) = rec {
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
        return;
    }
    // The runtime loader never interprets code: a specifier no compiled
    // path resolves names a registered record (its live exports or a raw
    // publication), a native module namespace, a manifest addon or parsed
    // JSON. Only the last three are opaque.
    // All of it only once a specifier reaches the call: a shared loader
    // body no computed specifier reaches yields nothing.
    implicit_args(m, call, "require specifier", Protocol::ToPrimitive);
    let ret = m.calls[call as usize].ret;
    // Besides the opaque ⊤, the result is a registered record's exports, as
    // `requirePathModule` reads them: a ⊤ callee runs only what escaped, and
    // registering a module's exports does not escape them. A relative
    // literal (`createRequire(import.meta.url)("./l.cjs")`) names no
    // registered path either. Per unit the registry is another unit's
    // value, ⊤, as in `requirePathModule`.
    if !m.unit_mode {
        let reg = m.cjs_union;
        m.cons(reg, Cons::CjsPublish { dst: ret });
    }
    // The loader resolves builtins first: the result is also the builtin
    // module object of each name the value may be, a constant's own or,
    // for a value no constant spells, every one. A member read then
    // resolves per key.
    let names: Vec<&'static str> = match name.as_deref() {
        Some(s) => builtin_module_names()
            .filter(|n| same_builtin(n, s))
            .collect(),
        None => builtin_module_names().collect(),
    };
    let mods = m.node_l("require: builtin module objects");
    for n in names {
        if let Some(t) = builtin_module_tok(m, n) {
            m.add_tok(mods, t);
        }
    }
    if let Some(a0) = m.calls[call as usize].arg(0).copied() {
        m.cons(
            a0,
            Cons::TopIfAny {
                dst: ret,
                from: mods,
                reason: None,
            },
        );
    }
    // It caches the registry records it loads.
    let r = m.reason("module loaded by a computed specifier");
    let (cache, _) = require_parts(m);
    let fa = m.field_any(cache);
    let reg = m.cjs_union;
    m.edge(reg, fa);
    if let Some(a0) = m.calls[call as usize].arg(0).copied() {
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

/// CTOR-CALL-THROWS: a builtin constructor whose [[Call]] with an undefined
/// NewTarget throws a TypeError as its first step, before it reads its
/// receiver or any argument: %TypedArray% and each concrete typed-array
/// constructor (ECMA-262 23.2.1.1, 23.2.5.1 step 1), ArrayBuffer (25.1.4.1),
/// SharedArrayBuffer (25.2.3.1), DataView (25.3.2.1), Map (24.1.1.1), Set
/// (24.2.1.1), WeakMap (24.3.1.1), WeakSet (24.4.1.1), Promise (27.2.3.1).
/// The runtime's call path of each is a throwing thunk
/// (`object/global_this/ctor_thunks.rs`: `typed_array_constructor_call_thunk`,
/// `construct_only_builtin_call_thunk`, `map_`/`set_`/`weak_map_`/`weak_set_`/
/// `promise_constructor_call_thunk`). Not here: constructors with a call
/// behaviour (`Array`, `Object`, `String`, `Number`, `Boolean`, `BigInt`,
/// `Symbol`, `Date`, `RegExp`, `Error`s, `Function`).
///
/// The global object (ECMA-262 19), `Math` (21.3), `JSON` (25.5) and
/// `Reflect` (28.1) are ordinary objects without a [[Call]] internal
/// method: Call throws a TypeError at 7.3.14 step 2, before it uses the
/// receiver or an argument.
pub(crate) fn call_throws(ns: BuiltinNs) -> bool {
    matches!(
        ns,
        BuiltinNs::GlobalThis
            | BuiltinNs::Math
            | BuiltinNs::Json
            | BuiltinNs::Reflect
            | BuiltinNs::TypedArray
            | BuiltinNs::TypedArrayKind(_)
            | BuiltinNs::ArrayBuffer
            | BuiltinNs::SharedArrayBuffer
            | BuiltinNs::DataView
            | BuiltinNs::Map
            | BuiltinNs::Set
            | BuiltinNs::WeakMap
            | BuiltinNs::WeakSet
            | BuiltinNs::Promise
    )
}

pub fn call_builtin_ns(m: &mut Model, ns: BuiltinNs, call: CallId) {
    crate::family_reach::namespace(m, ns, None);
    m.diagnostic_builtin(call, |_| format!("{ns:?}"));
    if call_throws(ns) {
        // As a class called without `new` (`solve/call_and_construct.rs`
        // `ClassObj`): the call throws before it uses anything; nothing
        // flows into its result, no receiver or argument reaches code.
        return;
    }
    match ns {
        BuiltinNs::Require => {
            // The path registry may run a lazy module body inside the call.
            let (lazy, region) = (m.lazy_raise, m.calls[call as usize].region);
            m.edge(lazy, region);
            // R-SPEC: the call resolves per value of its specifier. A
            // literal is the one-value case; a computed specifier resolves
            // each value that reaches it (`Cons::RequireSpec`).
            match m.call_lit.get(&call).copied() {
                Some(a) => require_by_spec(m, call, Some(a)),
                None => {
                    if let Some(a0) = m.calls[call as usize].arg(0).copied() {
                        m.cons(a0, Cons::RequireSpec { call });
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
        BuiltinNs::Error
        | BuiltinNs::NativeError(_)
        | BuiltinNs::AggregateError
        | BuiltinNs::RegExp => {
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
        // Called as functions: String 22.1.1.1, Number 21.1.1.1 (ToNumeric),
        // BigInt 21.2.1.1 (ToPrimitive, then ToBigInt), Symbol 20.4.1.1
        // (ToString of the description) convert by ToPrimitive; Boolean
        // 20.3.1.1 is ToBoolean (no call). The result kind is fixed.
        BuiltinNs::Boolean => ret_prim(m, call, prim::BOOL),
        BuiltinNs::String | BuiltinNs::Number | BuiltinNs::BigInt | BuiltinNs::Symbol => {
            implicit_args(m, call, "ToPrimitive", Protocol::ToPrimitive);
            let p = match ns {
                BuiltinNs::String => prim::STR,
                BuiltinNs::Number => prim::NUM,
                BuiltinNs::BigInt => prim::BIGINT,
                _ => prim::SYMBOL,
            };
            ret_prim(m, call, p);
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
            let arr = site_new_array(m, call);
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

/// The per-index rule for `Promise.all` over an array literal, OFF by
/// default (`PERRY_FACTS_PROMISE_ALL_INDEX=1`: on). Off, the result is one
/// merged element set as for any iterable. On, the three.js skin solve
/// keeps its proven sites but its maker trees deepen (one GLTFLoader
/// per-primitive Mesh holds 10,942 objects instead of 278, work 103M ->
/// past the 285M bound; BATCHB31): the rule waits for that cost's fix.
fn promise_all_index() -> bool {
    crate::provenance::env("PERRY_FACTS_PROMISE_ALL_INDEX").as_deref() == Some("1")
}

/// Rule PXINV: O.[[IsExtensible]]() of what `o` holds (`Object.isExtensible`,
/// `Reflect.isExtensible`), and with `integrity` TestIntegrityLevel's
/// (7.3.16) O.[[OwnPropertyKeys]]() and O.[[GetOwnProperty]](k) of each key
/// (`Object.isFrozen` / `isSealed`). An ordinary object runs no code there;
/// a Proxy runs its traps (`Cons::Meta`, `Cons::OwnProp`).
pub(crate) fn is_extensible_ops(m: &mut Model, o: NodeId, integrity: bool) {
    let reason = m.reason("[[IsExtensible]] / TestIntegrityLevel");
    m.cons(
        o,
        Cons::Meta {
            op: MetaOp::IsExtensible,
            reason,
        },
    );
    if integrity {
        m.cons(
            o,
            Cons::Meta {
                op: MetaOp::OwnKeys,
                reason,
            },
        );
        m.cons(
            o,
            Cons::OwnProp {
                key: Key::any(),
                idx: NO_NODE,
            },
        );
    }
}
