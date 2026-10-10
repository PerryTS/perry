//! One linker-coalesced initialization/slow/color routine per birth kind.
//! The caller commits only the Eden bump and the base header. A null raw
//! pointer requests slow allocation; no object exists across that call.
use crate::expr::FnCtx;
use crate::function::LlFunction;
use crate::types::{I64, I8, PTR};

const CLASS: &str = "perry_birth_class";
const EMPTY: &str = "perry_birth_empty_array";

fn publish(ctx: &mut FnCtx<'_>, name: &str, params: Vec<(&'static str, String)>, body: String) {
    if ctx.pending_helpers.iter().any(|f| f.name == name) {
        return;
    }
    let mut f = LlFunction::new(name, I64, params);
    f.linkage = "linkonce_odr".into();
    f.no_inline = true;
    let entry = f.create_block("entry");
    // Multiline Raw is split into individual native-dialect items by
    // LlFunction::inst_item; labels retain their unindented spelling.
    entry.emit_raw(body.replace("%entry ]", "%entry.0 ]"));
    ctx.pending_helpers.push(f);
}

pub(crate) fn class(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    state: &str,
    image: &str,
    rep: u64,
    leaf: bool,
) -> String {
    publish(
        ctx,
        CLASS,
        vec![
            (PTR, "%raw".into()),
            (PTR, "%state".into()),
            ("<2 x i64>", "%image".into()),
            (I64, "%rep".into()),
        ],
        format!(
            r#"
  %packed = extractelement <2 x i64> %image, i32 0
  %size = lshr i64 %packed, 32
  %missing = icmp eq ptr %raw, null
  br i1 %missing, label %slow, label %init
slow:
  %allocated = call ptr @js_inline_arena_slow_alloc(ptr %state, i64 %size, i64 8)
  br label %init
init:
  %base = phi ptr [ %raw, %entry ], [ %allocated, %slow ]
  %flag_field = getelementptr i8, ptr %state, i64 24
  %flag_address = load ptr, ptr %flag_field
  %flags = load volatile i8, ptr %flag_address
  %wide_flags = zext i8 %flags to i64
  %color = shl i64 %wide_flags, 8
  %born = or i64 %packed, %color
  %born_image = insertelement <2 x i64> %image, i64 %born, i32 0
  ; GC_STORE_AUDIT(INIT): fresh unpublished header and meta.
  store <2 x i64> %born_image, ptr %base, align 8
  %meta = getelementptr i8, ptr %base, i64 16
  store i64 0, ptr %meta
  %slots_bytes = sub i64 %size, 24
  %slots = lshr i64 %slots_bytes, 3
  %fields = getelementptr i8, ptr %base, i64 24
  br label %fill
fill:
  %i = phi i64 [ 0, %init ], [ %next, %fill ]
  %slot = getelementptr i64, ptr %fields, i64 %i
  ; GC_STORE_AUDIT(INIT): every fresh slot gets a valid Any default.
  store i64 {undefined}, ptr %slot
  %next = add i64 %i, 1
  %more = icmp ult i64 %next, %slots
  br i1 %more, label %fill, label %rep_check
rep_check:
  %typed = icmp ne i64 %rep, 0
  br i1 %typed, label %rep_fill, label %ready
rep_fill:
  %j = phi i64 [ 0, %rep_check ], [ %j_next, %rep_fill ]
  %lanes = phi i64 [ %rep, %rep_check ], [ %remaining, %rep_fill ]
  %lane = and i64 %lanes, 3
  %f64 = icmp eq i64 %lane, 1
  %default = select i1 %f64, i64 0, i64 {undefined}
  %typed_slot = getelementptr i64, ptr %fields, i64 %j
  ; GC_STORE_AUDIT(INIT): F64 lanes begin at +0.0 before publication.
  store i64 %default, ptr %typed_slot
  %remaining = lshr i64 %lanes, 2
  %j_next = add i64 %j, 1
  %lanes_more = icmp ne i64 %remaining, 0
  %slots_more = icmp ult i64 %j_next, %slots
  %typed_more = and i1 %lanes_more, %slots_more
  br i1 %typed_more, label %rep_fill, label %ready
ready:
  %active = icmp ne i8 %flags, 0
  br i1 %active, label %seed, label %done
seed:
  %seed_field = getelementptr i8, ptr %state, i64 32
  %seeds = load ptr, ptr %seed_field
  call void @js_gc_note_black_birth(ptr %base, ptr %seeds)
  br label %done
done:
  %user = getelementptr i8, ptr %base, i64 8
  %handle = ptrtoint ptr %user to i64
  ret i64 %handle
"#,
            undefined = crate::nanbox::TAG_UNDEFINED_I64
        ),
    );
    let rep = rep.to_string();
    let args = [(PTR, raw), (PTR, state), ("<2 x i64>", image), (I64, &rep)];
    // A non-null raw pointer proves the helper cannot take its allocating
    // branch. Its remaining initialization/color/seed operations are leaf.
    if leaf {
        ctx.block().call_gc_leaf(I64, CLASS, &args)
    } else {
        ctx.block().call(I64, CLASS, &args)
    }
}

