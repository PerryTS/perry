//! The birth capacity of a push-grown list (`ModuleBindings::list_capacities`).
//!
//! A **list birth** is a `let` / `const` whose initializer is the empty
//! array literal (`const moves = []`). Such an Array is born with four
//! slots, and every `push` past its capacity grows the storage (at least 16
//! more slots, doubling later: the runtime's `grown_capacity`). The facts
//! decide how many slots the birth reserves instead, from the push sites
//! that can reach the Array.
//!
//! **What the facts hold.** Every `push` of the program (the folded
//! `Expr::ArrayPush` / `ArrayPushSpread` of `Gen::folded_call`, and any
//! `x.push(..)` method call of `Gen::call_with`) is recorded where it is
//! generated with its receiver node, the body it
//! stands in and its loop depth in that body (`Gen::list_pushes`); every
//! list birth with its template, body and loop depth (`Gen::list_births`).
//! After the solve a push site *reaches* a birth when its receiver (any copy
//! of it, `rest_elems::node_copies`) holds an Array of the birth's template:
//! the push may be in another function (`addPawn(moves, ..)` pushes into its
//! parameter), through an alias, a field or a closure. Each reaching site is
//! one of:
//! - **repeated**: in the birth's body inside a loop the birth is not in
//!   (deeper loop depth), or in another body inside a loop of that body. It
//!   may run any number of times per Array;
//! - **single**: in the birth's body at the birth's loop depth, with a
//!   fixed number of arguments `k`. Through the birth's own binding it
//!   runs at most once per Array (the only way back to it is a back edge
//!   that also runs the birth again), so it appends at most `k` elements;
//!   an alias that comes back to an older Array is not counted, and that
//!   Array then grows as any other;
//! - **open**: anything else (another body outside its loops, whose call
//!   count is unknown; a spread push at the birth's depth; a site outside
//!   the birth's loops).
//!
//! **The rule.** With `C` = [`DEFAULT_SLOTS`]:
//! - a repeated site reaches the birth: its length has no static bound
//!   (`solve::arr_len` gives such an Array the hull `[0, u32::MAX]`: a rule
//!   runs once per token, not once per append), and the birth reserves `C`
//!   slots;
//! - only single sites reach it, appending at most `N` elements in all: `N`
//!   is the bound, and the birth reserves `min(N, C)` slots when `N > 4`
//!   (four is what every empty birth has);
//! - otherwise (no site, or an open one and no repeated one) nothing is
//!   reserved.
//!
//! The capacity of an Array is not observable: a push past it grows the
//! storage as before, and a capacity never read is only memory. So no proof
//! is consumed here and nothing can be falsified at run time; a missed or
//! spurious site costs slots or a growth, never an answer. The cost is
//! bounded by the cap: a reserving birth holds at most `C` slots, `(C - 4)
//! * 8` bytes more than the four-slot birth (288 bytes at 40) and `(C - 16)
//! * 8` more than the sixteen-slot birth codegen's own collector gives a
//! local pushed in a loop of its body (192 bytes at 40), and that only for
//! an Array that stays short.
//!
//! `C` itself is a growth-policy constant, as the runtime's "at least 16
//! more slots" is: for an unbounded list the facts give the class, not a
//! number. `PERRY_LIST_BIRTH_CAPACITY=<n>` (5..=[`MAX_SLOTS`]) sets it for
//! measurements; `0` (also `off`, `false`) exports nothing: main's births.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::{NodeId, TmplId};
use crate::pts::FxHashMap;

/// The slots a list birth reserves when a repeated push reaches it.
pub(crate) const DEFAULT_SLOTS: u32 = 40;
/// The most slots `PERRY_LIST_BIRTH_CAPACITY` may ask for: the birth is one
/// inline bump allocation whose slots are stored one by one.
pub(crate) const MAX_SLOTS: u32 = 64;
/// The slots every empty birth has (the runtime's `MIN_ARRAY_CAPACITY`).
const EMPTY_SLOTS: u32 = 4;

/// The cap `C` (module docs), or `None` with the switch off.
pub fn slots() -> Option<u32> {
    match std::env::var("PERRY_LIST_BIRTH_CAPACITY").as_deref() {
        Ok("0") | Ok("off") | Ok("false") => None,
        Ok(v) => Some(
            v.parse::<u32>()
                .ok()
                .filter(|n| (EMPTY_SLOTS + 1..=MAX_SLOTS).contains(n))
                .unwrap_or(DEFAULT_SLOTS),
        ),
        Err(_) => Some(DEFAULT_SLOTS),
    }
}

/// A list birth (module docs): `let local = []` of `module`, its template,
/// the body it stands in and its loop depth there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Birth {
    pub module: u32,
    pub local: u32,
    pub tmpl: TmplId,
    pub body: u32,
    pub depth: u32,
}

/// A `push` site: its receiver node, body and loop depth, and the number
/// of elements it appends (`None`: a spread).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Push {
    pub recv: NodeId,
    pub body: u32,
    pub depth: u32,
    pub count: Option<u32>,
}

/// What the push sites reaching one birth say (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reach {
    pub sites: u32,
    pub repeated: bool,
    pub open: bool,
    /// The elements the single sites append at most.
    pub single: u32,
}

