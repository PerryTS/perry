//! Safety contract for the shared class initializer's dynamic slot loops.
//! The ordinary invariant walks fixed stores; this one proves the loop bound
//! and dominance before publication, including the allocating null arm.
pub(super) fn check(function: &str) {
    let body = function.split_once('\n').unwrap().1;
    assert!(function.lines().next().unwrap().contains("preserve_mostcc"));
    assert!(function.lines().next().unwrap().contains("noinline"));
    let required = [
        "%size = lshr i64 %packed, 32",
        "%base = phi ptr [ %raw, %entry.0 ], [ %allocated, %slow ]",
        "%flag_field = getelementptr i8, ptr %state, i64 24",
        "%flag_address = load ptr, ptr %flag_field",
        "%flags = load volatile i8, ptr %flag_address",
        "%wide_flags = zext i8 %flags to i64",
        "%color = shl i64 %wide_flags, 8",
        "%born = or i64 %packed, %color",
        "%born_image = insertelement <2 x i64> %image, i64 %born, i32 0",
        "store <2 x i64> %born_image, ptr %base, align 8",
        "%meta = getelementptr i8, ptr %base, i64 16",
        "store i64 0, ptr %meta",
        "%slots_bytes = sub i64 %size, 24",
        "%slots = lshr i64 %slots_bytes, 3",
        "%fields = getelementptr i8, ptr %base, i64 24",
        "%i = phi i64 [ 0, %init ], [ %next, %fill ]",
        "%slot = getelementptr i64, ptr %fields, i64 %i",
        "%next = add i64 %i, 1",
        "%more = icmp ult i64 %next, %slots",
        "br i1 %more, label %fill, label %rep_check",
        "%j = phi i64 [ 0, %rep_check ], [ %j_next, %rep_fill ]",
        "%lanes = phi i64 [ %rep, %rep_check ], [ %remaining, %rep_fill ]",
        "%lane = and i64 %lanes, 3",
        "%f64 = icmp eq i64 %lane, 1",
        "%typed_slot = getelementptr i64, ptr %fields, i64 %j",
        "%remaining = lshr i64 %lanes, 2",
        "%j_next = add i64 %j, 1",
        "%slots_more = icmp ult i64 %j_next, %slots",
        "%typed_more = and i1 %lanes_more, %slots_more",
        "br i1 %typed_more, label %rep_fill, label %ready",
        "%active = icmp ne i8 %flags, 0",
        "br i1 %active, label %seed, label %done",
        "%seed_field = getelementptr i8, ptr %state, i64 32",
        "%seeds = load ptr, ptr %seed_field",
    ];
    for instruction in required {
        assert!(
            body.lines().any(|l| l.trim() == instruction),
            "missing {instruction}"
        );
    }
    assert!(body.contains(&format!(
        "store i64 {}, ptr %slot",
        crate::nanbox::TAG_UNDEFINED_I64
    )));
    assert!(body.contains(&format!(
        "%default = select i1 %f64, i64 0, i64 {}",
        crate::nanbox::TAG_UNDEFINED_I64
    )));
    assert!(body.contains("store i64 %default, ptr %typed_slot"));
    let seed = body
        .find("call void @js_gc_note_black_birth(ptr %base, ptr %seeds)")
        .unwrap();
    assert!(
        seed > body.find("ready:").unwrap(),
        "seed must follow both initialization loops"
    );
    let init = body.find("init:").unwrap();
    assert!(
        body[..init].contains("call ptr @js_inline_arena_slow_alloc(ptr %state, i64 %size, i64 8)")
    );
    assert!(
        !body[init..seed].contains("call "),
        "no collection while initializing"
    );
    assert_eq!(body.matches("call ").count(), 2);
    assert!(body[seed..].contains("%user = getelementptr i8, ptr %base, i64 8"));
    assert!(
        body[..seed]
            .lines()
            .filter(|l| l.trim().starts_with("store "))
            .all(|l| {
                ["ptr %base", "ptr %meta", "ptr %slot", "ptr %typed_slot"]
                    .iter()
                    .any(|target| l.contains(target))
            }),
        "unpublished birth may only write its own storage"
    );
}
