//! Young-entry logs for tagged-template and array named-property roots.

use super::*;

crate::perry_thread_local! {
    static TEMPLATE_CACHE_YOUNG: RefCell<crate::gc::young_log::YoungLog<u64>> =
        const { RefCell::new(crate::gc::young_log::YoungLog::new()) };
    static TEMPLATE_RAW_YOUNG: RefCell<crate::gc::young_log::YoungLog<usize>> =
        const { RefCell::new(crate::gc::young_log::YoungLog::new()) };
    static ARRAY_NAMED_YOUNG: RefCell<crate::gc::young_log::YoungLog<usize>> =
        const { RefCell::new(crate::gc::young_log::YoungLog::new()) };
    #[cfg(test)]
    static TEST_SUPPRESS_ARRAY_NAMED_YOUNG_NOTE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    #[cfg(test)]
    static TEST_SUPPRESS_TEMPLATE_RAW_YOUNG_NOTE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

const CACHE_LOG: &str = "array.template_object_cache";
const RAW_LOG: &str = "array.template_raw_map";
const NAMED_LOG: &str = "array.named_properties";

#[inline]
fn ptr_relevant(ptr: *mut ArrayHeader) -> bool {
    crate::gc::young_log::addr_is_minor_relevant(ptr as usize)
}

pub(super) fn note_template_cache(site: u64, cooked: *mut ArrayHeader, raw: *mut ArrayHeader) {
    if ptr_relevant(cooked) || ptr_relevant(raw) {
        TEMPLATE_CACHE_YOUNG.with(|log| log.borrow_mut().note(site));
    }
}

pub(super) fn note_template_raw(cooked: usize, raw: *mut ArrayHeader) {
    if crate::gc::young_log::addr_is_minor_relevant(cooked) || ptr_relevant(raw) {
        #[cfg(test)]
        if TEST_SUPPRESS_TEMPLATE_RAW_YOUNG_NOTE.with(std::cell::Cell::get) {
            return;
        }
        TEMPLATE_RAW_YOUNG.with(|log| log.borrow_mut().note(cooked));
    }
}

pub(super) fn note_array_named(owner: usize, value_bits: u64) {
    if !crate::gc::young_log::addr_is_minor_collectible(owner)
        && !crate::gc::young_log::bits_are_minor_relevant(value_bits)
    {
        return;
    }
    #[cfg(test)]
    if TEST_SUPPRESS_ARRAY_NAMED_YOUNG_NOTE.with(std::cell::Cell::get) {
        return;
    }
    ARRAY_NAMED_YOUNG.with(|log| log.borrow_mut().note(owner));
}

fn visit_cache_site(visitor: &mut crate::gc::RuntimeRootVisitor<'_>, site: u64) -> bool {
    TEMPLATE_OBJECT_CACHE.with(|m| {
        let mut map = m.borrow_mut();
        let Some((cooked, raw)) = map.get_mut(&site) else {
            return false;
        };
        visitor.visit_raw_mut_ptr_slot(cooked);
        visitor.visit_raw_mut_ptr_slot(raw);
        ptr_relevant(*cooked) || ptr_relevant(*raw)
    })
}

fn visit_raw_owner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>, owner: usize) -> Option<usize> {
    TEMPLATE_RAW_MAP.with(|m| {
        let mut map = m.borrow_mut();
        let mut raw = map.remove(&owner)?;
        let mut new_owner = owner;
        visitor.visit_usize_slot(&mut new_owner);
        visitor.visit_raw_mut_ptr_slot(&mut raw);
        map.insert(new_owner, raw);
        (crate::gc::young_log::addr_is_minor_relevant(new_owner) || ptr_relevant(raw))
            .then_some(new_owner)
    })
}

