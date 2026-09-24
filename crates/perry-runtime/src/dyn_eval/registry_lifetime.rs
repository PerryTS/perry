//! Parsed ASTs follow closure liveness. Owner addresses are weak: never mark
//! the closures from here.
//!
//! An `FN_REGISTRY` id stays registered while ANY of these holds it:
//!
//! * a live closure (`owner_counts`, maintained per owner insert/death, so a
//!   collection never rebuilds the live set);
//! * the bounded source cache (`permanent`: `SOURCE_FN_CACHE` never evicts);
//! * an active pin (an `Rc` clone on `ACTIVE_FUNCTIONS`: a running call, a
//!   construction in progress, or a running script);
//! * a node-cache entry whose PARENT AST is still registered (`node_parent`).
//!   A nested function's node key is an address inside its parent's AST, so
//!   the entry is exactly as long-lived as that AST; while the parent lives
//!   the child is reused (one registration per syntactic function, as before
//!   the registry was pruned), and when the parent is reclaimed its node
//!   entries go with it — before anything can reuse the address.
//!
//! Reclamation only examines `candidates`: ids that were just registered, or
//! whose last owner or parent just went away. A collection therefore costs
//! O(dead owners + candidates), and a MINOR visits only the young-owner log.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::gc::young_log::{self, YoungLog, YoungLogWalk};

crate::perry_thread_local! {
    // Pins live on the interpreter root stack so throws release them even
    // though Perry's exception transport skips Rust destructors.
    static ACTIVE_FUNCTIONS: RefCell<Vec<(usize, Rc<super::InterpFn>)>> = RefCell::new(Vec::new());
    /// One past the root index of the newest pin, 0 when there is none. Pins
    /// are pushed at the root-stack top, so every pin sits below this; it lets
    /// `roots_truncate` (run on nearly every interpreter step) skip the
    /// `ACTIVE_FUNCTIONS` borrow when nothing it truncates is pinned.
    static PIN_TOP: Cell<usize> = const { Cell::new(0) };
    /// `Lifetime::owners.len()`, readable without a borrow: the GC's closure
    /// move hook runs for EVERY relocated closure, compiled ones included.
    static OWNER_COUNT: Cell<usize> = const { Cell::new(0) };
    static LIFETIME: RefCell<Lifetime> = RefCell::new(Lifetime::default());
    /// Young-log for `Lifetime::owners` (see `gc/young_log.rs`): the owners a
    /// minor can find dead. Old owners are never noted — a minor cannot prove
    /// them dead, so the young prune has nothing to do for them.
    static YOUNG_OWNERS: RefCell<YoungLog<usize>> = const { RefCell::new(YoungLog::new()) };
}

const YOUNG_LOG_NAME: &str = "dyn_eval.closure_fn_ids";

#[derive(Default)]
struct Lifetime {
    /// Weak closure address → the `InterpFn` id it runs (`CLOSURE_FN_IDS`).
    owners: HashMap<usize, u32>,
    /// id → number of `owners` entries naming it. Zero entries are removed.
    owner_counts: HashMap<u32, u32>,
    /// ids held by `SOURCE_FN_CACHE` (which never evicts).
    permanent: HashSet<u32>,
    /// ids that may have become reclaimable. Duplicates are harmless.
    candidates: Vec<u32>,
    /// (parent id, AST-node address in the parent) → child id.
    node_ids: HashMap<(u32, usize), u32>,
    /// child id → its `node_ids` key.
    node_parent: HashMap<u32, (u32, usize)>,
    /// parent id → children registered from its AST.
    node_children: HashMap<u32, Vec<u32>>,
}

/// A borrowed AST backed by an explicit root-stack pin. Ordinary returns
/// truncate through Drop; exception restore truncates the same stack directly.
pub(super) struct FunctionPin {
    function: *const super::InterpFn,
    root: usize,
}

impl std::ops::Deref for FunctionPin {
    type Target = super::InterpFn;

    fn deref(&self) -> &Self::Target {
        // SAFETY: ACTIVE_FUNCTIONS owns an Rc until this pin's root is
        // truncated, and FN_REGISTRY keeps its own Rc until a later prune
        // finds no pin. Callers finish all AST reads before truncating that
        // root; a throw that truncates it never returns to the abandoned
        // caller.
        unsafe { &*self.function }
    }
}

