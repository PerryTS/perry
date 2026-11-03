//! ECMA-262 23.2. The caller has a solved TypedArray-family receiver;
//! own_method's intrinsic_miss replays the opaque path if a method changes.
use super::collections::cb_call;
use super::string_methods::{read_prim, to_prim};
use super::*;

fn load(m: &mut Model, recv: NodeId, key: Key) -> NodeId {
    let dst = m.node_l("typed-read");
    m.cons(recv, Cons::Load { key, dst });
    dst
}

/// TypedArray construction/from: GetMethod(@@iterator), iterator steps;
/// otherwise LengthOfArrayLike and indexed Get. Each element is converted
/// to Number/BigInt. Primitive length arguments have no observable hooks.
/// No source object or element identity is retained. ArrayBuffer sharing
/// is recorded separately by the caller; offsets and lengths use ToIndex.
pub(crate) fn source(m: &mut Model, call: CallId, src: NodeId) -> NodeId {
    read_prim(m, src, "length", "typed-array source length ToLength");
    let ek = m.elem_key();
    let indexed = load(m, src, ek);
    let iterated = iterate(m, call, src);
    let vals = m.node_l("typed-source-element");
    m.edge(indexed, vals);
    m.edge(iterated, vals);
    vals
}

/// SpeciesConstructor reads constructor and @@species. Construct the
/// returned constructor with the algorithm's exact argument list, with
/// the intrinsic result alongside it for the default/nullish case.
fn species(m: &mut Model, t: Tok, call: CallId, args: Vec<NodeId>) {
    let recv = m.node_l("typed-exemplar");
    m.add_tok(recv, t);
    let ca = m.atom("constructor");
    let ctor = load(m, recv, Key::Named(ca));
    let sa = m.atom("@@__perry_wk_species");
    let sp = load(m, ctor, Key::Named(sa));
    let c = m.calls[call as usize].clone();
    // The construct call keeps its origin's source site: the objects it
    // makes are the method call's own templates, not a fresh one per run
    // (a result fed back to the same call would otherwise never settle).
    let made = m.solver_call(
        call,
        CallSite {
            this: ThisArg::Undef,
            args,
            spread: vec![],
            ret: c.ret,
            site: c.site,
        },
    );
    m.cons(sp, Cons::New { call: made });
}

fn fresh(m: &mut Model, call: CallId) -> Tok {
    let out = site_tmpl(m, call, "TypedArray");
    let fa = m.field_any(out);
    m.add_prim(fa, prim::NUM | prim::BIGINT);
    ret_tok(m, call, out);
    out
}