fn visit_named_owner(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) -> Option<usize> {
    ARRAY_NAMED_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let mut props = map.remove(&owner)?;
        let mut new_owner = owner;
        visitor.visit_metadata_usize_slot(&mut new_owner);
        let mut relevant = crate::gc::young_log::addr_is_minor_collectible(new_owner);
        for prop in &mut props {
            visitor.visit_nanbox_f64_slot(&mut prop.value);
            relevant |= crate::gc::young_log::bits_are_minor_relevant(prop.value.to_bits());
        }
        merge_array_named_props(&mut map, new_owner, props);
        relevant.then_some(new_owner)
    })
}

#[cfg(any(debug_assertions, test))]
fn relevant_cache_sites() -> Vec<u64> {
    TEMPLATE_OBJECT_CACHE.with(|m| {
        m.borrow()
            .iter()
            .filter_map(|(&site, &(cooked, raw))| {
                (ptr_relevant(cooked) || ptr_relevant(raw)).then_some(site)
            })
            .collect()
    })
}

#[cfg(any(debug_assertions, test))]
fn relevant_raw_owners() -> Vec<usize> {
    TEMPLATE_RAW_MAP.with(|m| {
        m.borrow()
            .iter()
            .filter_map(|(&owner, &raw)| {
                (crate::gc::young_log::addr_is_minor_relevant(owner) || ptr_relevant(raw))
                    .then_some(owner)
            })
            .collect()
    })
}

#[cfg(any(debug_assertions, test))]
fn relevant_named_owners() -> Vec<usize> {
    ARRAY_NAMED_PROPS.with(|m| {
        m.borrow()
            .iter()
            .filter_map(|(&owner, props)| {
                (crate::gc::young_log::addr_is_minor_collectible(owner)
                    || props.iter().any(|prop| {
                        crate::gc::young_log::bits_are_minor_relevant(prop.value.to_bits())
                    }))
                .then_some(owner)
            })
            .collect()
    })
}

fn drain_log<K: Copy + Ord>(
    log: &'static crate::tls_hot::HotKey<RefCell<crate::gc::young_log::YoungLog<K>>>,
    mut visit: impl FnMut(K) -> Option<K>,
) -> (u64, u64, u64) {
    let mut logged = 0;
    let mut visited = 0;
    let mut kept = log.with(|log| log.borrow_mut().take_spare());
    loop {
        let batch = log.with(|log| log.borrow_mut().take_sorted());
        if batch.is_empty() {
            break;
        }
        logged += batch.len() as u64;
        for key in batch {
            visited += 1;
            if let Some(key) = visit(key) {
                kept.push(key);
            }
        }
    }
    let kept_len = kept.len() as u64;
    log.with(|log| log.borrow_mut().extend(kept));
    (logged, visited, kept_len)
}

fn report(name: &'static str, partial: bool, row: (u64, u64, u64), table_len: usize) {
    crate::gc::young_log::note_walk(
        name,
        crate::gc::young_log::YoungLogWalk {
            partial,
            logged: row.0,
            visited: row.1,
            kept: row.2,
            table_len: table_len as u64,
        },
    );
}

