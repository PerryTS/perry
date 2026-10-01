//! Young-scoped GC maintenance for per-instance built-in closure metadata.

thread_local! {
    static BUILTIN_CLOSURE_LENGTH: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static BUILTIN_CLOSURE_NON_CONSTRUCTABLE: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

crate::perry_thread_local! {
    static BUILTIN_CLOSURE_YOUNG: std::cell::RefCell<crate::gc::young_log::YoungLog<usize>> =
        const { std::cell::RefCell::new(crate::gc::young_log::YoungLog::new()) };
}

#[cfg(test)]
thread_local! {
    static TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

const LOG_NAME: &str = "object.builtin_closure_metadata";

#[inline]
fn note(closure: usize) {
    if !crate::gc::young_log::addr_is_minor_collectible(closure) {
        return;
    }
    #[cfg(test)]
    if TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(std::cell::Cell::get) {
        return;
    }
    BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().note(closure));
}

pub(crate) fn set_builtin_closure_length(closure: usize, length: u32) {
    note(closure);
    BUILTIN_CLOSURE_LENGTH.with(|m| {
        m.borrow_mut().insert(closure, length);
    });
}

/// The recorded spec `.length` of the closure at `closure` — an address the
/// caller has already proven is a closure (every reader asks from inside its
/// closure arm; the bind-capture fallback re-checks only the header byte).
pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    if let Some(len) = BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&closure).copied()) {
        return Some(len);
    }
    // A bind result carries its length in a capture, not in this table.
    unsafe { crate::closure::bound_function_length(closure) }
}

pub(crate) fn set_builtin_closure_non_constructable(closure: usize) {
    note(closure);
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| {
        m.borrow_mut().insert(closure);
    });
}

/// Per-instance entries first, then the function-KIND bit (#10521): a closure
/// whose body is a registered built-in non-constructor answers without an
/// entry of its own.
pub(crate) fn builtin_closure_is_non_constructable(closure: usize) -> bool {
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&closure))
        || crate::closure::closure_body_is_non_constructor(
            closure as *const crate::closure::ClosureHeader,
        )
}

#[cfg(any(debug_assertions, test))]
fn relevant_owners() -> Vec<usize> {
    let mut owners = Vec::new();
    BUILTIN_CLOSURE_LENGTH.with(|m| {
        owners.extend(
            m.borrow()
                .keys()
                .copied()
                .filter(|owner| crate::gc::young_log::addr_is_minor_collectible(*owner)),
        );
    });
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| {
        owners.extend(
            m.borrow()
                .iter()
                .copied()
                .filter(|owner| crate::gc::young_log::addr_is_minor_collectible(*owner)),
        );
    });
    owners.sort_unstable();
    owners.dedup();
    owners
}

fn visit_owner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>, owner: usize) -> Option<usize> {
    let mut new_owner = owner;
    visitor.visit_metadata_usize_slot(&mut new_owner);
    BUILTIN_CLOSURE_LENGTH.with(|lengths| {
        let mut lengths = lengths.borrow_mut();
        if new_owner != owner {
            if let Some(length) = lengths.remove(&owner) {
                lengths.insert(new_owner, length);
            }
        }
    });
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|set| {
        let mut set = set.borrow_mut();
        if new_owner != owner && set.remove(&owner) {
            set.insert(new_owner);
        }
    });
    crate::gc::young_log::addr_is_minor_collectible(new_owner).then_some(new_owner)
}

pub(crate) fn scan_builtin_closure_metadata_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let table_len = BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().len())
        + BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().len());
    if visitor.young_scope() {
        #[cfg(any(debug_assertions, test))]
        BUILTIN_CLOSURE_YOUNG.with(|log| {
            log.borrow()
                .debug_assert_logged(LOG_NAME, &relevant_owners())
        });
        let mut logged = 0u64;
        let mut visited = 0u64;
        let mut kept = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_spare());
        loop {
            let batch = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_sorted());
            if batch.is_empty() {
                break;
            }
            logged += batch.len() as u64;
            for owner in batch {
                visited += 1;
                if let Some(owner) = visit_owner(visitor, owner) {
                    kept.push(owner);
                }
            }
        }
        let kept_len = kept.len() as u64;
        BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().extend(kept));
        crate::gc::young_log::note_walk(
            LOG_NAME,
            crate::gc::young_log::YoungLogWalk {
                partial: true,
                logged,
                visited,
                kept: kept_len,
                table_len: table_len as u64,
            },
        );
        return;
    }

    let mut owners = Vec::new();
    BUILTIN_CLOSURE_LENGTH.with(|m| owners.extend(m.borrow().keys().copied()));
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| owners.extend(m.borrow().iter().copied()));
    owners.sort_unstable();
    owners.dedup();
    let visited = owners.len() as u64;
    let _ = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_spare());
    for owner in owners {
        if let Some(owner) = visit_owner(visitor, owner) {
            kept.push(owner);
        }
    }
    let kept_len = kept.len() as u64;
    BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().extend(kept));
    crate::gc::young_log::note_walk(
        LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: visited,
            visited,
            kept: kept_len,
            table_len: table_len as u64,
        },
    );
}

