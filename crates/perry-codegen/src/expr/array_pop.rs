//! Inline `arr.pop()` for a receiver the HIR admits as an Array.
//!
//! `js_array_pop_f64`'s own fast path is a handful of header tests and one
//! length store, but it is reached through a call, a heap-address
//! classification (`try_read_gc_header`) and — for anything it declines — a
//! tower of subclass / plain-object probes. In the wolf-ecs entity cycle the
//! plain-array `packed.pop()` of `SparseSet.remove` and `createEntity` is 8–10%
//! of self time with nothing but that fast path running (Linux `perf`, 10 s
//! twins). This emits the same decision inline, exactly as the runtime makes
//! it, and keeps the runtime call as the fallback for everything else:
//!
//! * a `POINTER`-tagged heap handle (the receiver may be a claimed, not
//!   proven, Array: a String or a NaN-boxed number takes the call);
//! * `GC_TYPE_ARRAY`, not forwarded (`GC_FLAG_FORWARDED`), none of
//!   `FROZEN | SEALED | NO_EXTEND | ARRAY_DESCRIPTORS` in `_reserved` — a
//!   frozen array or a non-writable `length` must throw, a descriptor-bearing
//!   one needs the descriptor-aware `[[Delete]]`;
//! * `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED == 0` — a polluted
//!   `Array.prototype` can intercept the element read;
//! * `0 < length <= capacity <= 100_000_000` and the popped slot is not the
//!   hole sentinel (a hole reads through the prototype chain).
//!
//! On the inline arm the element is read, `length` is decremented and the
//! element returned. Nothing is allocated and no pointer is stored (the
//! length word is an `i32`), so there is no GC bookkeeping: the runtime's
//! fast path performs none either. An empty array takes the call (it answers
//! `undefined`, and that arm is the runtime's to keep fast).
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

use super::{unbox_to_i64, FnCtx};

/// `FROZEN | SEALED | NO_EXTEND | ARRAY_DESCRIPTORS` in the GcHeader's
/// `_reserved` word (0x407) — the same mask `js_array_pop_f64` tests.
const POP_BLOCKING_FLAGS_I16: &str = "1031";
const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD
const HANDLE_BAND_TOP: &str = "1048575"; // 0x0FFFFF — heap objects are above
const HEAP_LIMIT: &str = "140737488355328"; // 2^47
const GC_TYPE_ARRAY_I8: &str = "1";
const GC_FLAG_FORWARDED_I8: &str = "-128"; // 0x80 as i8
const MAX_FAST_LENGTH_I32: &str = "100000000";

