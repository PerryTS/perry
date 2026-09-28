//! Function-entry GC polls — RFC deferred collection step S5, decision 2.
//!
//! Loop back-edge polls bound allocation in ITERATION. Two other kinds of
//! repetition allocate without passing a back-edge, and each gets a poll here:
//!
//! 1. **Indirect entry** — closures, class methods (instance, static,
//!    constructors, accessors) and the `__perry_wrap_*` forwarders that make a
//!    top-level function a callable value. A runtime helper that loops over a
//!    callback (`sort`, `forEach`, `Array.from` with a map function, an
//!    iterator protocol) enters the callback indirectly, once per element,
//!    and nothing else polls between those entries.
//! 2. **Recursion** — one entry poll per recursive SCC of the module's direct
//!    call graph, in the member with the most incoming edges from inside the
//!    SCC. `function t(d) { return d ? {l: t(d-1), r: t(d-1)} : null }`
//!    allocates 2^d objects with no loop at all.
//!
//! NOT a poll at every function entry: the census measured that at 12.18 M
//! relocations against 7.39 M for this placement (RFC §2).
//!
//! # How a poll is placed, and why it cannot change a safepoint
//!
//! Lowering emits the poll's SCAFFOLD at a candidate's entry, after its
//! parameters are rooted and before the first statement (the same point the
//! `arguments` object is materialised, which can itself allocate):
//!
//! ```llvm
//!   %armed = load volatile i32, ptr @PERRY_GC_POLL_ARMED
//!   %due = icmp ne i32 %armed, 0
//!   br i1 %due, label %gcentry, label %gcentry.done
//! gcentry:
//!   call void @js_gc_entry_safepoint()
//!   br label %gcentry.done
//! ```
//!
//! [`finalize_module`] then decides which scaffolds live, on the lowered IR:
//!
//! * The module's leaf set is computed with entry polls IGNORED
//!   (`gc_call_effects::transitive_leaf_functions` skips [`ENTRY_POLL`]).
//! * A scaffold is KEPT only in a function outside that set. Such a function
//!   already reaches the collector through some other edge, so every call to
//!   it was a statepoint before and still is: adding the poll changes no
//!   call's classification (S5 must not; that is S6's job).
//! * A kept Direct scaffold additionally requires that its function is the
//!   chosen representative of a recursive SCC.
//! * A dropped scaffold loses its call and its load becomes the constant 0, so
//!   LLVM folds the branch away and the function pays nothing.
//!
//! The kill switch is the loop polls' own: `PERRY_GC_MOVING_LOOP_POLLS=0`
//! emits no scaffold at all.

use std::collections::{HashMap, HashSet};

use crate::expr::FnCtx;
use crate::function::LlFunction;
use crate::inst::LlInst;
use crate::types::{DOUBLE, I32, I64, PTR};

/// The runtime poll entry an entry poll calls. Same body as the back-edge
/// poll (`js_gc_loop_safepoint`) — a distinct symbol only so the module leaf
/// analysis can tell an entry poll apart and ignore it.
pub(crate) const ENTRY_POLL: &str = "js_gc_entry_safepoint";
/// The forwarding-wrapper variant: the wrapper spills its arguments to a
/// buffer, and the runtime roots them across the collection and writes the
/// possibly-relocated values back (a wrapper has no root slots of its own).
pub(crate) const ENTRY_POLL_ARGS: &str = "js_gc_entry_safepoint_args";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryPollKind {
    /// Entered indirectly (closure body, method, value wrapper): kept whenever
    /// the function is not a proven leaf.
    Indirect,
    /// A top-level function entered directly: kept only as the poll of its
    /// recursive SCC.
    Direct,
}

/// Where a scaffold lives in its function, for [`finalize_module`].
#[derive(Clone, Debug)]
pub(crate) struct EntryPollSite {
    pub(crate) kind: EntryPollKind,
    load_block: usize,
    load_dst: String,
    call_block: usize,
}

