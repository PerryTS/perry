//! Held lengths: a local whose Arrays keep their length while its function
//! runs (`BindingFacts::stable_length`; CENSUS-residual-2 lever 8).
//!
//! `for (const x of f())`, or `for (let i = 0; i < a.length; i++)` with
//! calls in its body, reads `a.length` and re-derives the element slot at
//! every step, because a call may push onto `a`. The loop may hold the
//! length instead when no code that runs during the loop can change it.
//!
//! **The fact.** Local `a` of function `H` has a stable length when, for
//! every Array token `T` it may hold (all plain program Arrays:
//! `bindings::arr_rep`):
//!
//! 1. **`T` lives in frames only.** Every node that holds `T` is a node of
//!    one function body (`Model::node_fn`: a local, a parameter, a
//!    temporary, a result; or a summary instance's slot) that no other
//!    function references (`Model::captures`), and that function is plain:
//!    no generator, no async function, no generator step closure, whose
//!    frame would outlive its call; or it is a node of a module body
//!    without a top-level `await`. So no field, element, imported binding,
//!    closure cell, `arguments` object or thrown value ever holds an
//!    instance of `T`: an instance is reachable only from the frames it
//!    was passed down to or returned up to.
//! 2. **`H` hands no `T` down.** No call in `H`'s body has a `T` among its
//!    arguments or spread arguments, or as the receiver of a call that binds
//!    a function (a receiver of a builtin method only is no hand-over: what
//!    such a method does to it is an event of 3). A call the solve made
//!    for no source site counts for every function.
//! 3. **`H` changes no `T`'s length.** The solve records every event that
//!    may change an Array's length or move its elements where the event
//!    reaches the token (`Model::len_event`): an element or `length` store
//!    (`store_into`), an `Object.assign` into it, a builtin method that is
//!    not length-keeping (`builtins::array_method`: `push`, `pop`, `shift`,
//!    `unshift`, `splice`, a computed name), a declared native's element
//!    store, the HIR's own `pop`, a `defineProperty` of an index or
//!    `length`. Each is placed in the function whose body holds its store
//!    site or call; none of `T`'s is in `H`, and none is unplaced.
//! 4. `H` is plain and has no `arguments` object.
//!
//! A program whose solve has a marker (a stand-in for another unit's
//! argument or a contract function's parameter: its values are not in the
//! node that holds it) gets no fact at all, as a per-unit solve gets none.
//!
//! **Why that is enough.** Let `H`'s activation hold instance `I` in `a`.
//! `H` is plain, so until it returns only `H`'s own code and the
//! activations below it on the stack run. `H`'s code changes no length (3).
//! An activation below `H` can change `I` only if it holds `I`. By 1 it can
//! get `I` only from a frame: as an argument or receiver of its call, or as
//! the result of a call it made. `H` hands no `T` down (2), so by induction
//! on the call depth below `H` no such activation ever holds `I`: an Array
//! of token `T` it holds was born below `H` after `I`. Frames above `H` do
//! not run, and no suspended frame holds `I` (1). So `I`'s length, and the
//! place of its elements in its storage, are the same at every point of
//! `H`'s activation after `a` is bound. Recursion needs no further rule: an
//! inner activation of the function that bore `I` grows its own Array.
//!
//! A per-callee effect ("`f` may grow tokens `S`") does not give this: the
//! tokens are per birth site, so `countMoves` calling itself, or `negamax`
//! reaching `orderMoves` again, would "grow" the very token the loop walks.
//! The events of 3 are that per-function effect; rules 1 and 2 are what
//! make it enough to ask it of `H` alone.
//!
//! What moves an Array is not part of the fact: a collection may move `I`,
//! and its length and element offsets move with it. Codegen holds the
//! length (a scalar) and re-reads the Array's address from its root slot
//! (`perry-codegen` `stmt::loops`, `expr::held_len`).
//!
//! `BindingFacts::stable_front`: besides, every `T` has a defined length
//! (`solve::arr_len`: its interval is not ⊤), so its elements start right
//! behind its header for its whole life.
//!
//! `PERRY_FACTS_HELD_LENGTHS=0` records and exports nothing. The negative
//! control `PERRY_FACTS_SABOTAGE=heldlen` claims the fact for every `Arr`
//! local of a plain function. The dump (`PERRY_FACTS_DUMP`) prints one
//! `# held_len` line per `Arr` local of a function: `verdict=held` with
//! `front=0|1`, or `refused=<rule>:<detail>`.

