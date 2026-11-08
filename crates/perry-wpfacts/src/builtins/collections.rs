//! Builtin summaries: the function protocol, iterators, arrays and other
//! builtin data structures.

use super::effects;
use super::*;

/// BoundFunctionCreate(target `t`, `arg(0)`, `rest(1)`) for `call`, into
/// its result (the declared `bound` operand of `Function.prototype.bind`).
pub(super) fn bound_into_result(m: &mut Model, t: Tok, call: CallId) {
    let c = m.calls[call as usize].clone();
    // One bound value per (birth site, target): a bound value is a
    // birth, so every instance's copy of the bind call makes the
    // same one. Every bound target of a site shares one (`f =
    // f.bind(x)` in a loop rebinds its own result: per target that
    // would mint a new value each round). Its parts (target, bound
    // `this`, bound arguments) are its own nodes, and every bind
    // call that makes it copies its operands into them: the first
    // maker may be a summarized body's shared observation node,
    // which feeds no effect, while each instance's call carries
    // its caller's values.
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
            let bthis = m.node_l("bind-this");
            let bargs: Vec<NodeId> = (1..c.args.len().max(1))
                .map(|_| m.node_l("bind-arg"))
                .collect();
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
    let (target, bthis, bargs) = m.bound[b as usize].clone();
    m.add_tok(target, t);
    match c.args.first() {
        Some(&n) => m.edge(n, bthis),
        None => m.add_prim(bthis, prim::UNDEF),
    }
    let mut extra = Vec::new();
    for (i, &a) in c.args.iter().skip(1).enumerate() {
        match bargs.get(i) {
            Some(&p) => m.edge(a, p),
            None => extra.push(a),
        }
    }
    let bt = m.tok(TokKind::Bound(b));
    m.add_tok(c.ret, bt);
    // bind(...spread), or more bound args than the bound value's
    // first maker had (a call the solver made for the site): the
    // extra bound args land anywhere.
    for s in c.spread.iter().copied().chain(extra) {
        let r = m.reason("bind with spread arguments");
        m.sink_node(s, r);
    }
}

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
                    region: m.thrown,
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
                    region: m.thrown,
                },
            );
            m.call_tok(call2, t, None);
        }
        // A `Function.prototype` method the runtime declares (`bind`): its
        // declaration runs.
        Some(n) if effects::declared_at("Function.prototype", n).is_some() => {
            let e = effects::declared_at("Function.prototype", n).expect("declared");
            effects::run_native(m, e, t, call);
        }
        // Generator protocol (a generator object is its step closure): the
        // step closure only; any other function value has no such methods
        // (a computed key on one is a runtime property: unknown).
        Some("next" | "return" | "throw") | None if !matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_step()) => {
            if name.is_none() || !matches!(m.toks[t as usize], TokKind::Func(..)) {
                unknown_method(m, call, "computed method of a function value");
            }
        }
        // An async generator's step: the function runs with this = itself.
        Some("next" | "return" | "throw") | None if matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_async) =>
        {
            m.call_tok(call, t, Some(t));
            // It rejects: the shared cell.
            let thrown = m.thrown;
            gen_throw_passes(m, name, thrown, &c.args, &c.spread);
        }
        // A sync generator's public `next`/`return`/`throw`: the step runs
        // with this = itself and hands back its raw value (step kind 0
        // yield, 1 completion), which the runtime boundary wraps in a fresh
        // `{ value, done }`; a `yield*` re-yield (kind 2) hands back its
        // delegate's inner result object, returned as-is.
        Some("next" | "return" | "throw") | None => {
            // The public protocol accepts one resume value. The runtime
            // passes that value alone to a one-parameter step, otherwise
            // (numeric operation, value); public extra arguments are ignored.
            let arg = m.node_l("gen-resume-arg");
            m.nodes[arg as usize].frame = m.call_run(call);
            match c.args.first() {
                Some(&value) => m.edge(value, arg),
                None => m.add_prim(arg, prim::UNDEF),
            }
            for &spread in &c.spread {
                let key = m.elem_key();
                m.cons(spread, Cons::Load { key, dst: arg });
            }
            let region = m.calls[call as usize].region;
            gen_throw_passes(m, name, region, &[arg], &[]);
            let args = if matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].params.len() == 1)
            {
                vec![arg]
            } else {
                let op = m.node_l("gen-resume-op");
                m.nodes[op as usize].frame = m.call_run(call);
                m.add_prim(op, prim::NUM);
                vec![op, arg]
            };
            let raw = m.node_l("gen-step-raw");
            m.nodes[raw as usize].frame = m.call_run(call);
            let call2 = m.solver_call(
                call,
                CallSite {
                    this: c.this.clone(),
                    args,
                    spread: vec![],
                    ret: raw,
                    site: c.site,
                    region: m.thrown,
                },
            );
            m.call_tok(call2, t, Some(t));
            let r = site_tmpl(m, call, "IterResult");
            // The raw value holds the yielded values and the completion
            // value alike: a consumer's yield read (`#yield`) is all of it.
            let ya = m.atom(crate::model::YIELD_VALUE);
            let y = m.field(r, ya);
            m.edge(raw, y);
            let va = m.atom("value");
            let v = m.field(r, va);
            m.edge(y, v);
            let da = m.atom("done");
            let d = m.field(r, da);
            m.add_prim(d, prim::BOOL);
            ret_tok(m, call, r);
            if matches!(m.toks[t as usize], TokKind::Func(f, _) if m.funcs[f as usize].gen_reyields)
            {
                m.edge(raw, c.ret);
            }
        }
        Some("toString" | "toLocaleString") => ret_prim(m, call, prim::STR),
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        _ => {}
    }
}