/// Lower `recv.pop()` for an Array-admitted receiver: the inline tier above
/// with `js_array_pop_f64` behind it. Returns the popped element (boxed).
pub(crate) fn lower_array_pop_inline(ctx: &mut FnCtx<'_>, recv_box: &str) -> String {
    let hdr_idx = ctx.new_block("apop.hdr");
    let len_idx = ctx.new_block("apop.len");
    let read_idx = ctx.new_block("apop.read");
    let take_idx = ctx.new_block("apop.take");
    let slow_idx = ctx.new_block("apop.slow");
    let merge_idx = ctx.new_block("apop.merge");
    let hdr_label = ctx.block_label(hdr_idx);
    let len_label = ctx.block_label(len_idx);
    let read_label = ctx.block_label(read_idx);
    let take_label = ctx.block_label(take_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let handle = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let tag = blk.lshr(I64, &bits, "48");
        let is_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let handle = unbox_to_i64(blk, recv_box);
        let above_band = blk.icmp_ugt(I64, &handle, HANDLE_BAND_TOP);
        let below_heap = blk.icmp_ult(I64, &handle, HEAP_LIMIT);
        let in_heap = blk.and(I1, &above_band, &below_heap);
        let heap_candidate = blk.and(I1, &is_ptr, &in_heap);
        blk.cond_br(&heap_candidate, &hdr_label, &slow_label);
        handle
    };

    ctx.current_block = hdr_idx;
    {
        let blk = ctx.block();
        // GcHeader precedes the array: obj_type @-8 (i8), gc_flags @-7 (i8),
        // _reserved @-6 (i16).
        let gc_type_addr = blk.sub(I64, &handle, "8");
        let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
        let gc_type = blk.load(I8, &gc_type_ptr);
        let is_array = blk.icmp_eq(I8, &gc_type, GC_TYPE_ARRAY_I8);
        let gc_flags_addr = blk.sub(I64, &handle, "7");
        let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
        let gc_flags = blk.load(I8, &gc_flags_ptr);
        let fwd_bits = blk.and(I8, &gc_flags, GC_FLAG_FORWARDED_I8);
        let not_fwd = blk.icmp_eq(I8, &fwd_bits, "0");
        let reserved_addr = blk.sub(I64, &handle, "6");
        let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
        let reserved = blk.load(I16, &reserved_ptr);
        let blocking = blk.and(I16, &reserved, POP_BLOCKING_FLAGS_I16);
        let plain = blk.icmp_eq(I16, &blocking, "0");
        let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
        let prototype_clean = blk.icmp_eq(I8, &invalidated, "0");
        let mut ok = blk.and(I1, &is_array, &not_fwd);
        ok = blk.and(I1, &ok, &plain);
        ok = blk.and(I1, &ok, &prototype_clean);
        blk.cond_br(&ok, &len_label, &slow_label);
    }

    ctx.current_block = len_idx;
    let new_length = {
        let blk = ctx.block();
        // ArrayHeader: length @0 (i32), capacity @4 (i32), elements @8.
        let length = blk.safe_load_i32_from_ptr(&handle);
        let cap_addr = blk.add(I64, &handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        let capacity = blk.load(I32, &cap_ptr);
        let nonempty = blk.icmp_ne(I32, &length, "0");
        let within_capacity = blk.icmp_ule(I32, &length, &capacity);
        let sane = blk.icmp_ule(I32, &length, MAX_FAST_LENGTH_I32);
        let mut ok = blk.and(I1, &nonempty, &within_capacity);
        ok = blk.and(I1, &ok, &sane);
        let new_length = blk.sub(I32, &length, "1");
        blk.cond_br(&ok, &read_label, &slow_label);
        new_length
    };

    ctx.current_block = read_idx;
    let elem = {
        let blk = ctx.block();
        let new_length_i64 = blk.zext(I32, &new_length, I64);
        let elem_off = blk.shl(I64, &new_length_i64, "3");
        let elements_addr = blk.add(I64, &handle, "8");
        let elem_addr = blk.add(I64, &elements_addr, &elem_off);
        let elem_ptr = blk.inttoptr(I64, &elem_addr);
        let elem = blk.load(DOUBLE, &elem_ptr);
        let elem_bits = blk.bitcast_double_to_i64(&elem);
        let is_hole = blk.icmp_eq(I64, &elem_bits, crate::nanbox::TAG_HOLE_I64);
        blk.cond_br(&is_hole, &slow_label, &take_label);
        elem
    };

    ctx.current_block = take_idx;
    {
        let blk = ctx.block();
        let len_ptr = blk.inttoptr(I64, &handle);
        // `length` is a plain i32 word: no pointer, no barrier, no layout
        // note — exactly the runtime fast path's single store.
        blk.store(I32, &new_length, &len_ptr);
        blk.br(&merge_label);
    }
    let take_end = take_label.clone();

    ctx.current_block = slow_idx;
    let slow = {
        let blk = ctx.block();
        blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &handle)])
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&elem, &take_end), (&slow, &slow_end)])
}

