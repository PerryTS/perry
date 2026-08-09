//! Attributing emitted root-slot traffic to the slot it names (#7504).
//!
//! # Why counting the module stopped working
//!
//! `js_shadow_slot_bind` and `js_write_barrier_root_nanbox` used to have
//! exactly one producer worth counting: a named local's, or a scalar-replaced
//! field's, entry alloca. `scalar_replaced_slot_roots.rs` therefore measured
//! its subject with two whole-module counters — `ir.matches("call void
//! @js_shadow_slot_bind(").count()` and the same for the barrier.
//!
//! #7487 gave temporaries a claim on both. A pooled temp root reserves a frame
//! slot through `reserve_shadow_slot()` and emits `emit_shadow_slot_bind_ptr`
//! at every store — the identical call, on an alloca that belongs to no HIR
//! local. `console.log(p.x, p.y)`, the harmless tail of every fixture in that
//! file, contributes three binds naming the argument accumulator's slot. So
//! `bind_calls(&ir) == 0` became a claim about the accumulator and
//! `bind_calls(&ir) == 1` a coincidence.
//!
//! # What this module measures instead
//!
//! Every bind and every root-shading barrier, keyed by the **entry alloca** it
//! names, with that alloca classified:
//!
//! | kind | alloca | who reserves it |
//! |---|---|---|
//! | [`SlotKind::Value`] | `alloca double` | a named local, or a scalar-replaced field/element slot (#6968) |
//! | [`SlotKind::TempRoot`] | `alloca i64` null-initialised at entry | #7487's `TempRootPool` |
//!
//! [`bound_slots`] **panics on a bound alloca it cannot classify** rather than
//! defaulting it into either bucket. That is deliberate and is the property
//! that keeps this reader from decaying the way the counters did: when a third
//! slot family appears, the tests that use this go red naming it, instead of
//! silently folding it into whichever total happens to be asserted.
//!
//! Everything here reads the SHADOW-STACK spelling. The native-roots lowering
//! expresses the same root set as `ptr addrspace(1)` allocas with no bind call
//! to attribute, which is why the suites that use this pin
//! `NativeRootsPin::shadow()`; `crate::native_root_coverage` is the native
//! side's equivalent.

use std::collections::BTreeMap;

/// What a bound entry alloca belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotKind {
    /// A named local, or a scalar-replaced object field / array element slot.
    /// These hold NaN-boxed `double`s.
    Value,
    /// One of #7487's pooled temp-root slots: `alloca i64`, null-initialised in
    /// the entry block, holding an expression temporary rather than any HIR
    /// local.
    TempRoot,
}

/// `%reg` -> the text right of `=` on its defining line, within one function.
fn defs(fn_ir: &str) -> BTreeMap<&str, &str> {
    fn_ir
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('%'))
        .filter_map(|line| line.split_once(" = "))
        .map(|(reg, def)| (reg.trim(), def.trim()))
        .collect()
}

/// The `ptr` operand of a `js_shadow_slot_bind` call line, if this is one.
fn bind_slot(line: &str) -> Option<&str> {
    let rest = line
        .trim()
        .strip_prefix("call void @js_shadow_slot_bind(i32 ")?;
    let (_, slot) = rest.split_once(", ptr ")?;
    let slot = slot.trim_end_matches(')').trim();
    slot.starts_with('%').then_some(slot)
}

fn classify(fn_ir: &str, defs: &BTreeMap<&str, &str>, slot: &str) -> SlotKind {
    match defs.get(slot) {
        Some(&"alloca double") => SlotKind::Value,
        Some(&"alloca i64") if fn_ir.contains(&format!("store i64 0, ptr {slot}\n")) => {
            SlotKind::TempRoot
        }
        other => panic!(
            "root slot {slot} is bound but its alloca ({other:?}) belongs to no \
             known slot family. Adding one is fine — classify it HERE, in \
             `testing::root_slots`, so every test that measures root traffic \
             sees it. Silently folding it into an existing total is how a \
             whole-module bind count stopped measuring its subject (#7504)."
        ),
    }
}

