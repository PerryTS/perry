//! Young-scoped GC maintenance for per-instance built-in closure metadata.

thread_local! {
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

pub(crate) fn set_bound_native_closure_name(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
) {
    define_bound_native_closure_metadata(closure, name, None);
}

/// A native function's name and spec length share one attributed birth.
pub(crate) fn set_bound_native_closure_metadata(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
    length: u32,
) {
    define_bound_native_closure_metadata(closure, name, Some(length));
}

fn define_bound_native_closure_metadata(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
    length: Option<u32>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure);
    let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_handle = scope.root_string_ptr(ptr);
    let name_value =
        f64::from_bits(crate::value::JSValue::string_ptr(name_handle.get_raw_mut_ptr()).bits());
    let closure = closure_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize;
    let attrs = crate::object::PropertyAttrs::new(false, false, true);
    let entries = [("name", name_value), ("length", length.unwrap_or(0) as f64)];
    let count = if length.is_some() { 2 } else { 1 };
    let entries_attrs = [crate::object::key_attrs::attr_bits_to_entry(attrs.bits); 2];
    if crate::closure::closure_define_first_props_with_attrs(
        closure,
        &entries[..count],
        &entries_attrs[..count],
    ) {
        return;
    }
    crate::closure::closure_define_data_with_attrs(closure, "name", name_value, attrs);
    if let Some(length) = length {
        set_builtin_closure_length(closure, length);
    }
}

/// The spec length is an own data property, including its attributes.
pub(crate) fn set_builtin_closure_length(closure: usize, length: u32) {
    let attrs = crate::object::PropertyAttrs::new(false, false, true);
    if crate::closure::closure_define_first_props_with_attrs(
        closure,
        &[("length", length as f64)],
        &[crate::object::key_attrs::attr_bits_to_entry(attrs.bits)],
    ) {
        return;
    }
    crate::closure::closure_define_data_with_attrs(closure, "length", length as f64, attrs);
}

/// A recorded length is read from the function bag; an unmaterialized bound
/// length remains in its capture, as before.
pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    if let Some(length) = crate::closure::closure_get_own_dynamic_prop(closure, "length") {
        if length.is_finite() && length >= 0.0 && length <= u32::MAX as f64 {
            return Some(length as u32);
        }
        return None;
    }
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
    let table_len = BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().len());
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
/// candidate set, and the full `retain` over the set -- ~275k instructions
/// per copying minor on dotenv, where the set holds every built-in closure
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
        set_builtin_closure_non_constructable(closure);
        TEST_SUPPRESS_BUILTIN_CLOSURE_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            BUILTIN_CLOSURE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(LOG_NAME, &relevant_owners())
            });
        });
        BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow_mut().remove(&closure));
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
        set_builtin_closure_non_constructable(young);
        set_builtin_closure_non_constructable(old);

        prune_dead_builtin_closure_metadata_owners_young(&|_| true);

        let (young_nc, old_nc) = (
            BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&young)),
            BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&old)),
        );
        clear_all();
        assert!(!young_nc, "dead young owner kept");
        assert!(old_nc, "old owner pruned by a minor");
    }

    /// A live young owner stays in the maps AND in the log, so the next minor
    /// still considers it.
    #[test]
    fn young_prune_keeps_live_young_owners_logged() {
        let _lock = crate::gc::global_side_table_test_lock();
        clear_all();
        let young = crate::closure::js_closure_alloc(std::ptr::null(), 0) as usize;
        set_builtin_closure_non_constructable(young);
        prune_dead_builtin_closure_metadata_owners_young(&|_| false);
        let still = BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&young));
        let logged = BUILTIN_CLOSURE_YOUNG.with(|log| log.borrow_mut().take_sorted());
        clear_all();
        assert!(still);
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
