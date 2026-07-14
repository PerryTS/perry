//! `volatile` promotion for the allocas a `try` body mutates (#6385).
//!
//! Perry lowers `try`/`catch` to `setjmp`/`longjmp` (see `stmt/try_stmt.rs`),
//! not to LLVM unwind edges. `longjmp` restores the callee-saved registers and
//! the stack pointer that `setjmp` snapshotted — so any value LLVM decided to
//! keep in a register across the `setjmp` call reverts to its setjmp-time
//! contents when the exception fires. C spells the consequence out in
//! 7.13.2.1p3: an automatic object that is *modified between the `setjmp` and
//! the `longjmp`* and read afterwards has an indeterminate value unless it is
//! declared `volatile`.
//!
//! Perry's locals are alloca-backed, and at `-O2` `mem2reg`/`SROA` promote
//! those allocas to SSA registers. So the C hazard is exactly our hazard:
//!
//! ```text
//! let acc = 0;
//! try { acc = 41; throw e; }   // store promoted into a callee-saved reg
//! catch { acc += 1; }          // longjmp reverted the reg → acc reads 0
//! ```
//!
//! Historically Perry avoided this by stamping `optnone` on the **whole
//! function** containing the `try`. That is correct (at `-O0` every value is
//! spilled to the frame, and the frame survives `longjmp`) but it is a
//! sledgehammer: merely *having* a `try` — even one that never throws — cost a
//! 5x slowdown on the surrounding code, because the loop counters, the
//! arithmetic, the compares and the branches all stopped being optimized too.
//!
//! This module implements the `volatile` rule instead. `mem2reg` and `SROA`
//! both refuse to promote an alloca that has any volatile load/store
//! (`isAllocaPromotable` / SROA's slice analysis bail on `isVolatile()`), and
//! volatile accesses can be neither elided nor reordered against each other, so
//! the value provably lives in the frame across the `setjmp`. Everything else
//! in the function stays fully optimizable.
//!
//! # The volatile set — and why it is sound
//!
//! `LlBlock::emit` records the destination pointer of **every `store`
//! instruction emitted while codegen is inside a setjmp-protected region**
//! (`RegCounter::enter_try_region` / `exit_try_region`, driven from
//! `lower_try` and the async-boundary lowering). That recorded set is a
//! superset of "automatic objects this function modifies between a `setjmp` and
//! its `longjmp`", because:
//!
//! * The region is tracked by **emission depth**, not by block index, so it
//!   automatically covers nested blocks, loops, nested `try`s, and the
//!   duplicated finally bodies — anything lowered while the region is open,
//!   regardless of which basic block the instruction lands in.
//! * We drop the "…and read after the longjmp" half of the C condition. Marking
//!   an alloca that is written in the try but never read afterwards is merely
//!   conservative, never wrong.
//! * Every access to a marked alloca is upgraded, function-wide — not just the
//!   ones inside the try — because promotion is an all-or-nothing property of
//!   the alloca.
//! * Pointers *derived* from an alloca (`getelementptr` into an
//!   `alloca [N x double]`) are resolved back to their base alloca, so a store
//!   through a derived pointer marks the whole object.
//!
//! Conversely, an alloca this function never stores to inside a try region
//! cannot have been modified between the `setjmp` and the `longjmp` **by this
//! frame**, and the only other way to modify it is for its address to escape to
//! a callee — which by itself already defeats `mem2reg`/`SROA` (an alloca with
//! a non-load/store user is not promotable), so those stay in memory anyway.
//!
//! Values that are *not* memory need no help: an SSA value defined inside the
//! try body cannot be read from the catch block (it does not dominate it), and
//! an SSA value defined *before* the `setjmp` and used after it is live across
//! the call, so the register allocator either spills it to the frame (which
//! `longjmp` preserves) or parks it in a callee-saved register (which `longjmp`
//! restores to the same, unmodified value). Module globals are memory, and
//! every try region is bracketed by opaque runtime calls (`js_try_push`,
//! `_setjmp`, `js_try_end`, `js_throw`) that LLVM must assume clobber them, so
//! they are never cached in a register across the boundary either.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// `PERRY_SETJMP_VOLATILE=0` / `off` / `false` turns the promotion OFF.
///
/// **This produces miscompiled code and exists only to bisect/falsify.** With
/// the promotion disabled, a value written in a `try` body and read in the
/// `catch` silently reverts to its pre-`setjmp` value at `-O2`. It is here so
/// the guarantee this module provides is *testable*: build once, run
/// `test-files/test_gap_try_setjmp_volatile.ts` with and without the flag —
/// green with, red without. A test that passes both ways proves nothing.
///
/// (Same spirit as `PERRY_WRITE_BARRIERS=0` and `PERRY_GEN_GC=0`: a
/// deliberately-unsound switch, for A/B only.)
fn promotion_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var("PERRY_SETJMP_VOLATILE") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false"),
        Err(_) => true,
    })
}

