//! Named lengths: a local through whose name alone its functions change
//! the length of its Arrays (`BindingFacts::named_length`).
//!
//! `for (let i = 0; i < a.length; i++) body` with no call in `body` is
//! lowered with `a.length` read once (`perry-codegen` `stmt::loops`,
//! `classify_for_length_hoist`): the loop's walker looks at every store of
//! `body` and keeps the length when none of them can change it. A store
//! through the name `a` it can judge (`a[i] = v` under `i < a.length`
//! stays inside, `a.pop()` does not). A store through any other name, as
//! `b.pop()`, `this.items[n] = v` or `G.length = 0`, it cannot: whether `b`
//! may hold the Array `a` holds is not in the text of the loop (`a = k ? G
//! : F`, two parameters bound to one Array, a field read, a call's
//! result). That question is answered here.
//!
//! **The fact.** Local `a` has a named length when it holds only plain
//! program Arrays (`bindings::arr_rep`) and, for every Array token `T` it
//! may hold and every event that may change an Array's length or move its
//! elements where the event reaches `T` (`Model::len_events`: the events of
//! `held_lengths`, rule 3):
//!
//! - the event is placed in a function (its store site's or its call's);
//! - when that function can name `a` (it owns `a`, or it references it:
//!   `Model::captures`), the receiver of the event was read from `a`: the
//!   store's receiver node, or the builtin method call's `this`, is `a`'s
//!   node, a narrowed read of `a` (`Model::name_of`) or a summary
//!   instance's copy of one of them.
//!
//! **Why that is enough.** While a loop of function `F` without calls runs,
//! only `F`'s own code runs: every event during the loop is placed in `F`.
//! `F` names `a`, so each of them that reaches an Array `a` may hold has a
//! receiver read from `a`: it is a store the walker sees as a store through
//! `a`, and judges. Every other store of the loop reaches no Array `a` may
//! hold. (What an accessor or a conversion a plain read may run does is
//! not part of this fact, as it is not part of the walker's.)
//!
//! A receiver read from `a` is what a `LocalGet(a)` yields. An expression
//! that hands such a node on unchanged (a sequence's last operand, an
//! assignment's value) is a receiver read from `a` here too; the walker
//! refuses a store whose receiver may resolve to `a` that way.
//!
//! A program whose solve has a marker gets no fact, as it gets no held
//! length; nor does a per-unit solve.
//!
//! `PERRY_FACTS_NAMED_LENGTHS=0` records and exports nothing (and the
//! walker asks nothing: the loop is lowered as before the fact existed).
//! The negative control `PERRY_FACTS_SABOTAGE=namedlen` claims the fact for
//! every `Arr` local. The dump (`PERRY_FACTS_DUMP`) prints one `# named_len`
//! line per `Arr` local: `verdict=named`, or `refused=<why>`.

use crate::bindings::{ModuleBindings, Rep};
use crate::gen::Gen;
use crate::held_lengths::{call_fn, site_fn};
use crate::model::{FnIdx, NodeId, ThisArg, Tok, NO_FN, NO_NODE};
use crate::pts::FxHashMap;

/// `PERRY_FACTS_NAMED_LENGTHS=0` turns the fact off.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_NAMED_LENGTHS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// One length event on a token: the function it is placed in (`None`:
/// nowhere) and the node its receiver was read from.
type Event = (Option<FnIdx>, NodeId);

/// Fill `BindingFacts::named_length` (module docs).
pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: bool,
    mut probe: Option<&mut Vec<String>>,
) {
    let m = &g.m;
    if !m.named_lengths || m.aborted || m.unit_mode || interpreted {
        return;
    }
    // A marker stands for values that are not in the node that holds it:
    // a receiver that holds one may be any Array.
    if !m.param_marker.is_empty() || m.arg_bits.iter().any(|&w| w != 0) {
        if let Some(p) = probe.as_deref_mut() {
            p.push("# named_len off=markers".to_string());
        }
        return;
    }
    // A summary instance's copy of a node -> the node, and back.
    let mut orig_of: FxHashMap<NodeId, NodeId> = FxHashMap::default();
    let mut copies: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            orig_of.insert(copy, orig);
            copies.entry(orig).or_default().push(copy);
        });
    }
    // The local a receiver node was read from, when it was read from one.
    let name = |recv: NodeId| -> NodeId {
        let recv = orig_of.get(&recv).copied().unwrap_or(recv);
        m.name_of.get(&recv).copied().unwrap_or(recv)
    };
    let mut events: FxHashMap<Tok, Vec<Event>> = FxHashMap::default();
    for &(t, at, call, recv) in &m.len_events {
        let (f, recv) = match (at, call) {
            // A storeless rule (`pop`) of a summary instance's copy of its
            // receiver: the site is the receiver's (`len_index_sites`).
            (u32::MAX, false) if recv != NO_NODE => {
                let orig = orig_of.get(&recv).copied().unwrap_or(recv);
                let site = m.len_index_sites.get(&orig);
                (site.and_then(|&site| site_fn(m, site)), recv)
            }
            (u32::MAX, _) => (None, NO_NODE),
            (_, true) => (
                call_fn(m, at),
                match m.calls.get(at as usize).map(|c| &c.this) {
                    Some(ThisArg::Node(n)) => *n,
                    _ => NO_NODE,
                },
            ),
            (_, false) => (site_fn(m, at), recv),
        };
        let e = (f, if recv == NO_NODE { NO_NODE } else { name(recv) });
        let list = events.entry(t).or_default();
        if !list.contains(&e) {
            list.push(e);
        }
    }
    // The functions that reference a node another function owns.
    let mut namers: FxHashMap<NodeId, Vec<FnIdx>> = FxHashMap::default();
    for &(n, f) in &m.captures {
        namers.entry(n).or_default().push(f);
    }
    for (mi, mb) in bindings.iter_mut().enumerate().take(g.mods.len()) {
        for (id, b) in mb.locals.iter_mut() {
            if !matches!(b.rep, Rep::Arr(_)) {
                continue;
            }
            let Some(&n) = g.mods[mi].locals.get(id) else {
                continue;
            };
            let owner = m.node_fn[n as usize];
            if owner == NO_FN {
                continue;
            }
            let mut toks = m.nodes[n as usize].pts.to_sorted();
            for &c in copies.get(&n).into_iter().flatten() {
                if let Some(node) = m.nodes.get(c as usize) {
                    node.pts.sorted_into(&mut toks);
                }
            }
            toks.sort_unstable();
            toks.dedup();
            let names = |f: FnIdx| f == owner || namers.get(&n).is_some_and(|fs| fs.contains(&f));
            let refusal = if toks.is_empty() {
                Some("no-array".to_string())
            } else if sabotage {
                None
            } else {
                toks.iter().find_map(|t| {
                    events
                        .get(t)
                        .into_iter()
                        .flatten()
                        .find_map(|&(f, recv)| match f {
                            None => Some(format!("event:unplaced:tok{t}")),
                            Some(f) if names(f) && recv != n => Some(format!(
                                "event:other-name({}):tok{t}",
                                m.funcs[f as usize].name.replace(' ', "_")
                            )),
                            Some(_) => None,
                        })
                })
            };
            b.named_length = refusal.is_none();
            if let Some(p) = probe.as_deref_mut() {
                let func = m.funcs[owner as usize].name.replace(' ', "_");
                p.push(match refusal {
                    None => format!(
                        "# named_len module={mi} local={id} fn={func} verdict=named toks={}",
                        toks.len()
                    ),
                    Some(why) => {
                        format!("# named_len module={mi} local={id} fn={func} refused={why}")
                    }
                });
            }
        }
    }
}
