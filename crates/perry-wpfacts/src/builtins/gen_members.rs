//! The members a generator object inherits past its own protocol.
//!
//! A generator object is its step closure (`FuncInfo::gen_step`); the
//! function-protocol rule (`collections::function_method`) gave it `next`,
//! `return` and `throw` only. Its chain goes on past its generator
//! function's `prototype` to `%GeneratorPrototype%` and
//! `%IteratorPrototype%` (an async one's to `%AsyncGeneratorPrototype%` and
//! `%AsyncIteratorPrototype%`): `[Symbol.iterator]()` returns the generator
//! itself and the iterator helpers (`map`, `filter`, `take`, `forEach` ...)
//! pull its values. Without them a call of `gen().map(f)` ran nothing and
//! `g[Symbol.iterator]` read no value: a live receiver with an empty
//! points-to set, and `f` (or the step body behind
//! `g[Symbol.iterator].call(g).next()`) was called inactive while it ran.
//! `PERRY_FACTS_REACH_GEN_MEMBERS=0` (`reach::gen_members`) turns the
//! rules off.

use super::collections::{iter_over, iterator_method, WK_ITERATOR};
use super::*;

const WK_ASYNC_ITERATOR: &str = "@@__perry_wk_asyncIterator";

/// The step closure's function when `t` is a generator object and the
/// rules are on: (the function, an async generator's).
fn generator(m: &Model, t: Tok) -> Option<bool> {
    if !m.reach_gen_members {
        return None;
    }
    let TokKind::Func(f, _) = m.toks[t as usize] else {
        return None;
    };
    let fi = &m.funcs[f as usize];
    fi.gen_step().then_some(fi.gen_async)
}

/// `[Symbol.iterator]` of a sync generator, `[Symbol.asyncIterator]` of an
/// async one: `%IteratorPrototype%`'s and `%AsyncIteratorPrototype%`'s
/// "return this".
fn is_self_iterator(asynchronous: bool, name: &str) -> bool {
    name == if asynchronous {
        WK_ASYNC_ITERATOR
    } else {
        WK_ITERATOR
    }
}

/// An iterator helper of `%IteratorPrototype%` (a sync generator's chain):
/// its function members but the generator's own protocol and the
/// self-iterator.
fn is_helper(asynchronous: bool, name: &str) -> bool {
    !asynchronous
        && !matches!(name, "next" | "return" | "throw")
        && name != WK_ITERATOR
        && matches!(members::own(Family::Iterator, name), Some(Mem::Fn))
}

/// The values the generator object `t` yields to a helper called at
/// `call`: the helper reads `next` off the generator (GetIteratorDirect:
/// a `next` the program stored on the chain is the one found) and calls it
/// with no argument, a protocol call, then reads the step result as every
/// consumer does. A helper that stops early closes the generator the same
/// way (`return`).
fn pulled(m: &mut Model, t: Tok, call: CallId) -> NodeId {
    let site = m.calls[call as usize].site;
    let run = m.call_run(call);
    let recv = m.node_l("generator-helper-this");
    m.nodes[recv as usize].frame = run;
    m.add_tok(recv, t);
    let sub = |m: &mut Model, name: &str| -> NodeId {
        let ret = m.node_l("generator-helper-step");
        m.nodes[ret as usize].frame = run;
        let tg = m.atom("generator helper step");
        let na = m.atom(name);
        if let Some(c2) = m.solver_call_keyed(
            call,
            (crate::summary::MK_BUILTIN_CB, tg, na),
            Some(recv),
            CallSite {
                this: ThisArg::Node(recv),
                args: vec![],
                spread: vec![],
                ret,
                site,
                region: m.thrown,
            },
        ) {
            m.protocol_calls.insert(c2);
            m.cons(recv, Cons::MCall { key: Key::Named(na), call: c2 });
        }
        ret
    };
    let step = sub(m, "next");
    // Unconditional: conservative for a helper that always runs to the
    // end (`toArray`, `forEach`, `reduce`); the others may stop early.
    let _ = sub(m, "return");
    let out = m.node_l("generator-helper-values");
    m.nodes[out as usize].frame = run;
    let va = m.yield_key();
    m.cons(step, Cons::Load { key: va, dst: out });
    m.step_done(step);
    out
}

/// A call of member `name` of generator object `t`, when it is one of the
/// inherited members: `true`, the call is modelled. A computed key
/// (`None`) may name any of them beside the protocol the caller's rule
/// goes on to run: the generator itself is a result, and a helper the
/// model cannot name receives the arguments (`false`).
pub(super) fn call(m: &mut Model, t: Tok, name: Option<&str>, call: CallId) -> bool {
    let Some(asynchronous) = generator(m, t) else {
        return false;
    };
    match name {
        Some(n) if is_self_iterator(asynchronous, n) => {
            ret_tok(m, call, t);
            true
        }
        Some(n) if is_helper(asynchronous, n) => {
            let values = pulled(m, t, call);
            let it = iter_over(m, call, members::ANY_ITERATOR, values);
            iterator_method(m, it, Some(n), call);
            true
        }
        Some(_) => false,
        // A runtime protocol's touch under any key (`Model::protocol_calls`)
        // runs the step, as before.
        None if m.protocol_calls.contains(&call) => false,
        None => {
            ret_tok(m, call, t);
            if !asynchronous {
                let _ = pulled(m, t, call);
                unknown_method(m, call, "computed method of a generator object");
            }
            false
        }
    }
}

/// A read of member `name` of generator object `t` as a value
/// (`const f = g[Symbol.iterator]`, `g.next`), when the intrinsic chain
/// holds it: the function value there, which the model does not name (a
/// call of it with the generator as `this` is a call of an opaque builtin
/// function: the generator escapes to it). `true`: `dst` holds it.
pub(super) fn load(m: &mut Model, t: Tok, name: &str, dst: NodeId) -> bool {
    let Some(asynchronous) = generator(m, t) else {
        return false;
    };
    if !(matches!(name, "next" | "return" | "throw")
        || is_self_iterator(asynchronous, name)
        || is_helper(asynchronous, name))
    {
        return false;
    }
    let a = m.atom(name);
    // Imprecise on purpose: the generic built-in function token, with no
    // identity. A value read of `g.next` / `g[Symbol.iterator]` is an
    // opaque builtin function, so `f.call(g)` lets `g` escape and returns
    // ⊤. A member-identity token (`Native::Member { identity }` of the
    // `%GeneratorPrototype%` / `%IteratorPrototype%` member) belongs here
    // once the realm names these members.
    let bf = builtin_fn(m, "generator object member");
    m.add_tok(dst, bf);
    // A key a store into ⊤ may have written onto the chain reads what was
    // stored too, unless the read is final (DEC 250).
    if !locked_chain_read(m, t, a) && !intrinsic_miss(m, a, NativeMiss::Load(dst)) {
        opaque_member(m, dst);
    }
    true
}
