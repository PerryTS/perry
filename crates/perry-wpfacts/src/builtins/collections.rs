//! Builtin summaries: the function protocol, iterators, arrays and other
//! builtin data structures.

use super::*;

// ------------------------------------------------- function protocol

pub(super) fn function_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
    if let Some(name @ ("call" | "apply" | "bind" | "toString")) = name {
        m.diagnostic_builtin(call, |_| format!("Function.prototype.{name}"));
    }

    let c = m.calls[call as usize].clone();
    match name {
        // A well-known-symbol member of `Function.prototype`: only
        // `@@hasInstance` (OrdinaryHasInstance reads `prototype`, a data
        // property of a function; it calls nothing). Any other symbol is
        // absent on a function value: a protocol touch (ToPrimitive,
        // iteration) finds nothing there, a call of it throws.
        Some(n) if n.starts_with(WK_PREFIX) => {
            if n == crate::model::WK_HAS_INSTANCE {
                ret_prim(m, call, prim::BOOL);
            }
        }
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
                    if let Some(source) = m.diagnostic_source_call(call) {
                        m.diagnostic
                            .as_mut()
                            .unwrap()
                            .bound_origins
                            .insert(b, source);
                    }
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
        Some("next" | "return" | "throw") | None if !matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_step) => {
            if name.is_none() || !matches!(m.toks[t as usize], TokKind::Func(..)) {
                unknown_method(m, call, "computed method of a function value");
            }
        }
        // An async generator's step: the function runs with this = itself.
        Some("next" | "return" | "throw") | None
            if matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_async) =>
        {
            m.call_tok(call, t, Some(t));
        }
        // A sync generator's public `next`/`return`/`throw`: the step runs
        // with this = itself and hands back its raw value (step kind 0
        // yield, 1 completion), which the runtime boundary wraps in a fresh
        // `{ value, done }`; a `yield*` re-yield (kind 2) hands back its
        // delegate's inner result object, returned as-is.
        Some("next" | "return" | "throw") | None => {
            let raw = m.node_l("gen-step-raw");
            m.nodes[raw as usize].frame = m.call_run(call);
            let call2 = m.solver_call(
                call,
                CallSite {
                    this: c.this.clone(),
                    args: c.args.clone(),
                    spread: c.spread.clone(),
                    ret: raw,
                    site: c.site,
                },
            );
            m.call_tok(call2, t, Some(t));
            let r = site_tmpl(m, call, "IterResult");
            let va = m.atom("value");
            let v = m.field(r, va);
            m.edge(raw, v);
            let da = m.atom("done");
            let d = m.field(r, da);
            m.add_prim(d, prim::BOOL);
            ret_tok(m, call, r);
            if matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_reyields) {
                m.edge(raw, c.ret);
            }
        }
        Some("toString" | "toLocaleString") => ret_prim(m, call, prim::STR),
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        _ => {}
    }
}

// ------------------------------------------------------------- arrays

