//! Inline shadow-stack frame push and pop on single-threaded wasm.
//!
//! On every target a function with GC roots opens its frame with
//! `js_shadow_frame_enter(n)` and closes it with `js_shadow_frame_pop(h)`. On
//! wasm both are cross-module calls, and `enter` is a TLS lookup plus a
//! capacity check, a header write, a constant four-slot clear and two state
//! updates: about 126 wasm instructions per rooted call, more than many of the
//! function bodies that pay it.
//!
//! A wasm module without `atomics` has one thread, so the runtime keeps the
//! shadow state in the exported static `PERRY_SHADOW_STATE` (see
//! `perry-runtime`'s `gc::roots::shadow_stack`). This pass points each frame
//! operation at a small `internal alwaysinline` helper that performs it on that
//! static directly: the push zeroes exactly the frame's `n` slots, and the
//! runtime functions remain the slow arms (buffer growth; a handle that does
//! not name a live frame), so the runtime stays the one authority for both.
//! The helpers return and store exactly what the runtime functions do, so a
//! frame pushed inline and one pushed by the runtime are indistinguishable to
//! the collector and to `js_shadow_frame_restore_to`.

use crate::expr::shadow_inline::{
    SHADOW_ENTRY_META_OFFSET, SHADOW_ENTRY_SHIFT, SHADOW_STACK_HEADER_SLOTS,
    SHADOW_STATE_CAP_OFFSET, SHADOW_STATE_FRAME_TOP_OFFSET, SHADOW_STATE_LEN_OFFSET,
    SHADOW_STATE_PTR_OFFSET,
};

const ENTER: &str = "@js_shadow_frame_enter(";
const POP: &str = "@js_shadow_frame_pop(";
const INLINE_ENTER: &str = "@__perry_shadow_frame_enter(";
const INLINE_POP: &str = "@__perry_shadow_frame_pop(";

/// `ShadowStackState` is four pointer-sized words; the codegen contract
/// publishes the LP64 offsets, and on wasm32 each word is half as wide.
const fn ilp32(lp64_offset: u64) -> u64 {
    lp64_offset / 2
}