pub(crate) fn empty_array(ctx: &mut FnCtx<'_>) -> String {
    let state = crate::expr::load_inline_arena_state(ctx);
    let blk = ctx.block();
    let offset_field = blk.gep(I8, &state, &[(I64, "8")]);
    let offset = blk.load(I64, &offset_field);
    let next = blk.add(I64, &offset, "48");
    let limit_field = blk.gep(I8, &state, &[(I64, "16")]);
    let limit = blk.load(I64, &limit_field);
    let fits = blk.icmp_ule(I64, &next, &limit);
    let fast = ctx.new_block("arrlit.fast");
    let slow = ctx.new_block("arrlit.slow");
    let merge = ctx.new_block("arrlit.merge");
    let fast_label = ctx.block_label(fast);
    let merge_label = ctx.block_label(merge);
    let slow_label = ctx.block_label(slow);
    ctx.block().cond_br(&fits, &fast_label, &slow_label);
    ctx.current_block = fast;
    let blk = ctx.block();
    // GC_STORE_AUDIT(INIT): arena offset is allocator metadata.
    blk.store(I64, &next, &offset_field);
    let data = blk.load(PTR, &state);
    let raw = blk.gep(I8, &data, &[(I64, &offset)]);
    // GC_STORE_AUDIT(INIT): base header of unpublished empty array.
    blk.store(I64, "207232172545", &raw);
    publish(
        ctx,
        EMPTY,
        vec![(PTR, "%raw".into()), (PTR, "%state".into())],
        format!(
            r#"
  %missing = icmp eq ptr %raw, null
  br i1 %missing, label %slow, label %init
slow:
  %allocated = call ptr @js_inline_arena_slow_alloc(ptr %state, i64 48, i64 8)
  br label %init
init:
  %base = phi ptr [ %raw, %entry ], [ %allocated, %slow ]
  %flag_field = getelementptr i8, ptr %state, i64 24
  %flag_address = load ptr, ptr %flag_field
  %flags = load volatile i8, ptr %flag_address
  %wide_flags = zext i8 %flags to i64
  %color = shl i64 %wide_flags, 8
  %born = or i64 207232172545, %color
  ; GC_STORE_AUDIT(INIT): header, length/capacity and all four hole slots.
  store i64 %born, ptr %base
  %array = getelementptr i8, ptr %base, i64 8
  store i64 17179869184, ptr %array
  %s0 = getelementptr i8, ptr %base, i64 16
  store i64 {hole}, ptr %s0
  %s1 = getelementptr i8, ptr %base, i64 24
  store i64 {hole}, ptr %s1
  %s2 = getelementptr i8, ptr %base, i64 32
  store i64 {hole}, ptr %s2
  %s3 = getelementptr i8, ptr %base, i64 40
  store i64 {hole}, ptr %s3
  %active = icmp ne i8 %flags, 0
  br i1 %active, label %seed, label %done
seed:
  %seed_field = getelementptr i8, ptr %state, i64 32
  %seeds = load ptr, ptr %seed_field
  call void @js_gc_note_black_birth(ptr %base, ptr %seeds)
  br label %done
done:
  %handle = ptrtoint ptr %array to i64
  ret i64 %handle
"#,
            hole = crate::nanbox::TAG_HOLE
        ),
    );
    let fast_handle = ctx
        .block()
        .call_gc_leaf(I64, EMPTY, &[(PTR, &raw), (PTR, &state)]);
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = slow;
    let slow_handle = ctx
        .block()
        .call(I64, EMPTY, &[(PTR, "null"), (PTR, &state)]);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    ctx.block().phi(
        I64,
        &[(&fast_handle, &fast_pred), (&slow_handle, &slow_pred)],
    )
}
