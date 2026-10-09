//! The page index is sufficient to emit every intersecting object once.
use super::*;

#[test]
fn ordered_dirty_pages_emit_spanning_objects_once_even_across_clean_gaps() {
    super::tests::run_with_fresh_arenas(|| {
        let user = arena_alloc_gc_old(5 * 4096, 8, crate::gc::GC_TYPE_STRING);
        let header = unsafe { crate::gc::header_from_user_ptr(user) } as usize;
        let size = unsafe { (*(header as *const crate::gc::GcHeader)).size as usize };
        let overlaps = old_object_page_overlaps(header, size);
        assert!(overlaps.len() >= 5);
        for indices in [vec![0, 1, 2, 4], vec![1, 3, 4], vec![4]] {
            let pages = indices.iter().map(|&i| overlaps[i].0).collect();
            let mut actual = Vec::new();
            let count = old_arena_walk_objects_on_pages(&pages, |h| actual.push(h as usize));
            assert_eq!(count, 1);
            assert_eq!(actual, vec![header]);
        }
        // Negative control: visiting each page independently duplicates a
        // spanning object. The oracle must distinguish that traversal.
        let mut duplicated = Vec::new();
        for &(page, _) in &overlaps {
            let pages = [page].into_iter().collect();
            old_arena_walk_objects_on_pages(&pages, |h| duplicated.push(h as usize));
        }
        assert!(duplicated.len() > 1);
        assert!(duplicated.iter().all(|&h| h == header));
    });
}