use crate::bindings::{ModuleBindings, Rep};
use crate::gen::Gen;
use crate::model::{FnIdx, Model, NodeId, ThisArg, Tok, TokKind, NO_FN};
use crate::pts::{FxHashMap, FxHashSet};

/// `PERRY_FACTS_HELD_LENGTHS=0` turns the fact off.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_HELD_LENGTHS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// What the rules know of one candidate token.
#[derive(Default)]
struct TokInfo {
    /// Rule 1 or an unplaced event or hand-over: no function may hold it.
    /// The first few distinct reasons (the probe prints them all).
    bad: Vec<String>,
    /// Functions with an event on it (rule 3; `NO_FN`: module scope).
    events: Vec<FnIdx>,
    /// Functions that hand it down (rule 2).
    passed: Vec<FnIdx>,
}

impl TokInfo {
    fn refuse(&mut self, why: String) {
        if self.bad.len() < 8 && !self.bad.contains(&why) {
            self.bad.push(why);
        }
    }
}

/// Nodes no body owns that hold a value only while a builtin runs on it:
/// the receiver relay of a builtin method's run (`builtins::array_method`,
/// `builtins::effects`), and the one its species constructor is read
/// through (`builtins::species_from`: `slice` and its kin read
/// `constructor`; what they construct gets a length, never the receiver).
/// What the method does with the receiver is its modeled effect (a store,
/// a result, a callback's argument), judged where it lands.
const RELAYS: [&str; 2] = ["arr-recv", "array-species-receiver"];

/// The function a store site is in; `None` for a site the solve made.
pub(crate) fn site_fn(m: &Model, site: u32) -> Option<FnIdx> {
    let s = m.stores.get(site as usize)?;
    (s.module != u32::MAX && s.desc != "binding").then_some(s.func)
}

/// The function a call is in: its program site's (`Model::origin_of`: a
/// source call itself, the template of a summary instance's copy, the site
/// a builtin's callback call was made for), which is the owner of that
/// call's result node (`Gen::new_call`; `NO_FN`: module scope). `None` for
/// a call the solve made for no site (an accessor's, a bound function's).
pub(crate) fn call_fn(m: &Model, call: u32) -> Option<FnIdx> {
    let ret = m.calls.get(m.origin_of(call)? as usize)?.ret;
    (*m.labels.get(ret as usize)? == "callret").then(|| m.node_fn[ret as usize])
}

/// The HIR functions that are plain (rule 1), by `(module, func id)`.
fn plain_bodies(g: &Gen<'_>) -> FxHashSet<(u32, u32)> {
    let mut out = FxHashSet::default();
    for (mi, (_, module)) in g.modules.iter().enumerate() {
        let classes = module.classes.iter().flat_map(|c| {
            c.constructor
                .iter()
                .chain(&c.methods)
                .chain(&c.static_methods)
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
        });
        for f in module.functions.iter().chain(classes) {
            let lowered_generator = f.body.iter().any(|s| {
                matches!(
                    s,
                    perry_hir::Stmt::Return(Some(perry_hir::Expr::LinkGeneratorPrototype { .. }))
                )
            });
            if !(f.is_async || f.is_generator || f.was_plain_async || lowered_generator) {
                out.insert((mi as u32, f.id));
            }
        }
    }
    out
}

/// Function `f`'s frame ends with its call (rule 1).
fn plain(m: &Model, bodies: &FxHashSet<(u32, u32)>, f: FnIdx) -> bool {
    let Some(fi) = m.funcs.get(f as usize) else {
        return false;
    };
    fi.func_id != u32::MAX
        && fi.gen_proto.is_none()
        && fi.gen_fn_protos.is_none()
        && !fi.gen_async
        && bodies.contains(&(fi.module, fi.func_id))
}

/// The function whose frame node `n` is, when it is one (rule 1): a node a
/// body owns, or a summary instance's slot.
fn frame_of(m: &Model, n: usize) -> Option<FnIdx> {
    match m.node_fn[n] {
        NO_FN => {
            let frame = m.nodes[n].frame;
            (m.labels[n] == "inst" && frame != 0)
                .then(|| m.sum.insts.get(frame as usize - 1).map(|i| i.f))
                .flatten()
        }
        f => Some(f),
    }
}

/// The module bodies (`<init>`) that run once from start to end: no
/// top-level `await`.
fn module_bodies(g: &Gen<'_>) -> FxHashSet<FnIdx> {
    g.init_fn_of
        .iter()
        .filter(|(mi, _)| {
            g.modules
                .get(**mi as usize)
                .is_some_and(|(_, module)| !module.has_top_level_await)
        })
        .map(|(_, f)| *f)
        .collect()
}