/// Whether entry polls are emitted at all — the loop polls' switch.
pub(crate) fn entry_polls_enabled() -> bool {
    crate::stmt::moving_safepoint_polls_enabled()
}

/// Emit an entry-poll scaffold at the current point of `ctx`'s function,
/// unless the body provably cannot allocate (then it needs no poll: a
/// function that allocates nothing adds no pressure between polls, however
/// often it is entered).
pub(crate) fn emit_entry_poll(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    kind: EntryPollKind,
) {
    if !entry_polls_enabled() || ctx.block().is_terminated() || ctx.func.entry_poll.is_some() {
        return;
    }
    let may_allocate = {
        let is_inert = |e: &perry_hir::Expr| crate::rooting::expr_is_inert_primitive(ctx, e);
        crate::loop_purity::loop_may_allocate(body, &[], &is_inert)
    };
    if !may_allocate {
        return;
    }
    let poll_idx = ctx.new_block("gcentry");
    let done_idx = ctx.new_block("gcentry.done");
    let poll_label = ctx.block_label(poll_idx);
    let done_label = ctx.block_label(done_idx);
    let load_block = ctx.current_block;
    let load_dst = {
        let blk = ctx.block();
        let armed = blk.load_volatile(I32, "@PERRY_GC_POLL_ARMED");
        let due = blk.icmp_ne(I32, &armed, "0");
        blk.cond_br(&due, &poll_label, &done_label);
        armed
    };
    ctx.current_block = poll_idx;
    {
        let blk = ctx.block();
        blk.call_void(ENTRY_POLL, &[]);
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
    ctx.func.entry_poll = Some(EntryPollSite {
        kind,
        load_block,
        load_dst,
        call_block: poll_idx,
    });
}

/// The `__perry_wrap_*` forwarder's entry poll. `args` are the wrapper's own
/// parameters (`%this_closure` first, as `i64`, then the `double`s); returns
/// the values the forwarded call must use — relocated when the poll ran.
///
/// Only the armed path touches memory: it spills the arguments to an entry
/// buffer, calls [`ENTRY_POLL_ARGS`], and reloads them. The unarmed path is
/// the load and the branch, like every other poll.
pub(crate) fn emit_wrapper_entry_poll(
    func: &mut LlFunction,
    closure_param: &str,
    double_params: &[String],
) -> (String, Vec<String>) {
    let count = double_params.len() + 1;
    let buffer = func.alloca_entry_array(I64, count);
    let entry_label = func
        .blocks()
        .first()
        .map(|b| b.label.clone())
        .expect("wrapper has an entry block");
    let poll_label = func.create_block("gcentry").label.clone();
    let done_label = func.create_block("gcentry.done").label.clone();
    let (poll_idx, done_idx) = (func.num_blocks() - 2, func.num_blocks() - 1);
    let load_dst = {
        let blk = func.block_mut(0).expect("entry");
        let armed = blk.load_volatile(I32, "@PERRY_GC_POLL_ARMED");
        let due = blk.icmp_ne(I32, &armed, "0");
        blk.cond_br(&due, &poll_label, &done_label);
        armed
    };
    let mut reloaded = Vec::with_capacity(count);
    {
        let blk = func.block_mut(poll_idx).expect("poll block");
        // Slot 0: the closure pointer, NaN-boxed as an object so the runtime
        // can root (and rewrite) it like any other value.
        let boxed = blk.or(I64, closure_param, crate::nanbox::POINTER_TAG_I64);
        let slot0 = blk.gep(I64, &buffer, &[(I64, "0")]);
        blk.store(I64, &boxed, &slot0);
        for (i, param) in double_params.iter().enumerate() {
            let slot = blk.gep(I64, &buffer, &[(I64, &(i + 1).to_string())]);
            blk.store(DOUBLE, param, &slot);
        }
        blk.call_void(ENTRY_POLL_ARGS, &[(PTR, &buffer), (I32, &count.to_string())]);
        let slot0 = blk.gep(I64, &buffer, &[(I64, "0")]);
        let bits = blk.load(I64, &slot0);
        reloaded.push(blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64));
        for i in 0..double_params.len() {
            let slot = blk.gep(I64, &buffer, &[(I64, &(i + 1).to_string())]);
            reloaded.push(blk.load(DOUBLE, &slot));
        }
        blk.br(&done_label);
    }
    let mut values = Vec::with_capacity(count);
    {
        let blk = func.block_mut(done_idx).expect("done block");
        let closure = blk.phi(
            I64,
            &[(closure_param, &entry_label), (&reloaded[0], &poll_label)],
        );
        values.push(closure);
        for (i, param) in double_params.iter().enumerate() {
            values.push(blk.phi(
                DOUBLE,
                &[(param, &entry_label), (&reloaded[i + 1], &poll_label)],
            ));
        }
    }
    func.entry_poll = Some(EntryPollSite {
        kind: EntryPollKind::Indirect,
        load_block: 0,
        load_dst,
        call_block: poll_idx,
    });
    let closure = values.remove(0);
    (closure, values)
}