fn cb_call(m: &mut Model, by: CallId, cb: NodeId, this: ThisArg, args: Vec<NodeId>) -> NodeId {
    let ret = m.node_l("cb-ret");
    m.nodes[ret as usize].frame = m.call_run(by);
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

/// The `RegExp.prototype` methods that run RegExpExec (22.2.7.1) on the
/// receiver: `test` and `@@replace`/`@@match`/`@@search`.
pub(super) fn regexp_runs_exec(name: &str) -> bool {
    name == "test"
        || name
            .strip_prefix(WK_PREFIX)
            .is_some_and(|s| matches!(s, "replace" | "match" | "search"))
}

/// `@@split` and `@@matchAll` (22.2.6.14, 22.2.6.11) build their splitter or
/// matcher with SpeciesConstructor(rx, %RegExp%): `constructor` (and its
/// `@@species`) is read from the regexp and the result constructed from
/// `(rx, flags)`, so a constructor the program stored runs with any arguments.
pub(super) fn regexp_species(name: &str) -> bool {
    name.strip_prefix(WK_PREFIX)
        .is_some_and(|s| matches!(s, "split" | "matchAll"))
}

fn regexp_species_constructor(m: &mut Model, t: Tok) {
    let recv = m.node_l("regexp");
    m.add_tok(recv, t);
    let k = m.atom("constructor");
    let cn = m.node_l("species-constructor");
    m.cons(
        recv,
        Cons::Load {
            key: Key::Named(k),
            dst: cn,
        },
    );
    let r = m.reason("argument of builtin species constructor of the RegExp");
    m.cons(
        cn,
        Cons::Implicit {
            reason: r,
            proto: Protocol::Any,
        },
    );
}

/// RegExpExec: `exec` is read from the regexp as a property and called with
/// the string, so an `exec` the program stored on it (or on `RegExp.prototype`)
/// runs with the regexp as `this`.
fn regexp_exec(m: &mut Model, t: Tok, call: CallId) {
    let c = m.calls[call as usize].clone();
    let recv = m.node_l("regexp");
    m.add_tok(recv, t);
    let a = m.atom("exec");
    let ret = m.node_l("exec-ret");
    m.nodes[ret as usize].frame = m.call_run(call);
    let site = m.calls.len() as CallId;
    m.solver_call(
        call,
        CallSite {
            this: ThisArg::Node(recv),
            args: c.args.first().copied().into_iter().collect(),
            spread: vec![],
            ret,
            site,
        },
    );
    m.cons(
        recv,
        Cons::MCall {
            key: Key::Named(a),
            call: site,
        },
    );
}

/// The values iterating `v` yields, as `Gen::iterate` models them for
/// compiled code: array-like elements, then the protocol
/// `v[Symbol.iterator]().next().value` (and `v.next().value`).
pub(super) fn iterate(m: &mut Model, call: CallId, v: NodeId) -> NodeId {
    let site = m.calls[call as usize].site;
    let out = m.node_l("b-iterated");
    m.nodes[out as usize].frame = m.call_run(call);
    let ik = m.iter_key();
    m.cons(v, Cons::Load { key: ik, dst: out });
    let sub = |m: &mut Model, this: NodeId, key: Key| -> NodeId {
        let ret = m.node_l("b-iter-ret");
        m.nodes[ret as usize].frame = m.call_run(call);
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
        Some(n) => undescribed_method(
            m,
            Family::Iterator,
            t,
            n,
            call,
            &format!("argument of unmodeled iterator method {n}"),
        ),
    }
}

pub(super) fn array_method(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) {
    if let Some(name) = name {
        if matches!(members::member(Family::Array, name), Some(Mem::Fn)) {
            let owner = if matches!(members::member(Family::Object, name), Some(Mem::Fn)) {
                "Object"
            } else {
                "Array"
            };
            m.diagnostic_builtin(call, |_| format!("{owner}.prototype.{name}"));
        }
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
                    m.cons(
                        a,
                        Cons::Load {
                            key: Key::Any,
                            dst: ofa,
                        },
                    );
                }
            }
            if name == Some("flat") {
                m.cons(
                    elems,
                    Cons::Load {
                        key: Key::Any,
                        dst: ofa,
                    },
                );
            }
            ret_tok(m, call, out);
        }
        Some("sort" | "toSorted") => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, call, cb, ThisArg::Undef, vec![elems, elems]);
                let why = m.reason("sort comparator result");
                m.cons(
                    r,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
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
            m.cons(
                elems,
                Cons::Implicit {
                    reason: why,
                    proto: Protocol::ToPrimitive,
                },
            );
            implicit_args(m, call, "join separator", Protocol::ToPrimitive);
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
        // A name `Array.prototype` does not define is no method of the
        // array: `Object.prototype`'s run through the chain, any other is
        // absent (`split`, `valueOf` through ToPrimitive, a class's method
        // name on a value that may also be an array).
        Some(n) => undescribed_method(
            m,
            Family::Array,
            t,
            n,
            call,
            &format!("argument of unmodeled array method {n}"),
        ),
    }
}

pub(super) fn bobj_method(m: &mut Model, t: Tok, what: &str, name: Option<&str>, call: CallId) {
    if let Some(name) = name {
        if matches!(
            members::member(members::bobj_family(what), name),
            Some(Mem::Fn)
        ) {
            m.diagnostic_builtin(call, |_| format!("{what}.prototype.{name}"));
        }
    }

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
            implicit_args(m, call, "builtin method argument", Protocol::Any);
            if what == "RegExp" && regexp_runs_exec(n) {
                regexp_exec(m, t, call);
            }
            if what == "RegExp" && regexp_species(n) {
                regexp_species_constructor(m, t);
            }
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
            implicit_args(m, call, "buffer resize argument", Protocol::ToPrimitive);
        }
        ("TypedArray", Some(_)) => {
            implicit_args(m, call, "typed array method argument", Protocol::Any);
            ret_prim(m, call, prim::ALL);
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
        }
        (
            "primbox" | "keys" | "json" | "descriptor" | "descriptors" | "groups" | "fromEntries",
            Some(n),
        ) => array_method(m, t, Some(n), call),
        (_, Some(n)) => undescribed_method(
            m,
            members::bobj_family(what),
            t,
            n,
            call,
            &format!("argument of {what}.{n}"),
        ),
    }
}
