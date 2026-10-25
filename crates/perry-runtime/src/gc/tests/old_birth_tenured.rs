//! Old => TENURED holds for every old-generation birth. The barrier's parent
//! gate (`parent_flags_may_need_remembering`, mirrored by codegen) reads an
//! untenured arena header as young and remembers nothing, so an old object
//! born without the bit loses the young children stored into it at the next
//! minor whenever its page is not already dirty for another reason.

use super::super::*;
use super::support::*;

unsafe fn assert_born_tenured(user: usize, path: &str) {
    assert!(crate::arena::pointer_in_old_gen(user), "{path}: premise, born old");
    let flags = (*header_from_user_ptr(user as *const u8)).gc_flags;
    assert_ne!(flags & GC_FLAG_ARENA, 0, "{path}: arena header");
    assert_ne!(flags & GC_FLAG_TENURED, 0, "{path}: born old but not TENURED");
    assert!(
        super::super::barrier_store::parent_flags_may_need_remembering(flags),
        "{path}: the barrier's parent gate would skip this old object"
    );
}

#[test]
fn every_old_generation_birth_is_tenured() {
    let _isolation = copying_nursery_isolation_lock();
    super::super::old_free::old_free_reset_for_test();
    unsafe {
        let bumped = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
        assert_born_tenured(bumped, "arena_alloc_gc_old (bump)");

        let promoted = crate::arena::arena_alloc_gc_promoted(64, 8, GC_TYPE_OBJECT, false) as usize;
        assert_born_tenured(promoted, "arena_alloc_gc_promoted");

        let no_pages = crate::fast_hash::new_ptr_hash_set();
        let relocated =
            crate::arena::arena_alloc_gc_old_excluding_pages(64, 8, GC_TYPE_OBJECT, &no_pages)
                as usize;
        assert_born_tenured(relocated, "arena_alloc_gc_old_excluding_pages (bump)");

        // The hole-reuse arms: publish a swept-looking hole (no TENURED, as a
        // sweep leaves it) and take it back at the exact size.
        for (path, excluding) in [
            ("arena_alloc_gc_old (hole)", false),
            ("arena_alloc_gc_old_excluding_pages (hole)", true),
        ] {
            let hole = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
            let header = header_from_user_ptr(hole as *const u8);
            (*header).gc_flags = GC_FLAG_ARENA;
            super::super::old_free::old_free_push_for_test(hole, (*header).size as usize);
            let reused = if excluding {
                crate::arena::arena_alloc_gc_old_excluding_pages(64, 8, GC_TYPE_OBJECT, &no_pages)
            } else {
                crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT)
            } as usize;
            assert_eq!(reused, hole, "{path}: premise, the hole is reused");
            assert_born_tenured(reused, path);
        }
    }
    super::super::old_free::old_free_reset_for_test();
}