/// Every emitted `js_shadow_slot_bind`, keyed by the alloca it names.
///
/// The `declare` line is unconditional, so only emitted CALLs are counted.
pub fn bound_slots(fn_ir: &str) -> BTreeMap<String, (SlotKind, usize)> {
    let defs = defs(fn_ir);
    let mut out: BTreeMap<String, (SlotKind, usize)> = BTreeMap::new();
    for line in fn_ir.lines() {
        let Some(slot) = bind_slot(line) else {
            continue;
        };
        let kind = classify(fn_ir, &defs, slot);
        out.entry(slot.to_string()).or_insert((kind, 0)).1 += 1;
    }
    out
}

/// Binds naming a [`SlotKind::Value`] slot — the scalar-replaced fields and
/// named locals the #6968 / #6997 / #7013 contract is about.
pub fn value_slot_binds(fn_ir: &str) -> usize {
    bound_slots(fn_ir)
        .values()
        .filter(|(kind, _)| *kind == SlotKind::Value)
        .map(|(_, count)| *count)
        .sum()
}

/// Binds naming one of #7487's pooled temp-root slots.
///
/// Not noise to be subtracted — a number to be *stated*. A test that says
/// "zero value-slot binds" is only interesting if this is non-zero, because
/// that is what proves the reader was looking at a module where binds exist.
pub fn temp_root_slot_binds(fn_ir: &str) -> usize {
    bound_slots(fn_ir)
        .values()
        .filter(|(kind, _)| *kind == SlotKind::TempRoot)
        .map(|(_, count)| *count)
        .sum()
}

/// Root-shading barriers (`js_write_barrier_root_nanbox`) keyed by the slot the
/// shaded value was loaded out of.
///
/// The barrier takes the value, not the slot, so the attribution goes through
/// its operand's definition: `emit_shadow_slot_bind_ptr` shades what it just
/// re-loaded, so the operand is always a `load i64, ptr %slot` in the same
/// function. An operand that is not such a load is counted under the key
/// `"<unattributed>"` rather than dropped — a barrier nobody can attribute is a
/// finding, not a rounding error.
pub fn barriers_by_slot(fn_ir: &str) -> BTreeMap<String, usize> {
    let defs = defs(fn_ir);
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for line in fn_ir.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("call void @js_write_barrier_root_nanbox(i64 ") else {
            continue;
        };
        let value = rest
            .split(')')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let slot = defs
            .get(value)
            .and_then(|def| def.strip_prefix("load i64, ptr "))
            .map(|slot| slot.split(',').next().unwrap_or(slot).trim().to_string())
            .unwrap_or_else(|| "<unattributed>".to_string());
        *out.entry(slot).or_default() += 1;
    }
    out
}

/// Root-shading barriers whose value came out of a [`SlotKind::Value`] slot.
pub fn value_slot_barriers(fn_ir: &str) -> usize {
    let defs = defs(fn_ir);
    barriers_by_slot(fn_ir)
        .into_iter()
        .filter(|(slot, _)| matches!(defs.get(slot.as_str()), Some(&"alloca double")))
        .map(|(_, count)| count)
        .sum()
}

/// The slot count baked into this function's `js_shadow_frame_enter`.
pub fn frame_slot_count(fn_ir: &str) -> u32 {
    let needle = "call ptr @js_shadow_frame_enter(i32 ";
    let start = fn_ir
        .find(needle)
        .map(|i| i + needle.len())
        .unwrap_or_else(|| panic!("expected a shadow frame enter in:\n{fn_ir}"));
    let rest = &fn_ir[start..];
    let end = rest.find(')').expect("malformed frame enter");
    rest[..end].parse().expect("frame enter count is not a number")
}

/// The whole `define … { … }` body of `@name`.
///
/// Anchored on the `define` LINE — `ir.find("define i32 @main(")` would also
/// match a mention inside another function's body, and #7669 is what a slice
/// helper that cuts from the wrong anchor costs: every negative assertion
/// against the slice silently loses its subject.
pub fn function_slice<'a>(ir: &'a str, name: &str) -> &'a str {
    let marker = format!("@{name}(");
    let start = ir
        .match_indices("define ")
        .filter(|(idx, _)| *idx == 0 || ir.as_bytes()[idx - 1] == b'\n')
        .find(|(idx, _)| {
            let line_end = ir[*idx..].find('\n').map(|o| idx + o).unwrap_or(ir.len());
            ir[*idx..line_end].contains(&marker)
        })
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| panic!("no function `{name}` in IR:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|o| start + o + 2)
        .unwrap_or(ir.len());
    &ir[start..end]
}