/// Statistics of one module's finalisation, for tests and the report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EntryPollStats {
    pub(crate) scaffolds: usize,
    pub(crate) kept_indirect: usize,
    pub(crate) kept_scc: usize,
    pub(crate) recursive_sccs: usize,
    /// Allocating recursive SCCs none of whose members had a scaffold.
    pub(crate) uncovered_sccs: usize,
}

/// Decide which scaffolds live (see the module docs), and neutralise the rest.
///
/// A recursive SCC gets a poll only when the recursion itself allocates
/// without one: some member has a collecting edge of its own, or calls a
/// function outside the SCC that may collect and has no poll at its entry. A
/// callee that polls at entry bounds its own allocation per call, so a
/// recursion that only reaches allocation through such callees needs nothing
/// more — which is what keeps a numeric recursion whose only collecting edge
/// is a cold fallback into its generic clone (a specialized-ABI `fib` clone falling
/// back to the boxed `fib`) free of a
/// per-call poll. SCCs are visited callees-first, so a callee's poll is known
/// before its callers are decided.
pub(crate) fn finalize_module(functions: &mut [&mut LlFunction]) -> EntryPollStats {
    let mut stats = EntryPollStats::default();
    if !functions.iter().any(|f| f.entry_poll.is_some()) {
        return stats;
    }
    let (leaf, graph) = {
        let refs: Vec<&LlFunction> = functions.iter().map(|f| &**f).collect();
        (
            crate::gc_call_effects::transitive_leaf_functions(&refs),
            crate::gc_call_effects::direct_call_graph(&refs),
        )
    };
    let scaffold: HashMap<String, EntryPollKind> = functions
        .iter()
        .filter_map(|f| f.entry_poll.as_ref().map(|site| (f.name.clone(), site.kind)))
        .collect();
    let edges: HashMap<String, HashSet<String>> = graph
        .iter()
        .map(|(name, (callees, _))| (name.clone(), callees.clone()))
        .collect();
    // Indirect-entry scaffolds live in every non-leaf function.
    let mut polled: HashSet<String> = scaffold
        .iter()
        .filter(|(name, kind)| **kind == EntryPollKind::Indirect && !leaf.contains(*name))
        .map(|(name, _)| name.clone())
        .collect();
    let mut scc_reps: HashSet<String> = HashSet::new();
    for scc in sccs_callees_first(&edges) {
        let members: HashSet<&str> = scc.iter().map(String::as_str).collect();
        let recursive = scc.len() > 1 || edges.get(&scc[0]).is_some_and(|c| c.contains(&scc[0]));
        if !recursive {
            continue;
        }
        stats.recursive_sccs += 1;
        if scc.iter().any(|name| polled.contains(name)) {
            // An indirect-entry poll inside the SCC already covers it.
            for name in &scc {
                polled.insert(name.clone());
            }
            continue;
        }
        let allocates_uncovered = scc.iter().any(|name| {
            let Some((callees, collecting)) = graph.get(name) else {
                return false;
            };
            *collecting
                || callees.iter().any(|callee| {
                    !members.contains(callee.as_str())
                        && !leaf.contains(callee)
                        && !polled.contains(callee)
                })
        });
        if !allocates_uncovered {
            continue;
        }
        let rep = scc
            .iter()
            .filter(|name| scaffold.contains_key(name.as_str()))
            .max_by_key(|name| {
                let incoming = edges
                    .iter()
                    .filter(|(caller, callees)| {
                        members.contains(caller.as_str()) && callees.contains(name.as_str())
                    })
                    .count();
                // Deterministic tie-break: lexicographically smallest name.
                (incoming, std::cmp::Reverse((*name).clone()))
            });
        if let Some(rep) = rep {
            scc_reps.insert(rep.clone());
            for name in &scc {
                polled.insert(name.clone());
            }
        } else {
            stats.uncovered_sccs += 1;
        }
    }
    for function in functions.iter_mut() {
        let Some(site) = function.entry_poll.clone() else {
            continue;
        };
        stats.scaffolds += 1;
        let keep = !leaf.contains(&function.name)
            && match site.kind {
                EntryPollKind::Indirect => true,
                EntryPollKind::Direct => scc_reps.contains(&function.name),
            };
        if keep {
            match site.kind {
                EntryPollKind::Indirect => stats.kept_indirect += 1,
                EntryPollKind::Direct => stats.kept_scc += 1,
            }
            continue;
        }
        neutralise(function, &site);
    }
    crate::statepoint_report::note_entry_polls(&stats);
    stats
}

