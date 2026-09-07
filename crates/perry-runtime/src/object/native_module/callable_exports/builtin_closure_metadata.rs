//! Young-scoped GC maintenance for per-instance built-in closure metadata.

thread_local! {
    static BUILTIN_CLOSURE_LENGTH: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static BUILTIN_CLOSURE_NON_CONSTRUCTABLE: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());
    static BUILTIN_CLOSURE_YOUNG: std::cell::RefCell<crate::gc::young_log::YoungLog<usize>> =
        const { std::cell::RefCell::new(crate::gc::young_log::YoungLog::new()) };
    #[cfg(test)]
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

pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&closure).copied())
}

pub(crate) fn set_builtin_closure_non_constructable(closure: usize) {
    note(closure);
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| {
        m.borrow_mut().insert(closure);
    });
}

pub(crate) fn builtin_closure_is_non_constructable(closure: usize) -> bool {
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&closure))
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
}