/// The whole `define … { … }` body of the function containing `needle`.
pub fn enclosing_function<'a>(ir: &'a str, needle: &str) -> &'a str {
    let at = ir
        .find(needle)
        .unwrap_or_else(|| panic!("no `{needle}` in:\n{ir}"));
    let start = ir[..at]
        .rfind("\ndefine ")
        .map(|i| i + 1)
        .unwrap_or_else(|| panic!("`{needle}` is outside any function in:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|i| start + i + 2)
        .unwrap_or(ir.len());
    &ir[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `main` carrying both slot families at once: `%v` is a scalar-replaced
    /// field slot, `%t` is a pooled temp root. This is the exact shape #7504
    /// describes — three binds naming the `console.log` accumulator, one naming
    /// the field the test means to measure.
    const MIXED: &str = "\
define i32 @main() {
entry.0:
  %v = alloca double
  store double 0x7FFC000000000001, ptr %v
  %t = alloca i64
  store i64 0, ptr %t
  %r3 = call ptr @js_shadow_frame_enter(i32 2)
  call void @js_shadow_slot_bind(i32 1, ptr %v)
  %r13 = load i64, ptr %v
  call void @js_write_barrier_root_nanbox(i64 %r13)
  call void @js_shadow_slot_bind(i32 2, ptr %t)
  %r25 = load i64, ptr %t
  call void @js_write_barrier_root_nanbox(i64 %r25)
  call void @js_shadow_slot_bind(i32 2, ptr %t)
  ret i32 0
}
";

    #[test]
    fn binds_are_attributed_to_the_slot_that_owns_them() {
        assert_eq!(value_slot_binds(MIXED), 1);
        assert_eq!(temp_root_slot_binds(MIXED), 2);
        // …and the whole-module count this replaces cannot tell them apart,
        // which is the defect: 3 is what `bind_calls` reported for a program
        // whose subject binds once.
        assert_eq!(MIXED.matches("call void @js_shadow_slot_bind(").count(), 3);
    }

    #[test]
    fn barriers_are_attributed_through_the_value_they_shade() {
        assert_eq!(value_slot_barriers(MIXED), 1);
        assert_eq!(
            barriers_by_slot(MIXED).get("%t").copied().unwrap_or(0),
            1,
            "the temp root's own barrier must be visible, not merged away"
        );
        assert!(
            !barriers_by_slot(MIXED).contains_key("<unattributed>"),
            "every barrier in this fixture is attributable"
        );
    }

    /// The property that keeps this reader from rotting the way the counters
    /// did: a bound slot from an unknown family is a hard failure.
    #[test]
    fn a_bound_slot_from_an_unknown_family_is_refused_not_bucketed() {
        let unknown = "\
define i32 @main() {
entry.0:
  %x = alloca i32
  call void @js_shadow_slot_bind(i32 0, ptr %x)
  ret i32 0
}
";
        let refused = std::panic::catch_unwind(|| value_slot_binds(unknown));
        assert!(
            refused.is_err(),
            "an unclassifiable bound slot must fail loudly — folding it into a \
             total silently is #7504"
        );
    }

    /// An `alloca i64` that is NOT null-initialised at entry is not a temp-root
    /// slot; the frame base-index alloca is one such, and mistaking it for a
    /// pool slot would let a real regression hide in the temp bucket.
    #[test]
    fn an_uninitialised_i64_alloca_is_not_mistaken_for_a_temp_root() {
        let frame_base = "\
define i32 @main() {
entry.0:
  %b = alloca i64
  store i64 7, ptr %b
  call void @js_shadow_slot_bind(i32 0, ptr %b)
  ret i32 0
}
";
        assert!(std::panic::catch_unwind(|| bound_slots(frame_base)).is_err());
    }
}
