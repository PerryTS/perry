//! Arithmetic classification on Linux; historical test and platform backend.
use super::*;

#[inline(always)]
pub(crate) fn classify_heap_generation(addr: usize) -> HeapGeneration {
    #[cfg(target_os = "linux")]
    {
        if let Some(generation) = crate::arena::region::reservation::owned_generation(addr) {
            return generation;
        }
        // Synthetic range fixtures use the historical test-only directory.
    }

    #[cfg(all(target_os = "linux", not(test)))]
    {
        HeapGeneration::Unknown
    }
    #[cfg(any(not(target_os = "linux"), test))]
    {
        legacy_classify_heap_generation(addr)
    }
}

#[inline(always)]
pub(crate) fn classify_heap_space_in_range(addr: usize) -> Option<(HeapSpace, usize, *mut u64)> {
    #[cfg(target_os = "linux")]
    {
        if let Some(space) = crate::arena::region::reservation::owned_space(addr) {
            return Some(space);
        }
        // Synthetic range fixtures use the historical test-only directory.
    }

    #[cfg(all(target_os = "linux", not(test)))]
    {
        None
    }
    #[cfg(any(not(target_os = "linux"), test))]
    {
        legacy_classify_heap_space_in_range(addr)
    }
}

/// The owner of an address's arena region, answered by region metadata only;
/// no header or payload is read. Linux consults the reservation descriptor;
/// other platforms (and synthetic test ranges) consult this thread's page map,
/// which can prove only `Current`.
#[inline(always)]
pub(crate) fn classify_region_ownership(addr: usize) -> RegionOwnership {
    #[cfg(target_os = "linux")]
    {
        if let Some(ownership) = crate::arena::region::reservation::region_ownership(addr) {
            return ownership;
        }
    }

    #[cfg(all(target_os = "linux", not(test)))]
    {
        RegionOwnership::Outside
    }
    #[cfg(any(not(target_os = "linux"), test))]
    {
        if legacy_classify_heap_generation(addr) == HeapGeneration::Unknown {
            RegionOwnership::Outside
        } else {
            RegionOwnership::Current
        }
    }
}

pub(crate) fn uniform_heap_generation(base: usize, end: usize) -> Option<HeapGeneration> {
    #[cfg(target_os = "linux")]
    {
        if let Some(generation) =
            crate::arena::region::reservation::owned_uniform_generation(base, end)
        {
            return Some(generation);
        }
    }

    #[cfg(all(target_os = "linux", not(test)))]
    {
        None
    }
    #[cfg(any(not(target_os = "linux"), test))]
    {
        legacy_uniform_heap_generation(base, end)
    }
}

/// Space and allocation class from one canonical publication; only collectors
/// need the class. No storage lookup is added to either barrier predicate.
#[inline(always)]
pub(crate) fn classify_heap_storage_in_range(addr: usize)
    -> Option<(HeapSpace, usize, *mut u64, bool)>
{
    #[cfg(target_os = "linux")]
    if let Some(found) = crate::arena::region::reservation::owned_storage(addr) {
        return Some(found);
    }
    #[cfg(all(target_os = "linux", not(test)))]
    { None }
    #[cfg(any(not(target_os = "linux"), test))]
    {
        let key = generation_class_key_for_addr(addr);
        let pages = hot_page_generations().borrow();
        let range = pages.get(&key)?.find(addr)?;
        Some((range.space, range.base, range.object_starts, range.large_storage))
    }
}