/// Drop a scaffold: its load becomes the constant 0 (LLVM folds the branch)
/// and its poll call disappears.
fn neutralise(function: &mut LlFunction, site: &EntryPollSite) {
    if let Some(block) = function.block_mut(site.load_block) {
        for inst in block.insts_mut().iter_mut() {
            if matches!(inst, LlInst::Load { dst, .. } if *dst == site.load_dst) {
                *inst = LlInst::Bin {
                    dst: site.load_dst.clone(),
                    op: "add",
                    pre: "",
                    ty: I32,
                    a: "0".to_string(),
                    b: "0".to_string(),
                };
                break;
            }
        }
    }
    if let Some(block) = function.block_mut(site.call_block) {
        block.insts_mut().retain(|inst| {
            !matches!(inst, LlInst::Call { callee, .. } if callee == ENTRY_POLL || callee == ENTRY_POLL_ARGS)
        });
    }
    function.entry_poll = None;
}

/// Every strongly connected component of `graph` (Tarjan), callees first
/// (reverse topological order). Names sorted within a component.
pub(crate) fn sccs_callees_first(graph: &HashMap<String, HashSet<String>>) -> Vec<Vec<String>> {
    struct State<'g> {
        graph: &'g HashMap<String, HashSet<String>>,
        index: HashMap<&'g str, usize>,
        low: HashMap<&'g str, usize>,
        on_stack: HashSet<&'g str>,
        stack: Vec<&'g str>,
        next: usize,
        out: Vec<Vec<String>>,
    }
    // Iterative Tarjan: a generated module can have call chains deep enough
    // to overflow a recursive walk.
    fn strongconnect<'g>(state: &mut State<'g>, root: &'g str) {
        let mut work: Vec<(&'g str, Vec<&'g str>, usize)> = Vec::new();
        let succ = |state: &State<'g>, v: &'g str| -> Vec<&'g str> {
            let mut s: Vec<&'g str> = state
                .graph
                .get(v)
                .map(|c| c.iter().map(String::as_str).filter(|w| state.graph.contains_key(*w)).collect())
                .unwrap_or_default();
            s.sort_unstable();
            s
        };
        state.index.insert(root, state.next);
        state.low.insert(root, state.next);
        state.next += 1;
        state.stack.push(root);
        state.on_stack.insert(root);
        let root_succ = succ(state, root);
        work.push((root, root_succ, 0));
        while let Some((v, succs, i)) = work.pop() {
            if i < succs.len() {
                let w = succs[i];
                work.push((v, succs, i + 1));
                if !state.index.contains_key(w) {
                    state.index.insert(w, state.next);
                    state.low.insert(w, state.next);
                    state.next += 1;
                    state.stack.push(w);
                    state.on_stack.insert(w);
                    let w_succ = succ(state, w);
                    work.push((w, w_succ, 0));
                } else if state.on_stack.contains(w) {
                    let low = state.low[v].min(state.index[w]);
                    state.low.insert(v, low);
                }
                continue;
            }
            if let Some((parent, _, _)) = work.last() {
                let low = state.low[*parent].min(state.low[v]);
                state.low.insert(parent, low);
            }
            if state.low[v] == state.index[v] {
                let mut component = Vec::new();
                while let Some(w) = state.stack.pop() {
                    state.on_stack.remove(w);
                    component.push(w.to_string());
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                state.out.push(component);
            }
        }
    }
    let mut state = State {
        graph,
        index: HashMap::new(),
        low: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    let mut roots: Vec<&str> = graph.keys().map(String::as_str).collect();
    roots.sort_unstable();
    for root in roots {
        if !state.index.contains_key(root) {
            strongconnect(&mut state, root);
        }
    }
    state.out
}