#[cfg(test)]
mod tests {
    use crate::compile_module;
    use perry_hir::types::Type;
    use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Stmt};

    fn method(body: Vec<Stmt>) -> Function {
        Function {
            id: 720,
            name: "take".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    /// `class Stack { items = []; take() { return this.items.pop() } }` — the
    /// erased-field receiver, which the HIR classifies as
    /// `NativeMethodCall { module: "array", method: "pop" }`.
    fn field_pop_module() -> Module {
        let take = method(vec![Stmt::Return(Some(Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::PropertyGet {
                object: Box::new(Expr::This),
                property: "items".to_string(),
                byte_offset: 0,
            })),
            method: "pop".to_string(),
            args: Vec::new(),
        }))]);
        let class = Class {
            id: 406,
            name: "Stack".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: vec![ClassField {
                name: "items".to_string(),
                key_expr: None,
                ty: Type::Array(Box::new(Type::Number)),
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            }],
            constructor: None,
            methods: vec![take],
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        };
        let mut m = Module::new("array_pop_inline.ts");
        m.classes = vec![class];
        m.init = vec![Stmt::Expr(Expr::New {
            class_name: "Stack".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        })];
        m.init_kind = ModuleInitKind::Eager;
        m
    }

    /// `function take(xs: number[]) { return xs.pop() }` — the claimed-array
    /// receiver route (`lower_call/property_get.rs` → `lower_array_method`).
    fn param_pop_module() -> Module {
        const XS: u32 = 5;
        let mut m = Module::new("array_pop_inline_param.ts");
        m.functions = vec![Function {
            id: 721,
            name: "take".to_string(),
            type_params: Vec::new(),
            params: vec![perry_hir::Param {
                id: XS,
                name: "xs".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(XS)),
                    property: "pop".to_string(),
                    byte_offset: 0,
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }))],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }];
        m.init = vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(721)),
            args: vec![Expr::Array(vec![Expr::Number(1.0)])],
            type_args: Vec::new(),
            byte_offset: 0,
        })];
        m.init_kind = ModuleInitKind::Eager;
        m
    }

    fn ir_of(m: Module) -> String {
        String::from_utf8(
            compile_module(&m, super::super::class_field_barrier_tests::ir_opts())
                .expect("module compiles"),
        )
        .expect("LLVM IR should be UTF-8")
    }

    fn assert_inline_pop_tier(ir: &str, what: &str) {
        for label in [
            "apop.hdr",
            "apop.len",
            "apop.read",
            "apop.take",
            "apop.slow",
        ] {
            assert!(
                ir.contains(label),
                "{what}: block {label} must exist:\n{ir}"
            );
        }
        let hdr = super::super::class_field_barrier_tests::block_body(ir, "apop.hdr.")
            .expect("header block");
        assert!(
            hdr.contains(", 1031") && hdr.contains("icmp eq i8") && hdr.contains(", 1\n"),
            "{what}: the header gate must test GC_TYPE_ARRAY and the 0x407 integrity mask:\n{hdr}"
        );
        assert!(
            hdr.contains("@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED"),
            "{what}: the header gate must test the prototype-pollution latch:\n{hdr}"
        );
        let read = super::super::class_field_barrier_tests::block_body(ir, "apop.read.")
            .expect("read block");
        assert!(
            read.contains(crate::nanbox::TAG_HOLE_I64),
            "{what}: a hole must take the runtime route:\n{read}"
        );
        let take = super::super::class_field_barrier_tests::block_body(ir, "apop.take.")
            .expect("take block");
        assert!(
            take.contains("store i32") && !take.contains("call "),
            "{what}: the inline arm is one length store and no call:\n{take}"
        );
        let slow = super::super::class_field_barrier_tests::block_body(ir, "apop.slow.")
            .expect("slow block");
        assert!(
            slow.contains("call double @js_array_pop_f64("),
            "{what}: the runtime pop must remain the fallback:\n{slow}"
        );
    }

    /// Both `pop` routes — the erased class-field receiver
    /// (`NativeMethodCall`) and the claimed-array receiver
    /// (`lower_array_method`) — emit the inline tier with the runtime call
    /// behind it, mirroring `js_array_pop_f64`'s own admission exactly.
    #[test]
    fn array_pop_takes_the_inline_tier_with_the_runtime_call_as_fallback() {
        assert_inline_pop_tier(&ir_of(field_pop_module()), "erased field receiver");
        assert_inline_pop_tier(&ir_of(param_pop_module()), "claimed array parameter");
    }
}
