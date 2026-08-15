//! #7683: `js_object_clone_with_extra` published a half-built object whose
//! `keys_array` slot still held whatever the reused hole contained.
//!
//! # The hazard
//!
//! Both branches set `class_id`, `parent_class_id`,
//! `field_count` and `meta` immediately after allocation, then set
//! `keys_array` only at the END, via `set_object_keys_array`. In between sits
//! `crate::array::js_array_alloc`.
//!
//! That call ALLOCATES, so it can collect — and the collector reads exactly
//! this slot as a child edge (`object::gc_keys_array_slot`, enumerated by
//! `gc_child_slots`). A collection landing in that window scans a pointer the
//! mutator never wrote.
//!
//! The bytes are not zero. `arena_alloc_gc_old`'s fast path deliberately
//! reuses a swept, NON-zeroed hole (#7437: "reuse a swept same-size hole …
//! otherwise a block with any live object never yields its dead bytes back"),
//! so the slot holds real leftover heap content. Whether it happens to look
//! like a plausible-but-unmapped address depends on allocation history —
//! the shape of the ~1-in-102 `typed_feedback::object_shape` SIGSEGV in #7683.
//!
//! # Why this test is a SOURCE check and not a runtime one
//!
//! I wrote the runtime version first: force a collection into the window with
//! `force_next_general_arena_alloc_slow` + `GC_OLD_RECLAIM_PENDING`, then
//! assert the published clone's `keys_array` is sane. **It passed with the fix
//! deleted** — vacuous, and for two independent reasons. By the time the
//! function returns, `set_object_keys_array` has written the slot correctly,
//! so nothing observable survives the window; and a fresh arena block is
//! zeroed, so even inside the window the garbage would read as null unless the
//! allocation lands in a recycled old-space hole with the right history.
//!
//! Reproducing it in-suite therefore needs a specific swept-hole layout AND a
//! collection landing in a few-instruction window. That is exactly why the fix
//! is a by-construction initialisation rather than a guard — and why the test
//! that guards it asserts the invariant at the only place it is decidable: the
//! source. Same idea as `scripts/gc_pin_sites.py`'s custody check for
//! `GC_FLAG_PINNED`.

/// Every object-allocating branch of `js_object_clone_with_extra` must
/// initialise `keys_array` BEFORE the `js_array_alloc` that can collect.
#[test]
fn clone_with_extra_initializes_keys_array_before_it_can_collect() {
    let src = include_str!("../../object/alloc.rs");
    let start = src
        .find("pub unsafe extern \"C\" fn js_object_clone_with_extra")
        .expect("js_object_clone_with_extra must exist");
    // The function ends at the next item at column 0 after its body.
    let body = &src[start..];
    let end = body[1..]
        .find("\npub ")
        .map(|i| i + 1)
        .unwrap_or(body.len());
    let body = &body[..end];
    // Strip comments BEFORE scanning. Without this the check matches prose:
    // the very comment this fix added says "arena_alloc_gc_old's fast path",
    // which registered as a third allocation site and made the test fail
    // against correct code. A source-level check that reads its own
    // documentation as code is worse than no check.
    let body: String = body
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.as_str();

    // Each branch allocates, then eventually calls js_array_alloc.
    let alloc_sites: Vec<usize> = body
        .match_indices("= arena_alloc_gc")
        .map(|(i, _)| i)
        .collect();
    assert!(
        !alloc_sites.is_empty(),
        "no arena_alloc_gc site found — this test is anchored on the wrong \
         function or the allocator was renamed; fix the anchor rather than \
         deleting the check"
    );

    for site in alloc_sites {
        let after = &body[site..];
        let init = after.find("keys_array = ptr::null_mut()");
        let collect_point = after.find("js_array_alloc");
        let (Some(init), Some(collect_point)) = (init, collect_point) else {
            panic!(
                "#7683: an allocation site in js_object_clone_with_extra has \
                 no `keys_array = ptr::null_mut()` before its `js_array_alloc`.\n\
                 init={init:?} js_array_alloc={collect_point:?}\n\
                 js_array_alloc can collect, and the collector reads keys_array \
                 as a child edge — so the slot must be written first."
            );
        };
        assert!(
            init < collect_point,
            "#7683: keys_array is initialised AFTER the js_array_alloc that \
             can collect (init at +{init}, js_array_alloc at +{collect_point}). \
             A collection in that window scans an unwritten pointer."
        );
    }
}
