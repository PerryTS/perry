//! Which codegen unit each function of a split module compiles in.
//!
//! A unit is one LLVM module: nothing is inlined across units, and the order
//! of the units is the order of the module's text. Two partitions live here.
//!
//! [`units_by_size`] is the first one (#5391): largest function first, each
//! into the currently smallest unit. It balances the units exactly and knows
//! nothing else, so a caller and its callee meet in one unit by chance, and a
//! function that grows by a few bytes changes its rank and with it the unit
//! of many functions that did not change.
//!
//! [`units_by_locality`] (the default; `PERRY_CODEGEN_UNIT_LOCALITY=0`
//! restores the first) is a function of the call graph:
//!
//! 1. **Clusters.** Functions joined by a reference grow into clusters, the
//!    strongest reference first: a direct call inside a recursion cycle, a
//!    direct call of a function with one caller, any other direct call
//!    (fewest callers first), then an address reference (a closure body
//!    named by the function that creates it), unless the referrer names more
//!    than [`REF_FANOUT_MAX`] functions (a module initializer registers every
//!    method; that says nothing about locality). A cluster stops growing at
//!    the cluster cap, so what does not fit is cut at its weakest reference.
//! 2. **Sequence.** Clusters are ordered by their anchor, the member defined
//!    first; members keep definition order. Definition order is source
//!    order, so a class's methods and a file's neighbours stay adjacent
//!    without reading a name.
//! 3. **Units.** The sequence is cut into `n` consecutive runs. Cut `k` is
//!    taken inside a window around the balanced position `k * total / n`,
//!    at the cluster whose anchor NAME hashes lowest. The window bounds the
//!    imbalance; the hash makes the cut a property of the code that is
//!    there, not of the byte count before it, so an edit that moves every
//!    later position by a few bytes leaves the cuts where they were.
//!
//! A function larger than a balanced unit is a unit of its own, as it was
//! under the size partition. The result depends on the functions' names,
//! sizes, definition order and references only: no ordinal of a previous
//! build, no hash-map iteration order, no state.

use std::collections::HashMap;

/// A referrer naming more functions than this by address is a registrar.
pub(crate) const REF_FANOUT_MAX: usize = 16;

/// The cut window is `total / n / WINDOW_DIV` to either side of the balanced
/// position, so two units differ by at most `4 / WINDOW_DIV` of a unit unless
/// a single function straddles a window.
pub(crate) const WINDOW_DIV: usize = 32;

/// One function as the partition sees it.
pub(crate) struct UnitNode<'a> {
    pub name: &'a str,
    pub bytes: usize,
}

/// One reference from the body of `from` to the function `to`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct UnitEdge {
    pub from: u32,
    pub to: u32,
    /// A direct call (LLVM can inline it) rather than an address reference.
    pub direct: bool,
}

/// `PERRY_CODEGEN_UNIT_LOCALITY` (default on); `=0`/`off`/`false` restores
/// the size partition.
pub(crate) fn locality_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        locality_enabled_from_env(std::env::var("PERRY_CODEGEN_UNIT_LOCALITY").ok().as_deref())
    })
}

pub(crate) fn locality_enabled_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "off" | "false"))
}

/// Greedy largest-first bin packing: each function into the currently
/// smallest of `n` units. Minified bundles have a few enormous functions (a
/// 68 MB IIFE in the cli.js case); this isolates them and keeps the rest even.
pub(crate) fn units_by_size(sizes: &[usize], n: usize) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut bucket_bytes = vec![0usize; n];
    for &i in &order {
        let target = bucket_bytes
            .iter()
            .enumerate()
            .min_by_key(|&(_, &b)| b)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        buckets[target].push(i);
        bucket_bytes[target] += sizes[i];
    }
    buckets
}

/// The cluster cap for a balanced unit of `unit_bytes`: the largest power of
/// two not above half a cut window's width. A power of two so that an edit
/// which moves the module's total by a few bytes does not move the cap.
pub(crate) fn cluster_cap(unit_bytes: usize) -> usize {
    let limit = (unit_bytes / WINDOW_DIV).max(1);
    1usize << (usize::BITS - 1 - limit.leading_zeros())
}