/// Route every frame push and pop in `ir` through the inline helpers, and
/// append them. The identity on a module with no frame operations.
pub(crate) fn inline_frame_ops(ir: &str) -> String {
    let mut used = false;
    let mut out = String::with_capacity(ir.len() + 2048);
    for line in ir.split_inclusive('\n') {
        if line.starts_with("declare ") {
            out.push_str(line);
            continue;
        }
        let rewritten = line
            .replace(
                &format!("call ptr {ENTER}"),
                &format!("call ptr {INLINE_ENTER}"),
            )
            .replace(
                &format!("call void {POP}"),
                &format!("call void {INLINE_POP}"),
            );
        used |= rewritten.len() != line.len();
        out.push_str(&rewritten);
    }
    if !used {
        return ir.to_string();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // A declaration may carry attributes (`declare nonnull ptr @...`), so look
    // for the symbol, not the line.
    for (symbol, decl) in [
        (
            "@js_shadow_frame_enter(",
            "declare ptr @js_shadow_frame_enter(i32)",
        ),
        (
            "@js_shadow_frame_pop(",
            "declare void @js_shadow_frame_pop(i64)",
        ),
        (
            "@llvm.memset.p0.i32(",
            "declare void @llvm.memset.p0.i32(ptr, i8, i32, i1)",
        ),
    ] {
        if !out
            .lines()
            .any(|l| l.starts_with("declare ") && l.contains(symbol))
        {
            out.push_str(decl);
            out.push('\n');
        }
    }
    out.push_str(&helpers());
    out
}

fn helpers() -> String {
    let (ptr, len, cap, top) = (
        ilp32(SHADOW_STATE_PTR_OFFSET),
        ilp32(SHADOW_STATE_LEN_OFFSET),
        ilp32(SHADOW_STATE_CAP_OFFSET),
        ilp32(SHADOW_STATE_FRAME_TOP_OFFSET),
    );
    let (shift, header) = (SHADOW_ENTRY_SHIFT, SHADOW_STACK_HEADER_SLOTS);
    format!(
        r#"
@PERRY_SHADOW_STATE = external global [16 x i8], align 4

define internal ptr @__perry_shadow_frame_enter(i32 %n) alwaysinline {{
entry:
  %len.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {len}
  %cap.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {cap}
  %top.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {top}
  %len = load i32, ptr %len.p
  %cap = load i32, ptr %cap.p
  %need = add i32 %n, {header}
  %room = sub i32 %cap, %len
  %fits = icmp uge i32 %room, %need
  br i1 %fits, label %push, label %grow
grow:
  %grown = call ptr @js_shadow_frame_enter(i32 %n)
  ret ptr %grown
push:
  %buf.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {ptr}
  %buf = load ptr, ptr %buf.p
  %base.off = shl i32 %len, {shift}
  %hdr = getelementptr inbounds i8, ptr %buf, i32 %base.off
  %caller.top = load i32, ptr %top.p
  %caller.top.w = zext i32 %caller.top to i64
  store i64 %caller.top.w, ptr %hdr
  %meta.p = getelementptr inbounds i8, ptr %hdr, i32 {meta}
  store i32 %n, ptr %meta.p
  %slots = getelementptr inbounds i8, ptr %hdr, i32 {header_bytes}
  %slot.bytes = shl i32 %n, {shift}
  call void @llvm.memset.p0.i32(ptr align 8 %slots, i8 0, i32 %slot.bytes, i1 false)
  %new.top = add i32 %len, {header}
  %new.len = add i32 %new.top, %n
  store i32 %new.len, ptr %len.p
  store i32 %new.top, ptr %top.p
  ret ptr @PERRY_SHADOW_STATE
}}

define internal void @__perry_shadow_frame_pop(i64 %handle) alwaysinline {{
entry:
  %len.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {len}
  %len = load i32, ptr %len.p
  %len.w = zext i32 %len to i64
  %live = icmp ult i64 %handle, %len.w
  br i1 %live, label %pop, label %report
report:
  call void @js_shadow_frame_pop(i64 %handle)
  ret void
pop:
  %base = trunc i64 %handle to i32
  %buf.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {ptr}
  %buf = load ptr, ptr %buf.p
  %base.off = shl i32 %base, {shift}
  %hdr = getelementptr inbounds i8, ptr %buf, i32 %base.off
  %caller.top.w = load i64, ptr %hdr
  %caller.top = trunc i64 %caller.top.w to i32
  %top.p = getelementptr inbounds i8, ptr @PERRY_SHADOW_STATE, i32 {top}
  store i32 %caller.top, ptr %top.p
  store i32 %base, ptr %len.p
  ret void
}}
"#,
        meta = SHADOW_ENTRY_META_OFFSET,
        header_bytes = header << shift,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = "declare nonnull ptr @js_shadow_frame_enter(i32)
declare void @js_shadow_frame_pop(i64)

define double @f() {
entry.0:
  %r3 = call ptr @js_shadow_frame_enter(i32 2)
  %shadow_pop_l_0 = load i64, ptr %r1
  call void @js_shadow_frame_pop(i64 %shadow_pop_l_0)
  ret double 0.0
}
";

    #[test]
    fn frame_ops_route_through_inline_helpers_whose_slow_arms_are_the_runtime() {
        let out = inline_frame_ops(MODULE);
        assert!(
            out.contains("%r3 = call ptr @__perry_shadow_frame_enter(i32 2)"),
            "{out}"
        );
        assert!(
            out.contains("call void @__perry_shadow_frame_pop(i64 %shadow_pop_l_0)"),
            "{out}"
        );
        // The only remaining runtime calls are the helpers' slow arms.
        assert_eq!(
            out.matches("call ptr @js_shadow_frame_enter(").count(),
            1,
            "{out}"
        );
        assert_eq!(
            out.matches("call void @js_shadow_frame_pop(").count(),
            1,
            "{out}"
        );
        assert_eq!(
            out.matches("@js_shadow_frame_enter(i32)\n").count(),
            1,
            "{out}"
        );
        assert!(out.contains("declare void @llvm.memset.p0.i32("), "{out}");
        assert!(
            out.contains("@PERRY_SHADOW_STATE = external global"),
            "{out}"
        );
        // ILP32 offsets: ptr 0, len 4, cap 8, frame_top 12; slots follow the
        // one 16-byte header entry, and the clear spans exactly `n` entries.
        assert!(out.contains("ptr @PERRY_SHADOW_STATE, i32 4\n"), "{out}");
        assert!(out.contains("ptr @PERRY_SHADOW_STATE, i32 8\n"), "{out}");
        assert!(out.contains("ptr @PERRY_SHADOW_STATE, i32 12\n"), "{out}");
        assert!(
            out.contains("%slots = getelementptr inbounds i8, ptr %hdr, i32 16"),
            "{out}"
        );
        assert!(out.contains("%slot.bytes = shl i32 %n, 4"), "{out}");
    }

    #[test]
    fn a_module_without_frames_is_unchanged() {
        let ir = "define void @g() {\nentry:\n  ret void\n}\n";
        assert_eq!(inline_frame_ops(ir), ir);
    }
}