/// The recursive SCCs of `graph`: components with more than one member, or a
/// single member that calls itself. Sorted, for tests.
#[cfg(test)]
pub(crate) fn recursive_sccs(graph: &HashMap<String, HashSet<String>>) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = sccs_callees_first(graph)
        .into_iter()
        .filter(|scc| scc.len() > 1 || graph.get(&scc[0]).is_some_and(|c| c.contains(&scc[0])))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &str)], nodes: &[&str]) -> HashMap<String, HashSet<String>> {
        let mut g: HashMap<String, HashSet<String>> =
            nodes.iter().map(|n| (n.to_string(), HashSet::new())).collect();
        for (a, b) in edges {
            g.entry(a.to_string()).or_default().insert(b.to_string());
        }
        g
    }

    #[test]
    fn recursive_sccs_finds_self_loops_and_cycles_but_not_chains() {
        let g = graph(
            &[("a", "a"), ("b", "c"), ("c", "b"), ("d", "e"), ("e", "f")],
            &["a", "b", "c", "d", "e", "f"],
        );
        assert_eq!(
            recursive_sccs(&g),
            vec![vec!["a".to_string()], vec!["b".to_string(), "c".to_string()]]
        );
    }

    #[test]
    fn sccs_come_callees_first() {
        let g = graph(&[("caller", "rec"), ("rec", "rec"), ("rec", "leaf")], &["caller", "rec", "leaf"]);
        let order = sccs_callees_first(&g);
        let pos = |n: &str| order.iter().position(|c| c.contains(&n.to_string())).unwrap();
        assert!(pos("leaf") < pos("rec") && pos("rec") < pos("caller"), "{order:?}");
    }

    #[test]
    fn recursive_sccs_survives_a_deep_chain() {
        let names: Vec<String> = (0..50_000).map(|i| format!("f{i}")).collect();
        let mut g: HashMap<String, HashSet<String>> = HashMap::new();
        for w in names.windows(2) {
            g.entry(w[0].clone()).or_default().insert(w[1].clone());
        }
        g.entry(names[49_999].clone()).or_default().insert(names[0].clone());
        let sccs = recursive_sccs(&g);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 50_000);
    }
}