fn name_hash(name: &str) -> u64 {
    // FNV-1a, then a finalizer: names share long prefixes.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

struct Clusters {
    parent: Vec<u32>,
    bytes: Vec<usize>,
}

impl Clusters {
    fn find(&mut self, mut i: u32) -> u32 {
        while self.parent[i as usize] != i {
            let up = self.parent[self.parent[i as usize] as usize];
            self.parent[i as usize] = up;
            i = up;
        }
        i
    }

    /// Join the clusters of `a` and `b` when the result fits `cap`. The root
    /// is the member defined first, which is the cluster's anchor.
    fn join(&mut self, a: u32, b: u32, cap: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b || self.bytes[a as usize] + self.bytes[b as usize] > cap {
            return;
        }
        let (root, child) = if a < b { (a, b) } else { (b, a) };
        self.parent[child as usize] = root;
        self.bytes[root as usize] += self.bytes[child as usize];
    }
}

/// Strongly connected components of the direct-call graph (Tarjan, with an
/// explicit stack: generated call chains are deep). `component[i]` names the
/// component of function `i`.
fn call_cycles(count: usize, callees: &[Vec<u32>]) -> Vec<u32> {
    const NONE: u32 = u32::MAX;
    let mut index = vec![NONE; count];
    let mut low = vec![0u32; count];
    let mut on_stack = vec![false; count];
    let mut component = vec![NONE; count];
    let mut stack: Vec<u32> = Vec::new();
    let mut work: Vec<(u32, usize)> = Vec::new();
    let mut next = 0u32;
    let mut components = 0u32;
    for root in 0..count as u32 {
        if index[root as usize] != NONE {
            continue;
        }
        work.push((root, 0));
        while let Some(&mut (v, ref mut at)) = work.last_mut() {
            let vi = v as usize;
            if *at == 0 {
                index[vi] = next;
                low[vi] = next;
                next += 1;
                stack.push(v);
                on_stack[vi] = true;
            }
            if let Some(&w) = callees[vi].get(*at) {
                *at += 1;
                let wi = w as usize;
                if index[wi] == NONE {
                    work.push((w, 0));
                } else if on_stack[wi] {
                    low[vi] = low[vi].min(index[wi]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent as usize] = low[parent as usize].min(low[vi]);
            }
            if low[vi] == index[vi] {
                while let Some(w) = stack.pop() {
                    on_stack[w as usize] = false;
                    component[w as usize] = components;
                    if w == v {
                        break;
                    }
                }
                components += 1;
            }
        }
    }
    component
}

/// The clusters of step 1, each as its members in definition order, ordered
/// by anchor (step 2).
pub(crate) fn clusters(nodes: &[UnitNode<'_>], edges: &[UnitEdge], cap: usize) -> Vec<Vec<usize>> {
    let count = nodes.len();
    // One edge per ordered pair; a pair with a direct call is a call edge.
    let mut pairs: Vec<UnitEdge> = edges
        .iter()
        .copied()
        .filter(|e| e.from != e.to && (e.from as usize) < count && (e.to as usize) < count)
        .collect();
    pairs.sort_unstable_by_key(|e| (e.from, e.to, !e.direct));
    pairs.dedup_by_key(|e| (e.from, e.to));

    let mut callees: Vec<Vec<u32>> = vec![Vec::new(); count];
    let mut callers = vec![0usize; count];
    let mut ref_fanout = vec![0usize; count];
    for e in &pairs {
        if e.direct {
            callees[e.from as usize].push(e.to);
            callers[e.to as usize] += 1;
        } else {
            ref_fanout[e.from as usize] += 1;
        }
    }
    let cycle = call_cycles(count, &callees);

    // (strength tier, exclusivity, first-defined end, other end).
    let mut ranked: Vec<(u8, usize, u32, u32)> = pairs
        .iter()
        .filter_map(|e| {
            let (lo, hi) = (e.from.min(e.to), e.from.max(e.to));
            let (f, t) = (e.from as usize, e.to as usize);
            if e.direct {
                Some(if cycle[f] == cycle[t] {
                    (0, 0, lo, hi)
                } else if callers[t] == 1 {
                    (1, 0, lo, hi)
                } else {
                    (2, callers[t], lo, hi)
                })
            } else {
                (ref_fanout[f] <= REF_FANOUT_MAX).then_some((3, ref_fanout[f], lo, hi))
            }
        })
        .collect();
    ranked.sort_unstable();

    let mut set = Clusters {
        parent: (0..count as u32).collect(),
        bytes: nodes.iter().map(|n| n.bytes).collect(),
    };
    for &(_, _, a, b) in &ranked {
        set.join(a, b, cap);
    }
    // Roots are the first-defined members, so walking in definition order
    // meets every cluster at its anchor and fills it in definition order.
    let mut slot: HashMap<u32, usize> = HashMap::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..count as u32 {
        let root = set.find(i);
        let at = *slot.entry(root).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[at].push(i as usize);
    }
    out
}

/// Cut `seq` (cluster indices in sequence order) into at most `n` runs
/// (step 3). Returns the index in `seq` each run starts at, the first (0)
/// included.
fn cut_points(seq: &[usize], bytes: &[usize], anchors: &[&str], n: usize) -> Vec<usize> {
    let total: usize = seq.iter().map(|&c| bytes[c]).sum();
    let mut starts = vec![0usize];
    if n <= 1 || seq.len() <= 1 || total == 0 {
        return starts;
    }
    // prefix[i] = bytes before seq[i].
    let mut prefix = Vec::with_capacity(seq.len());
    let mut acc = 0usize;
    for &c in seq {
        prefix.push(acc);
        acc += bytes[c];
    }
    let window = total / n / WINDOW_DIV;
    let mut from = 1usize;
    for k in 1..n {
        if from >= seq.len() {
            break;
        }
        let ideal = (total as u128 * k as u128 / n as u128) as usize;
        let (lo, hi) = (ideal.saturating_sub(window), ideal + window);
        let first = from + prefix[from..].partition_point(|&p| p < lo);
        let end = from + prefix[from..].partition_point(|&p| p <= hi);
        let cut = if first < end {
            (first..end)
                .min_by_key(|&i| (name_hash(anchors[seq[i]]), i))
                .unwrap_or(first)
        } else if first >= seq.len() {
            // Everything left starts before the window: the last unit takes it.
            break;
        } else if first > from && ideal - prefix[first - 1] <= prefix[first] - ideal {
            // No cluster starts inside the window (one function straddles
            // it): the nearer of the two starts around it.
            first - 1
        } else {
            first
        };
        starts.push(cut);
        from = cut + 1;
    }
    starts
}

/// The locality partition: at most `n` units, each a list of function
/// indices, in the order the units and their functions are emitted.
pub(crate) fn units_by_locality(
    nodes: &[UnitNode<'_>],
    edges: &[UnitEdge],
    n: usize,
) -> Vec<Vec<usize>> {
    let n = n.clamp(1, nodes.len().max(1));
    let total: usize = nodes.iter().map(|f| f.bytes).sum();
    let unit_bytes = total.div_ceil(n).max(1);
    let groups = clusters(nodes, edges, cluster_cap(unit_bytes));
    let bytes: Vec<usize> = groups
        .iter()
        .map(|g| g.iter().map(|&i| nodes[i].bytes).sum())
        .collect();
    let anchors: Vec<&str> = groups.iter().map(|g| nodes[g[0]].name).collect();

    // A cluster larger than a balanced unit (one irreducible function) is a
    // unit of its own; the others share the units that remain.
    let (giants, rest): (Vec<usize>, Vec<usize>) =
        (0..groups.len()).partition(|&c| n > 1 && bytes[c] > unit_bytes);
    let rest_units = n.saturating_sub(giants.len()).max(1);
    let starts = cut_points(&rest, &bytes, &anchors, rest_units);

    let mut units: Vec<Vec<usize>> = Vec::with_capacity(starts.len() + giants.len());
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(rest.len());
        let unit: Vec<usize> = rest[start..end]
            .iter()
            .flat_map(|&c| groups[c].iter().copied())
            .collect();
        if !unit.is_empty() {
            units.push(unit);
        }
    }
    units.extend(giants.iter().map(|&c| groups[c].clone()));
    // Unit order is anchor order: a unit's position names where it starts in
    // the source, not how large it is.
    units.sort_by_key(|unit| unit.iter().copied().min());
    units
}

#[cfg(test)]
#[path = "unit_locality_tests.rs"]
mod tests;