/// A generator's `throw(v)` the runtime raises itself when the generator has
/// not started or has finished (`generator_state.rs`, `throw_program_value`;
/// an async one rejects with it): `v` is caught as it is. A computed method
/// may be `throw`.
/// `thrown` is the region of the `throw` call, or the shared cell for an
/// async generator's (it rejects).
fn gen_throw_passes(m: &mut Model, name: Option<&str>, thrown: NodeId, args: &[NodeId], spread: &[NodeId]) {
    if !matches!(name, Some("throw") | None) {
        return;
    }
    for &a in args {
        m.edge(a, thrown);
    }
    for &s in spread {
        let key = m.elem_key();
        m.cons(s, Cons::Load { key, dst: thrown });
    }
}

// ------------------------------------------------------------- arrays

pub(super) fn cb_call(
    m: &mut Model,
    by: CallId,
    cb: NodeId,
    this: ThisArg,
    args: Vec<NodeId>,
) -> NodeId {
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
            region: m.thrown,
        },
    );
    m.cons(cb, Cons::Call { call: call2 });
    ret
}

/// ToPrimitive on `v`: its `@@toPrimitive`, `valueOf` and `toString` run.
pub(crate) fn to_prim(m: &mut Model, v: NodeId, why: &str) {
    let r = m.reason(why);
    m.cons(
        v,
        Cons::Implicit {
            reason: r,
            proto: Protocol::ToPrimitive,
        },
    );
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

/// `recv[name](...args)` where the algorithm looks the method up as a
/// property: a method the program stored on the receiver runs, with the
/// receiver as `this`.
pub(crate) fn method_call(
    m: &mut Model,
    call: CallId,
    recv: NodeId,
    name: &str,
    args: Vec<NodeId>,
) -> NodeId {
    let a = m.atom(name);
    let ret = m.node_l("method-ret");
    m.nodes[ret as usize].frame = m.call_run(call);
    let site = m.calls.len() as CallId;
    m.solver_call(
        call,
        CallSite {
            this: ThisArg::Node(recv),
            args,
            spread: vec![],
            ret,
            site,
            region: m.thrown,
        },
    );
    m.cons(
        recv,
        Cons::MCall {
            key: Key::Named(a),
            call: site,
        },
    );
    ret
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
                region: m.thrown,
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
    let va = m.yield_key();
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

/// A fresh builtin iterator of kind `what` (one template per source call
/// site and kind) yielding the values in `values`. The kind names the
/// iterator's intrinsic prototype: `members::ARRAY_ITERATOR`
/// (`%ArrayIteratorPrototype%`, what the realm declares Array, its
/// prototype's methods and `%TypedArray%.prototype` birth),
/// `members::MAP_ITERATOR`, `members::SET_ITERATOR`, or
/// `members::ANY_ITERATOR` where the model does not know which (an iterator
/// helper, a folded `.values()` whose receiver may be any iterable).
pub(super) fn iter_over(m: &mut Model, call: CallId, what: &'static str, values: NodeId) -> Tok {
    let it = site_tmpl(m, call, what);
    let e = iter_elems(m, it);
    m.edge(values, e);
    it
}

/// The `[key, value]` pairs of an `entries()` iterator: one array template
/// per call site holding `values` (plus the index for arrays).
pub(super) fn entry_pairs(m: &mut Model, call: CallId, values: NodeId, index: bool) -> NodeId {
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
            // `value` is a yielded value or, on the done result,
            // `undefined`; `#yield` (`Model::yield_key`) only the former.
            let r = site_tmpl(m, call, "IterResult");
            let ya = m.atom(crate::model::YIELD_VALUE);
            let y = m.field(r, ya);
            m.edge(elems, y);
            let va = m.atom("value");
            let v = m.field(r, va);
            m.edge(y, v);
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
                    let it = iter_over(m, call, members::ANY_ITERATOR, r);
                    ret_tok(m, call, it);
                }
                Some("flatMap") => {
                    // The callback's results are iterated in turn
                    // (GetIteratorFlattenable): what iterating them yields.
                    let inner = iterate(m, call, r);
                    let it = iter_over(m, call, members::ANY_ITERATOR, inner);
                    ret_tok(m, call, it);
                }
                _ => {
                    let it = iter_over(m, call, members::ANY_ITERATOR, elems);
                    ret_tok(m, call, it);
                }
            }
        }
        Some("take" | "drop") => {
            let it = iter_over(m, call, members::ANY_ITERATOR, elems);
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

/// The result `made` of a call `mc` a builtin makes its array `out` with:
/// `mc` constructs the program constructors in `ctor` (`Cons::NewProgram`;
/// `%Array%` is `out` itself), and the builtin defines `out`'s elements on
/// what it makes and sets its `length` (ECMA-262 23.1.2.1, 23.1.3.21: the
/// elements by CreateDataPropertyOrThrow, the length by Set). `made` flows
/// into the builtin's result `ret`.
pub(crate) fn construct_into(
    m: &mut Model,
    ctor: NodeId,
    mc: CallId,
    out: Tok,
    made: NodeId,
    ret: NodeId,
) {
    m.cons(ctor, Cons::NewProgram { call: mc });
    let site = m.anon_post_site("elements of an array a program constructor made");
    let elems = m.field_any(out);
    let ek = m.elem_key();
    m.cons(
        made,
        Cons::Store {
            key: ek,
            src: elems,
            site,
        },
    );
    let la = m.atom("length");
    let len = m.node_l("constructed-length");
    m.add_prim(len, prim::NUM);
    m.cons(
        made,
        Cons::Store {
            key: Key::Named(la),
            src: len,
            site,
        },
    );
    m.edge(made, ret);
}

/// ArraySpeciesCreate's construct for template `out` of `call`
/// (`array_species`): `new C(...args)` of a program species `C` (`None`:
/// `new C(length)`), a call of its own, its result one of `call`'s.
fn species_construct(m: &mut Model, out: Tok, sc: NodeId, call: CallId, args: Option<Vec<NodeId>>) {
    let c = m.calls[call as usize].clone();
    let args = args.unwrap_or_else(|| {
        let num = m.node_l("species-length");
        m.add_prim(num, prim::NUM);
        vec![num]
    });
    let made = m.node_l("species-constructed");
    let mc = m.solver_call(
        call,
        CallSite {
            this: ThisArg::Undef,
            args,
            spread: vec![],
            ret: made,
            site: c.site,
            region: m.thrown,
        },
    );
    construct_into(m, sc, mc, out, made, c.ret);
}

/// The receivers' `constructor`, read off the receivers in `src`.
fn species_ctor(m: &mut Model, src: NodeId) -> NodeId {
    let k = m.atom("constructor");
    let cn = m.node_l("array-species-constructor");
    m.cons(
        src,
        Cons::Load {
            key: Key::Named(k),
            dst: cn,
        },
    );
    cn
}

/// Template `tm`'s species node takes constructor node `cn` and its
/// `@@species`. Only this links out of a constructor node, so an edge
/// already there means the link is made (the same load twice adds nothing).
/// True when the species node is made here.
fn species_link(m: &mut Model, tm: TmplId, cn: NodeId) -> bool {
    let (sc, made) = match m.tmpls[tm as usize].species {
        Some(n) => (n, false),
        None => {
            let n = m.node_l("array-species");
            m.tmpls[tm as usize].species = Some(n);
            (n, true)
        }
    };
    let node = &m.nodes[cn as usize];
    let linked = match &node.succ_set {
        Some(s) => s.contains(&sc),
        None => node.succ.contains(&sc),
    };
    if linked {
        return made;
    }
    m.edge(cn, sc);
    let sp = m.atom("@@__perry_wk_species");
    m.cons(
        cn,
        Cons::Load {
            key: Key::Named(sp),
            dst: sc,
        },
    );
    made
}

/// `out` (a call's array) is made from receiver `t` with ArraySpeciesCreate
/// (`array_species`). What `t.constructor` reads depends on `t` alone: one
/// constructor node per receiver token, made on first use, linked to each
/// call's template. `args`: what the construct passes (`species_construct`).
pub(super) fn species_from(
    m: &mut Model,
    out: Tok,
    t: Tok,
    call: CallId,
    args: Option<Vec<NodeId>>,
) {
    let tm = m.tmpl_of(out).expect("site template");
    let cn = match m.species_ctor.get(&t) {
        Some(&n) => n,
        None => {
            let src = m.node_l("array-species-receiver");
            m.add_tok(src, t);
            let cn = species_ctor(m, src);
            m.species_ctor.insert(t, cn);
            cn
        }
    };
    if species_link(m, tm, cn) {
        let sc = m.tmpls[tm as usize].species.expect("species node");
        species_construct(m, out, sc, call, args);
    }
}

/// What iterating `t` yields: each index below the length, `undefined` at
/// a hole (`Model::hole`). It depends on `t` alone: one node per token,
/// made by the first call that iterates `t`; each call reads it through
/// its own edge.
fn arr_iterated(m: &mut Model, t: Tok) -> NodeId {
    if let Some(&n) = m.arr_iterated.get(&t) {
        return n;
    }
    let n = m.node_l("arr-iterated");
    m.arr_iterated.insert(t, n);
    let elems = m.field_any(t);
    m.edge(elems, n);
    if m.is_arr(t) {
        let h = m.hole(t);
        m.edge(h, n);
    }
    n
}

/// `Array.prototype.<name>` run with `this` = object `t`, an array or not
/// (`native_receiver`): `array_method`, and for an object that is no array
/// what the method does with it beyond an array's elements. ECMA-262 23.1.3:
/// the method touches `this` only through Get, Set, HasProperty,
/// DeletePropertyOrThrow and CreateDataPropertyOrThrow, under `length` and
/// the indices, any of which may be an accessor, own or inherited. Each is
/// the program's own operation on a node holding `t` (`non_array_this`), so
/// accessors, setters and the chain are the solver's ordinary ones. Its
/// indices are [[Get]]s under a number: its own fields a number key names
/// (`num_fields`: numeric names too, `{ 0: v }`) and past them its chain
/// (`plain_key`: program prototypes' numeric fields; an intrinsic
/// prototype refuses index adds), all of them its elements here; an
/// accessor under a numeric name runs through `array_method`'s
/// `native_index_access`. `concat` makes it one element unless it is
/// spreadable (IsConcatSpreadable: its `@@isConcatSpreadable`, whose
/// elements the call already takes).
pub(super) fn array_method_on(m: &mut Model, t: Tok, name: &str, call: CallId) {
    if !m.is_arr(t) {
        let own = m.num_fields(t);
        let elems = m.field_any(t);
        m.edge(own, elems);
        m.plain_key(t, ChainAct::Load(elems));
        non_array_this(m, t, name, call);
    }
    array_method(m, t, Some(name), call);
    if name == "concat" && !m.is_arr(t) {
        let out = site_tmpl(m, call, "Array");
        let ofa = m.field_any(out);
        m.add_tok(ofa, t);
    }
}

/// What `Array.prototype.<name>` does to a receiver `t` that is no array
/// beyond its indices, each as the program's own operation on `t`:
/// LengthOfArrayLike (Get `length`, then ToLength: ToPrimitive of what it
/// read), Set `length` (the methods that store it), DeletePropertyOrThrow
/// of an index (the program's `delete o[i]`: E7), `toString`'s Invoke of
/// `join`, `concat`'s Get of `@@isConcatSpreadable`. HasProperty runs
/// program code only as a Proxy's `has` trap, and a Proxy is ⊤.
fn non_array_this(m: &mut Model, t: Tok, name: &str, call: CallId) {
    let recv = m.node_l("array-like-this");
    m.add_tok(recv, t);
    let length = Key::Named(m.atom("length"));
    let len = m.node_l("array-like-length");
    m.cons(
        recv,
        Cons::Load {
            key: length,
            dst: len,
        },
    );
    to_prim(m, len, "array-like length (ToLength)");
    if matches!(name, "push" | "pop" | "shift" | "unshift" | "splice") {
        let num = m.node_l("array-like-new-length");
        m.add_prim(num, prim::NUM);
        let site = m.anon_store_site();
        m.cons(
            recv,
            Cons::Store {
                key: length,
                src: num,
                site,
            },
        );
    }
    if matches!(
        name,
        "pop" | "shift" | "splice" | "unshift" | "reverse" | "copyWithin" | "sort"
    ) {
        let reason = m.reason("E7 delete of an index by an Array.prototype method");
        m.cons(
            recv,
            Cons::Hard {
                kind: Hard::E7Delete,
                reason,
            },
        );
    }
    match name {
        "toString" => {
            let join = Key::Named(m.atom("join"));
            let f = m.node_l("array-like-join");
            m.cons(recv, Cons::Load { key: join, dst: f });
            let r = cb_call(m, call, f, ThisArg::Tok(t), vec![]);
            let ret = m.calls[call as usize].ret;
            m.edge(r, ret);
        }
        "concat" => {
            let k = Key::Named(m.atom(&format!("{WK_PREFIX}isConcatSpreadable")));
            let v = m.node_l("array-like-spreadable");
            m.cons(recv, Cons::Load { key: k, dst: v });
        }
        _ => {}
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

    // A method that may move the receiver's length (`solve::arr_len`);
    // these never do.
    let keeps_length = matches!(
        name,
        Some(
            WK_ITERATOR
                | "next"
                | "return"
                | "throw"
                | "at"
                | "indexOf"
                | "lastIndexOf"
                | "includes"
                | "join"
                | "toString"
                | "toLocaleString"
                | "slice"
                | "concat"
                | "map"
                | "filter"
                | "forEach"
                | "every"
                | "some"
                | "find"
                | "findIndex"
                | "findLast"
                | "findLastIndex"
                | "reduce"
                | "reduceRight"
                | "flat"
                | "flatMap"
                | "keys"
                | "values"
                | "entries"
                | "fill"
                | "copyWithin"
                | "reverse"
                | "sort"
                | "toReversed"
                | "toSorted"
                | "toSpliced"
                | "with"
        )
    );
    if !keeps_length {
        m.arr_len_top(t);
    }
    let c = m.calls[call as usize].clone();
    let elems = m.field_any(t);
    // Every method here reads or writes the receiver's indices through its
    // [[Get]] and [[Set]] (ECMA-262 23.1.3), hand-modeled or declared: the
    // program's accessors under those keys run.
    m.native_index_access(t, elems);
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
            let iterated = arr_iterated(m, t);
            let it = iter_over(m, call, members::ARRAY_ITERATOR, iterated);
            ret_tok(m, call, it);
        }
        // Any other computed-key method: one that returns the receiver, or
        // one that makes an array through its `constructor` (`splice`, `map`
        // ...). A runtime protocol's call (`Model::protocol_calls`) is made
        // per touched value and its result goes back to the runtime only:
        // an array made there holds the receiver's elements, which the
        // touch of the receiver already reaches, so the receiver stands
        // for it (an array per such call would be touched in turn, without
        // end).
        None => {
            ret_tok(m, call, t);
            if m.protocol_calls.contains(&call) {
                return;
            }
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            species_from(m, out, t, call, None);
            ret_tok(m, call, out);
        }
        // Arrays and strings are iterables, not iterators: no `next`,
        // `return` or `throw` (a call throws; `Gen::iterate` tries `next`
        // on every iterable).
        Some("next" | "return" | "throw") => {}
        // A spread argument supplies its elements, never a member a
        // computed key could name.
        Some("unshift") => {
            for &a in &c.args {
                m.edge(a, elems);
            }
            let key = m.elem_key();
            for &s in &c.spread {
                m.cons(s, Cons::Load { key, dst: elems });
            }
            ret_prim(m, call, prim::NUM);
        }
        Some("pop" | "shift" | "at") => {
            m.edge(elems, c.ret);
            ret_prim(m, call, prim::UNDEF);
        }
        // The removed elements, in an array made through the receiver's
        // `constructor`.
        Some("splice") => {
            for &a in c.args.iter().skip(2) {
                m.edge(a, elems);
            }
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            species_from(m, out, t, call, None);
            ret_tok(m, call, out);
        }
        // In place: the receiver itself.
        Some("copyWithin") => ret_tok(m, call, t),
        // A new array, born at this call site, holding the receiver's
        // elements and what the call adds. It is not the receiver: a write
        // into the copy never reaches the source (`args.slice()` then
        // `copy[i] = v`).
        Some("slice" | "concat" | "toReversed" | "flat" | "with" | "toSpliced") => {
            // `slice` keeps exactly the receiver's holes (it skips an absent
            // index, ECMA-262 23.1.3.28 step 15.c); `toReversed`, `with` and
            // `toSpliced` write every index, reading a hole as `undefined`.
            // An array-like receiver may lack any index, and `concat` and
            // `flat` keep the general birth.
            let dense =
                matches!(name, Some("slice" | "toReversed" | "with" | "toSpliced")) && m.is_arr(t);
            let out = if dense {
                site_dense_arr(m, call)
            } else {
                site_tmpl(m, call, "Array")
            };
            if dense {
                let source_holes = m.hole(t);
                if name == Some("slice") {
                    let out_holes = m.hole(out);
                    m.edge(source_holes, out_holes);
                } else {
                    let out_elems = m.field_any(out);
                    m.edge(source_holes, out_elems);
                }
            }
            if matches!(name, Some("slice" | "concat" | "flat")) {
                species_from(m, out, t, call, None);
            }
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
                    // An array operand gives its elements (`Get(E, ToString(k))`),
                    // never a method an any-key read could name.
                    let key = m.elem_key();
                    m.cons(a, Cons::Load { key, dst: ofa });
                }
            }
            if name == Some("flat") {
                m.cons(
                    elems,
                    Cons::Load {
                        key: Key::any(),
                        dst: ofa,
                    },
                );
            }
            ret_tok(m, call, out);
        }
        // toSorted has no interpreted declaration; its comparator's result
        // is converted (ToNumber).
        Some("toSorted") => {
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
            let out = site_tmpl(m, call, "Array");
            let ofa = m.field_any(out);
            m.edge(elems, ofa);
            ret_tok(m, call, out);
        }
        // `forEach` is declared, kept here: its declaration passes the
        // callback a Map/Set receiver's key as the second argument
        // (`prim(num) | elems(this)`), an array's is a number. `map` and
        // `filter` run their declarations (`species(this, ...)`).
        Some("forEach" | "findIndex" | "findLast" | "findLastIndex" | "flatMap") => {
            let Some(&cb) = c.args.first() else {
                return;
            };
            let r = cb_call(m, call, cb, this_arg(1), vec![elems, num, recv]);
            match name {
                Some("flatMap") => {
                    let out = site_tmpl(m, call, "Array");
                    species_from(m, out, t, call, None);
                    let ofa = m.field_any(out);
                    m.edge(r, ofa);
                    m.cons(
                        r,
                        Cons::Load {
                            key: Key::any(),
                            dst: ofa,
                        },
                    );
                    ret_tok(m, call, out);
                }
                Some("findLast") => {
                    m.edge(elems, c.ret);
                    ret_prim(m, call, prim::UNDEF);
                }
                Some("forEach") => ret_prim(m, call, prim::UNDEF),
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
        Some("lastIndexOf" | "findIndexOf") => {
            ret_prim(m, call, prim::NUM | prim::BOOL);
        }
        // Each element's own `toLocaleString(locales, options)` is invoked
        // (ECMA-402 19.5.1), its result made a string; a hole or a nullish
        // element is skipped.
        Some("toLocaleString") => {
            let a = m.atom("toLocaleString");
            let nested = m.calls.len() as CallId;
            let r = m.node_l("array-locale-result");
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Node(elems),
                    args: c.args.iter().take(2).copied().collect(),
                    spread: vec![],
                    ret: r,
                    site: nested,
                    region: m.thrown,
                },
            );
            m.cons(
                elems,
                Cons::MCall {
                    key: Key::Named(a),
                    call: nested,
                },
            );
            let why = m.reason("array join (locale element ToString)");
            m.cons(
                r,
                Cons::Implicit {
                    reason: why,
                    proto: Protocol::ToPrimitive,
                },
            );
            ret_prim(m, call, prim::STR);
        }
        Some("toString") => {
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
            let it = iter_over(m, call, members::ARRAY_ITERATOR, num);
            ret_tok(m, call, it);
        }
        Some("values") => {
            let iterated = arr_iterated(m, t);
            let it = iter_over(m, call, members::ARRAY_ITERATOR, iterated);
            ret_tok(m, call, it);
        }
        Some("entries") => {
            let iterated = arr_iterated(m, t);
            let pairs = entry_pairs(m, call, iterated, true);
            let it = iter_over(m, call, members::ARRAY_ITERATOR, pairs);
            ret_tok(m, call, it);
        }
        Some("hasOwnProperty") => ret_prim(m, call, prim::BOOL),
        // A method the runtime declares the effects of (and no arm above
        // models): its declaration runs (`effects::run_native`). A name
        // `Array.prototype` does not define is no method of the array:
        // `Object.prototype`'s run through the chain, any other is absent
        // (`split`, `valueOf` through ToPrimitive, a class's method name on
        // a value that may also be an array).
        Some(n) => match effects::declared(Family::Array, n) {
            Some(e) => effects::run_native(m, e, t, call),
            None => undescribed_method(
                m,
                Family::Array,
                t,
                n,
                call,
                &format!("argument of unmodeled array method {n}"),
            ),
        },
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
    // The four collection kinds share one model of their contents; a
    // member no arm models is looked up on the kind's own chain.
    let (kind, fam) = (what, members::bobj_family(what));
    let what = match fam {
        Family::Collection => "Collection",
        Family::Iterator => "Iterator",
        _ => what,
    };
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
            let it = iter_over(m, call, members::collection_iterator(kind), vals);
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
            let it = iter_over(m, call, members::collection_iterator(kind), elems);
            ret_tok(m, call, it);
        }
        ("Collection", Some("entries")) => {
            let pairs = entry_pairs(m, call, elems, false);
            let it = iter_over(m, call, members::collection_iterator(kind), pairs);
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
        // ECMA-262 21.4.4: getters inspect [[DateValue]], never the
        // operands. The known Date token and intrinsic_miss in own_method
        // guard this rule; a written method also takes the opaque path.
        (
            "Date",
            Some(
                "getTime" | "getDate" | "getDay" | "getFullYear" | "getYear" | "getMonth"
                | "getHours" | "getMinutes" | "getSeconds" | "getMilliseconds" | "getUTCDate"
                | "getUTCDay" | "getUTCFullYear" | "getUTCMonth" | "getUTCHours" | "getUTCMinutes"
                | "getUTCSeconds" | "getUTCMilliseconds" | "getTimezoneOffset" | "valueOf",
            ),
        ) if c.spread.is_empty() => ret_prim(m, call, prim::NUM),
        (
            "Date",
            Some(
                "toString" | "toDateString" | "toTimeString" | "toUTCString" | "toGMTString"
                | "toISOString",
            ),
        ) if c.spread.is_empty() => ret_prim(m, call, prim::STR),
        (
            "Date",
            Some(
                n @ ("setTime" | "setDate" | "setFullYear" | "setYear" | "setMonth" | "setHours"
                | "setMinutes" | "setSeconds" | "setMilliseconds" | "setUTCDate"
                | "setUTCFullYear" | "setUTCMonth" | "setUTCHours" | "setUTCMinutes"
                | "setUTCSeconds" | "setUTCMilliseconds"),
            ),
        ) if c.spread.is_empty() => {
            // Each supplied component is ToNumber'd. The setter retains
            // only the clipped numeric time and returns a Number.
            let count = match n {
                "setFullYear" | "setUTCFullYear" | "setMinutes" | "setUTCMinutes" => 3,
                "setMonth" | "setUTCMonth" | "setSeconds" | "setUTCSeconds" => 2,
                "setHours" | "setUTCHours" => 4,
                _ => 1,
            };
            for &a in c.args.iter().take(count) {
                string_methods::to_prim(m, a, "Date setter ToNumber");
            }
            ret_prim(m, call, prim::NUM);
        }
        ("Date", Some("toJSON")) if c.spread.is_empty() => {
            // 21.4.4.37: ToPrimitive(this, number), then Invoke(this,
            // "toISOString"). Its arbitrary result is returned verbatim;
            // a nonfinite primitive time instead returns null.
            let recv = m.node_l("date");
            m.add_tok(recv, t);
            string_methods::to_prim(m, recv, "Date toJSON time");
            let a = m.atom("toISOString");
            let nested = m.calls.len() as CallId;
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Node(recv),
                    args: vec![],
                    spread: vec![],
                    ret: c.ret,
                    site: nested,
                    region: m.thrown,
                },
            );
            m.cons(
                recv,
                Cons::MCall {
                    key: Key::Named(a),
                    call: nested,
                },
            );
            ret_prim(m, call, prim::NULL);
        }

        // WHATWG URL: serialization reads internal slots; arguments are
        // ignored. URLSearchParams' inputs alone undergo USVString.
        ("URL", Some("toString" | "toJSON")) if c.spread.is_empty() => ret_prim(m, call, prim::STR),
        ("URLSearchParams", Some(n @ ("get" | "getAll" | "has" | "delete" | "set" | "append")))
            if c.spread.is_empty() =>
        {
            let count = if matches!(n, "get" | "getAll") { 1 } else { 2 };
            for &a in c.args.iter().take(count) {
                string_methods::to_prim(m, a, "URLSearchParams USVString operand");
            }
            match n {
                "get" => ret_prim(m, call, prim::STR | prim::NULL),
                "has" => ret_prim(m, call, prim::BOOL),
                "getAll" => {
                    let out = site_tmpl(m, call, "Array");
                    let fa = m.field_any(out);
                    m.add_prim(fa, prim::STR);
                    ret_tok(m, call, out);
                }
                _ => ret_prim(m, call, prim::UNDEF),
            }
        }
        ("URLSearchParams", Some("toString")) if c.spread.is_empty() => {
            ret_prim(m, call, prim::STR)
        }
        ("URLSearchParams", Some("sort")) if c.spread.is_empty() => ret_prim(m, call, prim::UNDEF),
        ("URLSearchParams", Some("forEach")) if c.spread.is_empty() => {
            if let Some(&cb) = c.args.first() {
                let recv = m.node_l("params");
                m.add_tok(recv, t);
                let text = m.node_l("param-string");
                m.add_prim(text, prim::STR);
                let this = c
                    .args
                    .get(1)
                    .copied()
                    .map(ThisArg::Node)
                    .unwrap_or(ThisArg::Undef);
                let _ = cb_call(m, call, cb, this, vec![text, text, recv]);
            }
            ret_prim(m, call, prim::UNDEF);
        }
        (
            "URLSearchParams",
            Some(n @ ("keys" | "values" | "entries" | "@@__perry_wk_iterator")),
        ) if c.spread.is_empty() => {
            let text = m.node_l("param-string");
            m.add_prim(text, prim::STR);
            let vals = if matches!(n, "entries" | "@@__perry_wk_iterator") {
                entry_pairs(m, call, text, false)
            } else {
                text
            };
            let it = iter_over(m, call, members::ANY_ITERATOR, vals);
            ret_tok(m, call, it);
        }
        // Encoding: input ToString for encode/encodeInto, but decode's
        // BufferSource/destination Uint8Array are validated via internal
        // slots, with no JS property reads or identity handout.
        ("TextCodec", Some("encode" | "encodeInto")) if c.spread.is_empty() => {
            if let Some(&src) = c.args.first() {
                string_methods::to_prim(m, src, "TextEncoder source USVString");
            }
            if name == Some("encode") {
                let out = site_tmpl(m, call, "TypedArray");
                let fa = m.field_any(out);
                m.add_prim(fa, prim::NUM);
                ret_tok(m, call, out);
            } else {
                let out = site_tmpl(m, call, "Object()");
                for key in ["read", "written"] {
                    let a = m.atom(key);
                    let f = m.field(out, a);
                    m.add_prim(f, prim::NUM);
                }
                ret_tok(m, call, out);
            }
        }
        ("TextCodec", Some("decode")) if c.spread.is_empty() => {
            if let Some(&options) = c.args.get(1) {
                string_methods::read_only(m, options, "stream");
            } // ToBoolean, no hooks.
            ret_prim(m, call, prim::STR);
        }
        // Node Timeout handles: no argument is consumed, callbacks/args
        // remain in the original registration; ref/refresh return this.
        ("Timer", Some("ref" | "unref" | "refresh" | "close")) if c.spread.is_empty() => {
            ret_tok(m, call, t)
        }
        ("Timer", Some("hasRef")) if c.spread.is_empty() => ret_prim(m, call, prim::BOOL),
        ("Timer", Some("@@__perry_wk_toPrimitive")) if c.spread.is_empty() => {
            ret_prim(m, call, prim::NUM)
        }
        ("Timer", Some("@@__perry_wk_dispose")) if c.spread.is_empty() => {
            ret_prim(m, call, prim::UNDEF)
        }
        ("RegExp", Some(n)) if string_methods::regexp_described(n) => {
            string_methods::regexp_method(m, t, n, call)
        }
        (
            "Date" | "RegExp" | "URL" | "URLSearchParams" | "TextCodec" | "builtin"
            | "BoxedPrimitive" | "Timer",
            Some(n),
        ) if !n.starts_with("on") && n != "addEventListener" => {
            implicit_args(m, call, "builtin method argument", Protocol::Any);
            if what == "Date" && n == "toJSON" {
                // 21.4.4.37: ToPrimitive(this), then Invoke(this, "toISOString").
                let recv = m.node_l("date");
                m.add_tok(recv, t);
                to_prim(m, recv, "Date toJSON ToPrimitive");
                method_call(m, call, recv, "toISOString", vec![]);
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
        ("TypedArray", Some(n)) if typed_array_methods::method(m, t, n, call) => {}
        ("TypedArray", Some("transfer" | "transferToFixedLength" | "resize" | "grow")) => {
            let r = m.reason("B2 buffer transfer/resize");
            m.mark_hard(t, Hard::B2BufferResize, r);
            ret_tok(m, call, t);
            implicit_args(m, call, "buffer resize argument", Protocol::ToPrimitive);
        }
        ("TypedArray", Some(_)) => {
            implicit_args(m, call, "typed array method argument", Protocol::Any);
            if name == Some("toString") {
                // Array.prototype.toString calls the receiver's `join`.
                let recv = m.node_l("typed-array");
                m.add_tok(recv, t);
                method_call(m, call, recv, "join", vec![]);
            }
            ret_prim(m, call, prim::ALL);
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
        }
        (
            "primbox" | "arraybox" | "keys" | "json" | "descriptor" | "descriptors" | "groups"
            | "fromEntries" | "mapGroups" | "hostrecord",
            Some(n),
        ) => match members::box_chain(kind) {
            members::BoxChain::Array | members::BoxChain::Unknown => {
                array_method(m, t, Some(n), call)
            }
            // No `Array.prototype` on the chain (`box_chain`): past the own
            // values (never callable) an ordinary object finds
            // `Object.prototype`'s methods, run through the chain, and any
            // other key absent (final, `locked_read`); a null-prototype
            // object finds nothing (a TypeError).
            members::BoxChain::Object => undescribed_method(
                m,
                Family::Object,
                t,
                n,
                call,
                &format!("argument of {kind}.{n}"),
            ),
            members::BoxChain::Null => {}
        },
        (_, Some(n)) => undescribed_method(m, fam, t, n, call, &format!("argument of {kind}.{n}")),
    }
}
