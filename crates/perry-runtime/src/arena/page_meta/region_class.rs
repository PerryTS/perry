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