impl Drop for FunctionPin {
    fn drop(&mut self) {
        super::roots_truncate(self.root);
    }
}

pub(super) fn pin_function(id: u32) -> Option<FunctionPin> {
    let function = super::lookup_fn(id)?;
    let ptr = Rc::as_ptr(&function);
    let root = super::root_push(super::bridge::undefined());
    ACTIVE_FUNCTIONS.with(|pins| pins.borrow_mut().push((root, function)));
    PIN_TOP.with(|top| top.set(root + 1));
    Some(FunctionPin {
        function: ptr,
        root,
    })
}

#[inline]
pub(super) fn release_function_pins(root_len: usize) {
    if PIN_TOP.with(Cell::get) <= root_len {
        return;
    }
    ACTIVE_FUNCTIONS.with(|pins| {
        let mut pins = pins.borrow_mut();
        while pins.last().is_some_and(|(root, _)| *root >= root_len) {
            pins.pop();
        }
        PIN_TOP.with(|top| top.set(pins.last().map_or(0, |(root, _)| root + 1)));
    });
}

/// A freshly registered id has no owner yet; the next prune decides it.
pub(super) fn note_registered(id: u32) {
    LIFETIME.with(|lt| lt.borrow_mut().candidates.push(id));
}

/// `SOURCE_FN_CACHE` now holds `id` for the rest of the thread's life.
pub(super) fn note_permanent(id: u32) {
    LIFETIME.with(|lt| lt.borrow_mut().permanent.insert(id));
}

/// The child id registered for the AST node at `addr` inside `parent`'s AST,
/// building and registering it on first use.
pub(super) fn node_fn_id(parent: u32, addr: usize, build: impl FnOnce() -> super::InterpFn) -> u32 {
    let key = (parent, addr);
    if let Some(id) = LIFETIME.with(|lt| lt.borrow().node_ids.get(&key).copied()) {
        return id;
    }
    // Built and registered outside the borrow: `register_fn` notes the id.
    let id = super::register_fn(build());
    LIFETIME.with(|lt| {
        let mut lt = lt.borrow_mut();
        lt.node_ids.insert(key, id);
        lt.node_parent.insert(id, key);
        lt.node_children.entry(parent).or_default().push(id);
    });
    id
}

pub(crate) fn register_closure(owner: usize, id: u32) {
    // Rule 1 of gc/young_log.rs: note before the entry becomes findable.
    if young_log::addr_is_minor_collectible(owner) {
        YOUNG_OWNERS.with(|log| log.borrow_mut().note(owner));
    }
    LIFETIME.with(|lt| {
        let mut lt = lt.borrow_mut();
        let lt = &mut *lt;
        if let Some(previous) = lt.owners.insert(owner, id) {
            release_owner(&mut lt.owner_counts, &mut lt.candidates, previous);
        }
        *lt.owner_counts.entry(id).or_insert(0) += 1;
        OWNER_COUNT.with(|n| n.set(lt.owners.len()));
    });
}

pub(crate) fn function_owner_moved(old: usize, new: usize) {
    if old == new || OWNER_COUNT.with(Cell::get) == 0 {
        return;
    }
    LIFETIME.with(|lt| {
        let mut lt = lt.borrow_mut();
        if let Some(id) = lt.owners.remove(&old) {
            // A re-keyed owner is noted unconditionally; the next young walk
            // drops it if it was promoted.
            YOUNG_OWNERS.with(|log| log.borrow_mut().note(new));
            lt.owners.insert(new, id);
        }
    });
}

fn release_owner(counts: &mut HashMap<u32, u32>, candidates: &mut Vec<u32>, id: u32) {
    if let Some(count) = counts.get_mut(&id) {
        *count -= 1;
        if *count == 0 {
            counts.remove(&id);
            candidates.push(id);
        }
    }
}