/// Fill `BindingFacts::stable_length` and `stable_front` (module docs).
pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: bool,
    mut probe: Option<&mut Vec<String>>,
) {
    let m = &g.m;
    if !m.held_lengths || m.aborted || m.unit_mode || interpreted {
        return;
    }
    // A marker stands for values that are not in the node that holds it
    // (another unit's argument, a contract function's parameter): rule 1
    // could not see such a holder. No program with one gets the fact.
    if !m.param_marker.is_empty() || m.arg_bits.iter().any(|&w| w != 0) {
        if let Some(p) = probe.as_deref_mut() {
            p.push("# held_len off=markers".to_string());
        }
        return;
    }
    let bodies = plain_bodies(g);
    // The candidates: an `Arr` local of a plain function.
    struct Cand {
        module: usize,
        at: usize,
        local: u32,
        f: FnIdx,
        toks: Vec<Tok>,
        early: Option<&'static str>,
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut info: FxHashMap<Tok, TokInfo> = FxHashMap::default();
    // A local of a summarized function holds, in each instance of the
    // summary, that instance's values (an Array a literal of the body
    // bears per instance is its own token, and only the instance's copy of
    // the local holds it): the local's tokens are those of its node and of
    // every instance copy.
    let mut copies: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    for (mi, mb) in bindings.iter().enumerate().take(g.mods.len()) {
        for (at, (id, b)) in mb.locals.iter().enumerate() {
            if !matches!(b.rep, Rep::Arr(_)) {
                continue;
            }
            let Some(&n) = g.mods[mi].locals.get(id) else {
                continue;
            };
            let f = m.node_fn[n as usize];
            if f == NO_FN {
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
            let early = if toks.is_empty() {
                Some("no-array")
            } else if !plain(m, &bodies, f) {
                Some("function:not-plain")
            } else if m.funcs[f as usize].has_arguments {
                Some("function:arguments")
            } else {
                None
            };
            if early.is_none() {
                for &t in &toks {
                    info.entry(t).or_default();
                }
            }
            cands.push(Cand {
                module: mi,
                at,
                local: *id,
                f,
                toks,
                early,
            });
        }
    }
    if cands.is_empty() {
        return;
    }
    if !info.is_empty() && !sabotage {
        rule_holders(m, &bodies, &module_bodies(g), &mut info);
        rule_events(m, &mut info);
        rule_calls(m, &mut info);
    }
    for c in cands {
        let refusal = c.early.map(str::to_string).or_else(|| {
            if sabotage {
                return None;
            }
            c.toks.iter().find_map(|t| {
                let i = &info[t];
                if !i.bad.is_empty() {
                    Some(format!("{}:tok{t}", i.bad.join("+")))
                } else if i.events.contains(&c.f) {
                    Some(format!("event:own:tok{t}"))
                } else if i.passed.contains(&c.f) {
                    Some(format!("passed:own:tok{t}"))
                } else {
                    None
                }
            })
        });
        let front = c.toks.iter().all(|&t| defined(m, t));
        if refusal.is_none() {
            let b = &mut bindings[c.module].locals[c.at].1;
            b.stable_length = true;
            b.stable_front = front;
        }
        if let Some(p) = probe.as_deref_mut() {
            let name = &m.funcs[c.f as usize].name;
            p.push(match refusal {
                None => format!(
                    "# held_len module={} local={} fn={} verdict=held front={} toks={}",
                    c.module,
                    c.local,
                    name.replace(' ', "_"),
                    u8::from(front),
                    c.toks.len()
                ),
                Some(why) => format!(
                    "# held_len module={} local={} fn={} refused={why}",
                    c.module,
                    c.local,
                    name.replace(' ', "_")
                ),
            });
        }
    }
}

/// Token `t`'s length is defined (`solve::arr_len`): its interval is not ⊤,
/// or no rule touched it and its birth recorded one.
fn defined(m: &Model, t: Tok) -> bool {
    match m.arr_len.get(&t) {
        Some(&ln) => {
            let r = m.nodes[ln as usize].range;
            !r.is_top() && !r.is_empty()
        }
        None => matches!(m.toks[t as usize], TokKind::Obj(tm, _)
            if m.arr_birth_len.contains_key(&tm)),
    }
}

/// Rule 1: every holder of a candidate token is a frame node of a plain
/// function that no other function references.
fn rule_holders(
    m: &Model,
    bodies: &FxHashSet<(u32, u32)>,
    inits: &FxHashSet<FnIdx>,
    info: &mut FxHashMap<Tok, TokInfo>,
) {
    let captured: FxHashSet<NodeId> = m.captures.iter().map(|&(n, _)| n).collect();
    let mut plain_fn: FxHashMap<FnIdx, bool> = FxHashMap::default();
    // A shared set is judged once per verdict of its holder.
    let mut seen: FxHashSet<usize> = FxHashSet::default();
    let mut buf: Vec<u32> = Vec::new();
    for (n, node) in m.nodes.iter().enumerate() {
        if node.pts.is_empty() {
            continue;
        }
        let refused = if captured.contains(&(n as NodeId)) {
            Some("captured".to_string())
        } else {
            match frame_of(m, n) {
                None if RELAYS.contains(&m.labels[n]) => None,
                None => Some("heap".to_string()),
                // A module body runs once from start to end: its nodes
                // are frame nodes (a binding another function reads is
                // `captured`, one another module reads is in its import
                // node, a heap node).
                Some(f) if inits.contains(&f) => None,
                Some(f) => (!*plain_fn.entry(f).or_insert_with(|| plain(m, bodies, f)))
                    .then(|| format!("suspending({})", m.funcs[f as usize].name.replace(' ', "_"))),
            }
        };
        let Some(why) = refused else {
            continue;
        };
        if let Some(id) = node.pts.storage_id() {
            if !seen.insert(id) {
                continue;
            }
        }
        buf.clear();
        node.pts.sorted_into(&mut buf);
        for t in &buf {
            if let Some(i) = info.get_mut(t) {
                i.refuse(format!("{why}:{}", m.labels[n]));
            }
        }
    }
}

/// Rule 3: where each candidate token's length events are.
fn rule_events(m: &Model, info: &mut FxHashMap<Tok, TokInfo>) {
    for &(t, at, call, _) in &m.len_events {
        let Some(i) = info.get_mut(&t) else {
            continue;
        };
        let f = match (at, call) {
            (u32::MAX, _) => None,
            (_, true) => call_fn(m, at),
            (_, false) => site_fn(m, at),
        };
        match f {
            Some(f) => {
                if !i.events.contains(&f) {
                    i.events.push(f);
                }
            }
            None => i.refuse(format!(
                "event:unplaced:{}",
                if call { "call" } else { "site" }
            )),
        }
    }
}

/// Rule 2: which functions hand each candidate token down.
fn rule_calls(m: &Model, info: &mut FxHashMap<Tok, TokInfo>) {
    // Calls that bind a function (or an unknown callee): their receiver is
    // handed over too.
    let mut binds: FxHashSet<u32> = m.bind_done.iter().map(|&(c, _, _)| c).collect();
    binds.extend(m.sum.composed.iter().map(|&(c, _)| c));
    let mut seen: FxHashSet<(usize, Option<FnIdx>)> = FxHashSet::default();
    let mut buf: Vec<u32> = Vec::new();
    let hand = |info: &mut FxHashMap<Tok, TokInfo>, t: Tok, by: Option<FnIdx>, c: usize| {
        let Some(i) = info.get_mut(&t) else {
            return;
        };
        match by {
            Some(f) => {
                if !i.passed.contains(&f) {
                    i.passed.push(f);
                }
            }
            None => i.refuse(format!(
                "passed:unplaced:{}",
                m.labels
                    .get(m.calls[c].ret as usize)
                    .copied()
                    .unwrap_or("call")
            )),
        }
    };
    for (c, cs) in m.calls.iter().enumerate() {
        let by = call_fn(m, c as u32);
        let bound = binds.contains(&(c as u32));
        let this = match cs.this {
            ThisArg::Node(n) if bound => Some(n),
            ThisArg::Tok(t) => {
                if bound {
                    hand(info, t, by, c);
                }
                None
            }
            _ => None,
        };
        for &n in cs.args.iter().chain(&cs.spread).chain(this.iter()) {
            let Some(node) = m.nodes.get(n as usize) else {
                continue;
            };
            if node.pts.is_empty() {
                continue;
            }
            if let Some(id) = node.pts.storage_id() {
                if !seen.insert((id, by)) {
                    continue;
                }
            }
            buf.clear();
            node.pts.sorted_into(&mut buf);
            for &t in &buf {
                hand(info, t, by, c);
            }
        }
    }
}