pub fn scan_template_raw_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let cache_len = TEMPLATE_OBJECT_CACHE.with(|m| m.borrow().len());
    let raw_len = TEMPLATE_RAW_MAP.with(|m| m.borrow().len());
    let named_len = ARRAY_NAMED_PROPS.with(|m| m.borrow().len());
    if visitor.young_scope() {
        #[cfg(any(debug_assertions, test))]
        {
            TEMPLATE_CACHE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(CACHE_LOG, &relevant_cache_sites())
            });
            TEMPLATE_RAW_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(RAW_LOG, &relevant_raw_owners())
            });
            ARRAY_NAMED_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(NAMED_LOG, &relevant_named_owners())
            });
        }
        let cache = drain_log(&TEMPLATE_CACHE_YOUNG, |site| {
            visit_cache_site(visitor, site).then_some(site)
        });
        let raw = drain_log(&TEMPLATE_RAW_YOUNG, |owner| visit_raw_owner(visitor, owner));
        let named = drain_log(&ARRAY_NAMED_YOUNG, |owner| {
            visit_named_owner(visitor, owner)
        });
        report(CACHE_LOG, true, cache, cache_len);
        report(RAW_LOG, true, raw, raw_len);
        report(NAMED_LOG, true, named, named_len);
        return;
    }

    let cache_sites: Vec<u64> =
        TEMPLATE_OBJECT_CACHE.with(|m| m.borrow().keys().copied().collect());
    let raw_owners: Vec<usize> = TEMPLATE_RAW_MAP.with(|m| m.borrow().keys().copied().collect());
    let named_owners: Vec<usize> = ARRAY_NAMED_PROPS.with(|m| m.borrow().keys().copied().collect());
    let _ = TEMPLATE_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let _ = TEMPLATE_RAW_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let _ = ARRAY_NAMED_YOUNG.with(|log| log.borrow_mut().take_sorted());
    // `drain_log` consumes the log, so seed it with the authoritative keys.
    TEMPLATE_CACHE_YOUNG.with(|log| log.borrow_mut().extend(cache_sites));
    let cache = drain_log(&TEMPLATE_CACHE_YOUNG, |site| {
        visit_cache_site(visitor, site).then_some(site)
    });
    TEMPLATE_RAW_YOUNG.with(|log| log.borrow_mut().extend(raw_owners));
    let raw = drain_log(&TEMPLATE_RAW_YOUNG, |owner| visit_raw_owner(visitor, owner));
    ARRAY_NAMED_YOUNG.with(|log| log.borrow_mut().extend(named_owners));
    let named = drain_log(&ARRAY_NAMED_YOUNG, |owner| {
        visit_named_owner(visitor, owner)
    });
    report(CACHE_LOG, false, cache, cache_len);
    report(RAW_LOG, false, raw, raw_len);
    report(NAMED_LOG, false, named, named_len);
}

#[cfg(test)]
pub(super) fn clear_named_log() {
    ARRAY_NAMED_YOUNG.with(|log| log.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alloc_empty_array() -> *mut ArrayHeader {
        let arr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<ArrayHeader>(),
            std::mem::align_of::<ArrayHeader>(),
            crate::gc::GC_TYPE_ARRAY,
        ) as *mut ArrayHeader;
        unsafe {
            (*arr).length = 0;
            (*arr).capacity = 0;
        }
        arr
    }

    #[test]
    fn template_raw_log_rederivation_rejects_a_suppressed_writer() {
        let _lock = crate::gc::global_side_table_test_lock();
        let cooked = alloc_empty_array();
        let raw = alloc_empty_array();
        TEST_SUPPRESS_TEMPLATE_RAW_YOUNG_NOTE.with(|flag| flag.set(true));
        test_seed_template_raw_roots(cooked, raw);
        TEST_SUPPRESS_TEMPLATE_RAW_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            TEMPLATE_RAW_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(RAW_LOG, &relevant_raw_owners())
            });
        });
        TEMPLATE_RAW_MAP.with(|m| m.borrow_mut().clear());
        TEMPLATE_RAW_YOUNG.with(|log| log.borrow_mut().clear());
        assert!(
            missed.is_err(),
            "sabotage: suppressing the template-raw writer's note must trip completeness"
        );
    }

    #[test]
    fn array_named_log_rederivation_rejects_a_suppressed_setter() {
        let _lock = crate::gc::global_side_table_test_lock();
        let arr = alloc_empty_array();
        let key = crate::string::js_string_from_bytes(b"sabotage".as_ptr(), 8);
        TEST_SUPPRESS_ARRAY_NAMED_YOUNG_NOTE.with(|flag| flag.set(true));
        unsafe { array_named_property_set(arr, key, 7.0) };
        TEST_SUPPRESS_ARRAY_NAMED_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            ARRAY_NAMED_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(NAMED_LOG, &relevant_named_owners())
            });
        });
        ARRAY_NAMED_PROPS.with(|m| m.borrow_mut().remove(&(arr as usize)));
        clear_named_log();
        assert!(
            missed.is_err(),
            "sabotage: suppressing array_named_property_set's note must trip completeness"
        );
    }
}