/// Destination pointer operand of a `store` line.
///
/// `store double %r4, ptr %r3, align 8` → `%r3`. Uses the LAST `, ptr `
/// so that `store ptr %v, ptr %slot` (a pointer value stored into a slot)
/// still yields the destination rather than the value.
pub(crate) fn store_dest_ptr(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if !t.starts_with("store ") {
        return None;
    }
    let at = t.rfind(", ptr ")?;
    Some(first_operand(&t[at + ", ptr ".len()..]))
}

/// Source pointer operand of a `load` line.
/// `%r5 = load double, ptr %r3, align 8` → `%r3`.
fn load_src_ptr(t: &str) -> Option<&str> {
    let at = t.find(" = load ")?;
    let rest = &t[at + " = load ".len()..];
    let p = rest.rfind(", ptr ")?;
    Some(first_operand(&rest[p + ", ptr ".len()..]))
}

/// `%r3 = alloca double` / `%r3 = alloca [8 x double], align 8` → `%r3`.
fn alloca_result(t: &str) -> Option<&str> {
    let at = t.find(" = alloca ")?;
    let res = &t[..at];
    res.starts_with('%').then_some(res)
}

/// A pointer-producing instruction whose base is another pointer:
/// `%r9 = getelementptr inbounds [8 x double], ptr %r3, i64 0, i64 2`
/// → `(%r9, %r3)`. The base is the FIRST `, ptr ` operand.
fn derived_ptr(t: &str) -> Option<(&str, &str)> {
    let at = t.find(" = getelementptr ")?;
    let res = &t[..at];
    if !res.starts_with('%') {
        return None;
    }
    let rest = &t[at + " = getelementptr ".len()..];
    let p = rest.find(", ptr ")?;
    Some((res, first_operand(&rest[p + ", ptr ".len()..])))
}

/// First whitespace/comma-delimited token of `s`.
fn first_operand(s: &str) -> &str {
    let end = s
        .find(|c: char| c == ',' || c.is_whitespace())
        .unwrap_or(s.len());
    &s[..end]
}