/// False means no described algorithm (computed method, spread, DataView
/// or buffer-specific method): retain the existing Protocol::Any arm.
pub(super) fn method(m: &mut Model, t: Tok, name: &str, call: CallId) -> bool {
    let c = m.calls[call as usize].clone();
    if !c.spread.is_empty() {
        return false;
    }
    let recv = m.node_l("typed-receiver");
    m.add_tok(recv, t);
    let num = m.node_l("typed-index");
    m.add_prim(num, prim::NUM);
    let elem = m.node_l("typed-element");
    m.add_prim(elem, prim::NUM | prim::BIGINT);
    let this_arg = || {
        c.args
            .get(1)
            .copied()
            .map(ThisArg::Node)
            .unwrap_or(ThisArg::Undef)
    };
    match name {
        // ToIntegerOrInfinity(index), Number/BigInt(value); no callback.
        "at" | "copyWithin" | "fill" | "with" | "subarray" | "slice" => {
            let count = match name {
                "at" => 1,
                "with" | "slice" | "subarray" => 2,
                _ => 3,
            };
            for &a in c.args.iter().take(count) {
                to_prim(m, a, "typed-array numeric operand");
            }
            match name {
                "at" => ret_prim(m, call, prim::NUM | prim::BIGINT | prim::UNDEF),
                "copyWithin" | "fill" => ret_tok(m, call, t),
                "with" => {
                    fresh(m, call);
                } // SameType, no species.
                "slice" => {
                    fresh(m, call);
                    species(m, t, call, vec![num]);
                }
                _ => {
                    let out = fresh(m, call);
                    m.cons(recv, Cons::ShareBuffer { ta: out });
                    // Internal [[ViewedArrayBuffer]], offset and length:
                    // represented by the same shared owner token.
                    species(m, t, call, vec![recv, num, num]);
                }
            }
        }
        // Search element is compared, never coerced; only fromIndex is.
        "includes" | "indexOf" | "lastIndexOf" => {
            if let Some(&a) = c.args.get(1) {
                to_prim(m, a, "typed-array fromIndex ToIntegerOrInfinity");
            }
            ret_prim(
                m,
                call,
                if name == "includes" {
                    prim::BOOL
                } else {
                    prim::NUM
                },
            );
        }
        "set" => {
            if let Some(&src) = c.args.first() {
                // set does not iterate source, only indexed Get.
                read_prim(m, src, "length", "typed-array set source ToLength");
                let ek = m.elem_key();
                let v = load(m, src, ek);
                to_prim(m, v, "typed-array set element ToNumber/ToBigInt");
            }
            if let Some(&off) = c.args.get(1) {
                to_prim(m, off, "typed-array set offset ToIntegerOrInfinity");
            }
            ret_prim(m, call, prim::UNDEF);
        }
        "forEach" | "map" | "filter" | "some" | "every" | "find" | "findLast" | "findIndex"
        | "findLastIndex" => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, call, cb, this_arg(), vec![elem, num, recv]);
                if name == "map" {
                    to_prim(m, r, "typed-array map result ToNumber/ToBigInt");
                }
                // Predicates only ToBoolean their result (no hooks).
            }
            match name {
                "map" | "filter" => {
                    fresh(m, call);
                    species(m, t, call, vec![num]);
                }
                "forEach" => ret_prim(m, call, prim::UNDEF),
                "some" | "every" => ret_prim(m, call, prim::BOOL),
                "find" | "findLast" => ret_prim(m, call, prim::NUM | prim::BIGINT | prim::UNDEF),
                _ => ret_prim(m, call, prim::NUM),
            }
        }
        "reduce" | "reduceRight" => {
            let acc = m.node_l("typed-accumulator");
            m.edge(elem, acc);
            if let Some(&init) = c.args.get(1) {
                m.edge(init, acc);
            }
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, call, cb, ThisArg::Undef, vec![acc, elem, num, recv]);
                m.edge(r, acc);
            }
            m.edge(acc, c.ret); // No coercion or escape of the accumulator.
        }
        "sort" | "toSorted" => {
            if let Some(&cb) = c.args.first() {
                let r = cb_call(m, call, cb, ThisArg::Undef, vec![elem, elem]);
                to_prim(m, r, "typed-array comparator result ToNumber");
            }
            if name == "sort" {
                ret_tok(m, call, t);
            } else {
                fresh(m, call);
            }
        }
        "reverse" => ret_tok(m, call, t),
        "toReversed" => {
            fresh(m, call);
        }
        "join" => {
            if let Some(&a) = c.args.first() {
                to_prim(m, a, "typed-array separator ToString");
            }
            ret_prim(m, call, prim::STR);
        }
        "toLocaleString" => {
            // Intl locale/option processing has no precise primitive
            // summary here. Keep those operands conservative until its
            // dictionary and locale-list preconditions are established.
            implicit_args(m, call, "typed-array Intl locales/options", Protocol::Any);
            // Each numeric element's toLocaleString is invoked with the
            // supplied locales/options. Number/BigInt's Intl rule decides
            // their dictionary reads; replacements run through lookup.
            let a = m.atom("toLocaleString");
            let nested = m.calls.len() as CallId;
            let r = m.node_l("typed-locale-result");
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Node(elem),
                    args: c.args.iter().take(2).copied().collect(),
                    spread: vec![],
                    ret: r,
                    site: nested,
                },
            );
            m.cons(
                elem,
                Cons::MCall {
                    key: Key::Named(a),
                    call: nested,
                },
            );
            to_prim(m, r, "typed-array locale element ToString");
            ret_prim(m, call, prim::STR);
        }
        "toString" => {
            // Array.prototype.toString invokes the receiver's join.
            let a = m.atom("join");
            let nested = m.calls.len() as CallId;
            m.solver_call(
                call,
                CallSite {
                    this: ThisArg::Node(recv),
                    args: vec![],
                    spread: vec![],
                    ret: c.ret,
                    site: nested,
                },
            );
            m.cons(
                recv,
                Cons::MCall {
                    key: Key::Named(a),
                    call: nested,
                },
            );
            ret_prim(m, call, prim::STR);
        }
        "keys" | "values" | "entries" | "@@__perry_wk_iterator" => {
            let vals = if name == "entries" {
                super::collections::entry_pairs(m, call, elem, true)
            } else if name == "keys" {
                num
            } else {
                elem
            };
            let it = super::collections::iter_over(m, call, vals);
            ret_tok(m, call, it);
        }
        _ => return false,
    }
    true
}