/// Full-cycle prune, called by the GC's dead-owner pass before dead storage
/// is recycled. Walks every owner and rebuilds the young log.
pub(crate) fn prune_dead_function_owners(is_dead: &dyn Fn(usize) -> bool) {
    LIFETIME.with(|lt| {
        let mut lt = lt.borrow_mut();
        let lt = &mut *lt;
        if lt.owners.is_empty() && lt.candidates.is_empty() {
            return;
        }
        let _ = YOUNG_OWNERS.with(|log| log.borrow_mut().take_sorted());
        let mut kept = YOUNG_OWNERS.with(|log| log.borrow_mut().take_spare());
        let table_len = lt.owners.len() as u64;
        let Lifetime {
            owners,
            owner_counts,
            candidates,
            ..
        } = &mut *lt;
        owners.retain(|&owner, &mut id| {
            if is_dead(owner) {
                release_owner(owner_counts, candidates, id);
                false
            } else {
                if young_log::addr_is_minor_collectible(owner) {
                    kept.push(owner);
                }
                true
            }
        });
        OWNER_COUNT.with(|n| n.set(lt.owners.len()));
        let walk = YoungLogWalk {
            partial: false,
            logged: table_len,
            visited: table_len,
            kept: kept.len() as u64,
            table_len,
        };
        YOUNG_OWNERS.with(|log| log.borrow_mut().extend(kept));
        young_log::note_walk(YOUNG_LOG_NAME, walk);
        reclaim_candidates(lt);
    });
}

/// Minor-cycle prune: only young owners can die in a minor, and every young
/// owner is in `YOUNG_OWNERS`, so this never walks the whole table.
pub(crate) fn prune_dead_function_owners_young(is_dead: &dyn Fn(usize) -> bool) {
    LIFETIME.with(|lt| {
        let mut lt = lt.borrow_mut();
        let lt = &mut *lt;
        if lt.owners.is_empty() && lt.candidates.is_empty() {
            return;
        }
        #[cfg(debug_assertions)]
        {
            // Rule 2: every minor-collectible owner must be logged.
            let relevant = lt
                .owners
                .keys()
                .copied()
                .filter(|&owner| young_log::addr_is_minor_collectible(owner))
                .collect::<Vec<_>>();
            YOUNG_OWNERS.with(|log| log.borrow().debug_assert_logged(YOUNG_LOG_NAME, &relevant));
        }
        let logged = YOUNG_OWNERS.with(|log| log.borrow_mut().take_sorted());
        let mut kept = YOUNG_OWNERS.with(|log| log.borrow_mut().take_spare());
        let mut visited = 0u64;
        for &owner in &logged {
            let Some(&id) = lt.owners.get(&owner) else {
                continue;
            };
            visited += 1;
            if is_dead(owner) {
                lt.owners.remove(&owner);
                release_owner(&mut lt.owner_counts, &mut lt.candidates, id);
            } else if young_log::addr_is_minor_collectible(owner) {
                kept.push(owner);
            }
        }
        OWNER_COUNT.with(|n| n.set(lt.owners.len()));
        let walk = YoungLogWalk {
            partial: true,
            logged: logged.len() as u64,
            visited,
            kept: kept.len() as u64,
            table_len: lt.owners.len() as u64,
        };
        YOUNG_OWNERS.with(|log| log.borrow_mut().extend(kept));
        young_log::note_walk(YOUNG_LOG_NAME, walk);
        reclaim_candidates(lt);
    });
}

/// Drop every candidate nothing holds. A reclaimed parent evicts its node
/// entries in the same step, so no address inside its AST can be looked up
/// after the AST is freed; the orphaned children become candidates in turn.
fn reclaim_candidates(lt: &mut Lifetime) {
    if lt.candidates.is_empty() {
        return;
    }
    let mut work = std::mem::take(&mut lt.candidates);
    let mut pinned = Vec::new();
    super::FN_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        while let Some(id) = work.pop() {
            let Some(function) = registry.get(&id) else {
                continue;
            };
            if lt.owner_counts.contains_key(&id)
                || lt.permanent.contains(&id)
                || lt.node_parent.contains_key(&id)
            {
                // Held; whatever releases the hold re-queues the id.
                continue;
            }
            if Rc::strong_count(function) > 1 {
                // Running or mid-construction: retry at the next prune.
                pinned.push(id);
                continue;
            }
            registry.remove(&id);
            for child in lt.node_children.remove(&id).unwrap_or_default() {
                if let Some(key) = lt.node_parent.remove(&child) {
                    lt.node_ids.remove(&key);
                }
                work.push(child);
            }
        }
    });
    pinned.sort_unstable();
    pinned.dedup();
    lt.candidates = pinned;
}

#[cfg(test)]
pub(super) fn node_cache_len() -> usize {
    LIFETIME.with(|lt| lt.borrow().node_ids.len())
}

#[cfg(test)]
#[path = "registry_lifetime_tests.rs"]
mod tests;