/// Rewrite `ir` so every load/store touching an alloca that the try region
/// stores into carries `volatile`.
///
/// `try_stores` are the raw pointer operands recorded at emit time
/// (see [`store_dest_ptr`]); most are alloca registers, but the set may also
/// contain globals and heap pointers, which are filtered out here.
pub(crate) fn apply_setjmp_volatile(ir: &str, try_stores: &HashSet<String>) -> String {
    if !promotion_enabled() {
        return ir.to_string();
    }
    let lines: Vec<&str> = ir.lines().collect();

    // 1. Every alloca defined in this function is its own base.
    let mut base: HashMap<&str, &str> = HashMap::new();
    for l in &lines {
        let t = l.trim_start();
        if let Some(r) = alloca_result(t) {
            base.insert(r, r);
        }
    }
    if base.is_empty() {
        return ir.to_string();
    }

    // 2. Resolve derived pointers back to their base alloca. Blocks are
    //    rendered in creation order, which is not guaranteed to be a
    //    dominator order, so a single linear pass can miss a `getelementptr`
    //    whose base is defined further down the text. Iterate to a fixpoint.
    loop {
        let mut changed = false;
        for l in &lines {
            let t = l.trim_start();
            if let Some((res, src)) = derived_ptr(t) {
                if !base.contains_key(res) {
                    if let Some(&b) = base.get(src) {
                        base.insert(res, b);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // 3. Allocas the try region writes — directly or through a derived pointer.
    let mut volatile_allocas: HashSet<&str> = HashSet::new();
    for p in try_stores {
        if let Some(&b) = base.get(p.as_str()) {
            volatile_allocas.insert(b);
        }
    }
    if volatile_allocas.is_empty() {
        return ir.to_string();
    }

    // 4. Upgrade EVERY access to those allocas, function-wide — not just the
    //    ones inside the try. Suppressing promotion only takes one volatile
    //    access (mem2reg/SROA bail on any `isVolatile()` user), but the
    //    *individual* accesses must be volatile too: a plain load in the catch
    //    block could otherwise be forwarded by GVN from a plain store that
    //    dominates the setjmp, reintroducing the stale read we are fixing.
    let mut out = String::with_capacity(ir.len() + 128);
    for l in &lines {
        out.push_str(&upgrade(l, &base, &volatile_allocas));
        out.push('\n');
    }
    out
}

fn is_volatile_target(
    ptr: &str,
    base: &HashMap<&str, &str>,
    volatile_allocas: &HashSet<&str>,
) -> bool {
    base.get(ptr).is_some_and(|b| volatile_allocas.contains(*b))
}

fn upgrade<'a>(
    line: &'a str,
    base: &HashMap<&str, &str>,
    volatile_allocas: &HashSet<&str>,
) -> Cow<'a, str> {
    let t = line.trim_start();
    let indent = &line[..line.len() - t.len()];

    if t.starts_with("store ") {
        if t.starts_with("store volatile ") {
            return Cow::Borrowed(line);
        }
        if let Some(p) = store_dest_ptr(t) {
            if is_volatile_target(p, base, volatile_allocas) {
                return Cow::Owned(format!("{}store volatile {}", indent, &t["store ".len()..]));
            }
        }
        return Cow::Borrowed(line);
    }

    if let Some(at) = t.find(" = load ") {
        let rest = &t[at + " = load ".len()..];
        if rest.starts_with("volatile ") {
            return Cow::Borrowed(line);
        }
        if let Some(p) = load_src_ptr(t) {
            if is_volatile_target(p, base, volatile_allocas) {
                // `!invariant.load` promises the memory never changes — the
                // exact opposite of what a try-mutated slot needs. Drop it.
                let rest = rest.split(", !invariant.load").next().unwrap_or(rest);
                return Cow::Owned(format!("{}{} = load volatile {}", indent, &t[..at], rest));
            }
        }
    }

    Cow::Borrowed(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores(regs: &[&str]) -> HashSet<String> {
        regs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_store_destination_not_the_stored_pointer() {
        assert_eq!(store_dest_ptr("  store double %r4, ptr %r3"), Some("%r3"));
        assert_eq!(
            store_dest_ptr("  store double %r4, ptr %r3, align 8"),
            Some("%r3")
        );
        // A pointer VALUE stored into a slot: the destination is the 2nd ptr.
        assert_eq!(store_dest_ptr("  store ptr %r7, ptr %r2"), Some("%r2"));
        assert_eq!(store_dest_ptr("  %r1 = load double, ptr %r2"), None);
    }

    #[test]
    fn upgrades_only_the_try_written_slot() {
        let ir = "define double @f() #1 {\n\
                  entry:\n\
                  \x20 %acc = alloca double\n\
                  \x20 %i = alloca double\n\
                  \x20 store double 0.0, ptr %acc\n\
                  \x20 store double 0.0, ptr %i\n\
                  \x20 %v = load double, ptr %i\n\
                  \x20 %a = load double, ptr %acc\n\
                  \x20 ret double %a\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &stores(&["%acc"]));
        assert!(out.contains("store volatile double 0.0, ptr %acc"));
        assert!(out.contains("%a = load volatile double, ptr %acc"));
        // The loop counter is untouched — that is the whole point of #6385.
        assert!(out.contains("  store double 0.0, ptr %i\n"));
        assert!(out.contains("  %v = load double, ptr %i\n"));
    }

    #[test]
    fn a_store_through_a_gep_marks_the_whole_alloca() {
        let ir = "define void @f() #1 {\n\
                  entry:\n\
                  \x20 %buf = alloca [4 x double]\n\
                  \x20 %p0 = getelementptr inbounds [4 x double], ptr %buf, i64 0, i64 0\n\
                  \x20 %p1 = getelementptr inbounds [4 x double], ptr %buf, i64 0, i64 1\n\
                  \x20 store double 1.0, ptr %p1\n\
                  \x20 %x = load double, ptr %p0\n\
                  \x20 ret void\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &stores(&["%p1"]));
        assert!(out.contains("store volatile double 1.0, ptr %p1"));
        // The sibling element of the same alloca is upgraded too: mem2reg/SROA
        // promotability is a property of the alloca, not of one slice.
        assert!(out.contains("%x = load volatile double, ptr %p0"));
    }

    #[test]
    fn globals_and_heap_pointers_are_not_allocas_and_stay_untouched() {
        let ir = "define void @f() #1 {\n\
                  entry:\n\
                  \x20 %s = alloca double\n\
                  \x20 store double 1.0, ptr @perry_global_m__3\n\
                  \x20 %g = load double, ptr @perry_global_m__3\n\
                  \x20 ret void\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &stores(&["@perry_global_m__3"]));
        assert!(!out.contains("volatile"));
    }

    #[test]
    fn already_volatile_accesses_are_left_alone() {
        let ir = "define void @f() #1 {\n\
                  entry:\n\
                  \x20 %s = alloca double\n\
                  \x20 store volatile double 1.0, ptr %s\n\
                  \x20 %v = load volatile double, ptr %s\n\
                  \x20 ret void\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &stores(&["%s"]));
        assert!(!out.contains("store volatile volatile"));
        assert!(!out.contains("load volatile volatile"));
    }

    #[test]
    fn invariant_load_metadata_is_dropped_on_upgrade() {
        let ir = "define void @f() #1 {\n\
                  entry:\n\
                  \x20 %s = alloca i64\n\
                  \x20 store i64 1, ptr %s\n\
                  \x20 %v = load i64, ptr %s, !invariant.load !0\n\
                  \x20 ret void\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &stores(&["%s"]));
        assert!(out.contains("%v = load volatile i64, ptr %s"));
        assert!(!out.contains("!invariant.load"));
    }

    #[test]
    fn no_try_stores_is_a_no_op() {
        let ir = "define void @f() {\n\
                  entry:\n\
                  \x20 %s = alloca double\n\
                  \x20 store double 1.0, ptr %s\n\
                  \x20 ret void\n\
                  }\n";
        let out = apply_setjmp_volatile(ir, &HashSet::new());
        assert_eq!(out, ir);
    }
}