pub(crate) fn prune_dead_builtin_closure_metadata_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    BUILTIN_CLOSURE_LENGTH.with(|lengths| {
        lengths
            .borrow_mut()
            .retain(|owner, _| !is_dead_owner(*owner));
    });
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|non_constructable| {
        non_constructable
            .borrow_mut()
            .retain(|owner| !is_dead_owner(*owner));
    });
}

/// [`prune_dead_builtin_closure_metadata_owners`] for a MINOR. A minor can
/// find only a minor-collectible owner dead, and every such owner is in
/// `BUILTIN_CLOSURE_YOUNG`: its writers note it (rule 1) and the minor's
/// young-scoped scan re-logs every owner that is still collectible, which a
/// dead owner (never moved, still in from-space) is. So the log is the
/// candidate set, and the full `retain` over both maps -- ~275k instructions
/// per copying minor on dotenv, where the maps hold every built-in closure
/// the program ever made -- is only needed on a full collection.
pub(crate) fn prune_dead_builtin_closure_metadata_owners_young(
    is_dead_owner: &dyn Fn(usize) -> bool,
) {
    #[cfg(any(debug_assertions, test))]
    BUILTIN_CLOSURE_YOUNG.with(|log| {
        log.borrow()
            .debug_assert_logged(LOG_NAME, &relevant_owners())
    });
    let candidates = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = Vec::with_capacity(candidates.len());
    for owner in candidates {
        if is_dead_owner(owner) {
            BUILTIN_CLOSURE_LENGTH.with(|m| {
                m.borrow_mut().remove(&owner);
            });
            BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| {
                m.borrow_mut().remove(&owner);
            });
        } else if crate::gc::young_log::addr_is_minor_collectible(owner) {
            kept.push(owner);
        }
    }
    BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().extend(kept));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_closure_log_rederivation_rejects_a_suppressed_writer() {
        let _lock = crate::gc::global_side_table_test_lock();
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 0) as usize;
        TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(|flag| flag.set(true));
        set_builtin_closure_length(closure, 3);
        TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            BUILTIN_CLOSURE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(LOG_NAME, &relevant_owners())
            });
        });
        BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow_mut().remove(&closure));
        BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().clear());
        assert!(
            missed.is_err(),
            "sabotage: suppressing the setter's note must trip completeness"
        );
    }

    fn old_closure() -> usize {
        crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            std::mem::align_of::<crate::closure::ClosureHeader>(),
            crate::gc::GC_TYPE_CLOSURE,
        ) as usize
    }

    fn clear_all() {
        BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow_mut().clear());
        BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow_mut().clear());
        BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().clear());
    }

    /// The young prune's candidates are the log: a dead YOUNG owner's entries
    /// go, and an OLD owner is never even asked about — even under a predicate
    /// that calls everything dead, which only a full collection may apply.
    #[test]
    fn young_prune_drops_dead_young_owners_and_never_touches_old_ones() {
        let _lock = crate::gc::global_side_table_test_lock();
        clear_all();
        let young = crate::closure::js_closure_alloc(std::ptr::null(), 0) as usize;
        let old = old_closure();
        assert!(crate::gc::young_log::addr_is_minor_collectible(young));
        assert!(!crate::gc::young_log::addr_is_minor_collectible(old));
        set_builtin_closure_length(young, 2);
        set_builtin_closure_non_constructable(young);
        set_builtin_closure_length(old, 5);
        set_builtin_closure_non_constructable(old);

        prune_dead_builtin_closure_metadata_owners_young(&|_| true);

        let (young_len, young_nc, old_len, old_nc) = (
            BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&young).copied()),
            BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&young)),
            BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&old).copied()),
            BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&old)),
        );
        clear_all();
        assert_eq!(
            (young_len, young_nc),
            (None, false),
            "dead young owner kept"
        );
        assert_eq!(
            (old_len, old_nc),
            (Some(5), true),
            "old owner pruned by a minor"
        );
    }

    /// A live young owner stays in the maps AND in the log, so the next minor
    /// still considers it.
    #[test]
    fn young_prune_keeps_live_young_owners_logged() {
        let _lock = crate::gc::global_side_table_test_lock();
        clear_all();
        let young = crate::closure::js_closure_alloc(std::ptr::null(), 0) as usize;
        set_builtin_closure_length(young, 1);
        prune_dead_builtin_closure_metadata_owners_young(&|_| false);
        let still = BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&young).copied());
        let logged = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_sorted());
        clear_all();
        assert_eq!(still, Some(1));
        assert_eq!(logged, vec![young]);
    }

    /// SABOTAGE: a writer that publishes a young owner without noting it is
    /// caught by the young prune's completeness check, before the prune
    /// could silently keep that owner's entry after it dies.
    #[test]
    fn young_prune_rejects_a_suppressed_writer() {
        let _lock = crate::gc::global_side_table_test_lock();
        clear_all();
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 0) as usize;
        TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(|flag| flag.set(true));
        set_builtin_closure_non_constructable(closure);
        TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            prune_dead_builtin_closure_metadata_owners_young(&|_| false);
        });
        clear_all();
        assert!(
            missed.is_err(),
            "the young prune must refuse an incomplete log"
        );
    }
}