impl Reach {
    fn add(&mut self, b: &Birth, p: &Push) {
        self.sites += 1;
        if p.body == b.body {
            if p.depth > b.depth {
                self.repeated = true;
            } else if p.depth == b.depth && p.count.is_some() {
                self.single = self.single.saturating_add(p.count.unwrap_or(0));
            } else {
                self.open = true;
            }
        } else if p.depth > 0 {
            self.repeated = true;
        } else {
            self.open = true;
        }
    }

    /// The slots the birth reserves under the cap `c` (module docs).
    pub(crate) fn capacity(&self, c: u32) -> Option<u32> {
        if self.repeated {
            Some(c)
        } else if !self.open && self.single > EMPTY_SLOTS {
            Some(self.single.min(c))
        } else {
            None
        }
    }
}

/// Every list birth with what reaches it, in generation order.
pub(crate) fn reaches(g: &Gen<'_>) -> Vec<(Birth, Reach)> {
    let m = &g.m;
    // Template -> its births (a body walked twice births one template per
    // walk; each is its own entry).
    let mut by_tmpl: FxHashMap<TmplId, Vec<usize>> = FxHashMap::default();
    for (i, b) in g.list_births.iter().enumerate() {
        by_tmpl.entry(b.tmpl).or_default().push(i);
    }
    let mut out: Vec<(Birth, Reach)> = g
        .list_births
        .iter()
        .map(|b| (*b, Reach::default()))
        .collect();
    if by_tmpl.is_empty() || g.list_pushes.is_empty() {
        return out;
    }
    let copies = crate::rest_elems::node_copies(m);
    let mut seen: Vec<TmplId> = Vec::new();
    for p in &g.list_pushes {
        seen.clear();
        let nodes =
            std::iter::once(p.recv).chain(copies.get(&p.recv).into_iter().flatten().copied());
        for n in nodes {
            for t in m.nodes[n as usize].pts.to_sorted() {
                if t == crate::model::TOP {
                    continue;
                }
                if let Some(tm) = m.tmpl_of(t) {
                    if by_tmpl.contains_key(&tm) && !seen.contains(&tm) {
                        seen.push(tm);
                    }
                }
            }
        }
        for tm in &seen {
            for &i in &by_tmpl[tm] {
                let (b, r) = &mut out[i];
                r.add(b, p);
            }
        }
    }
    out
}

/// Fill `ModuleBindings::list_capacities` (module docs). `probe`: one
/// `# list_cap` line per list birth.
pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    mut probe: Option<&mut Vec<String>>,
) {
    let Some(c) = slots() else {
        return;
    };
    if g.m.aborted {
        return;
    }
    let m = &g.m;
    // (module, local) -> the most any birth of it reserves.
    let mut caps: std::collections::BTreeMap<(u32, u32), u32> = Default::default();
    for (b, r) in reaches(g) {
        let cap = r.capacity(c);
        if let Some(cap) = cap {
            let e = caps.entry((b.module, b.local)).or_insert(0);
            *e = (*e).max(cap);
        }
        if let Some(p) = probe.as_deref_mut() {
            // The length hull over the template's Arrays (`solve::arr_len`).
            let mut len = crate::model::Iv::EMPTY;
            for t in m.tmpl_toks(b.tmpl) {
                if let Some(&n) = m.arr_len.get(&t) {
                    len = len.join(m.nodes[n as usize].range);
                }
            }
            let len = if len.is_top() {
                "top".to_string()
            } else if len.is_empty() {
                "birth".to_string()
            } else {
                format!("[{},{}]", len.lo, len.hi)
            };
            p.push(format!(
                "# list_cap module={} local={} tmpl={} sites={} repeated={} open={} single={} len={len} cap={}",
                b.module,
                b.local,
                m.tmpls[b.tmpl as usize].display.replace(' ', "_"),
                r.sites,
                u8::from(r.repeated),
                u8::from(r.open),
                r.single,
                cap.map_or("-".to_string(), |c| c.to_string()),
            ));
        }
    }
    for ((mi, local), cap) in caps {
        if let Some(mb) = bindings.get_mut(mi as usize) {
            mb.list_capacities.push((local, cap));
        }
    }
}

impl Gen<'_> {
    /// Enter a body whose loop depth starts at 0 (a function, a closure,
    /// a class's field initializers, a module's top level): its serial
    /// becomes the current one. Returns the body to restore.
    pub(crate) fn list_enter_body(&mut self) -> u32 {
        self.list_bodies += 1;
        std::mem::replace(&mut self.list_body, self.list_bodies)
    }

    /// A `push` of `count` elements (`None`: a spread) on receiver `recv`.
    pub(crate) fn list_push(&mut self, recv: NodeId, count: Option<u32>) {
        self.list_pushes.push(Push {
            recv,
            body: self.list_body,
            depth: self.loop_depth,
            count,
        });
    }

    /// `let local = []` of `module` bore template `tmpl`.
    pub(crate) fn list_birth(&mut self, module: u32, local: u32, tmpl: TmplId) {
        self.list_births.push(Birth {
            module,
            local,
            tmpl,
            body: self.list_body,
            depth: self.loop_depth,
        });
    }
}
