//! Reading the temp-root emission contract out of IR, in either lowering
//! (#7503).
//!
//! # The contract, and why its old spelling is gone
//!
//! #6951's rule is that an evaluated-but-not-yet-consumed temporary must be a
//! precise GC root, not a bare SSA register: pushed into a rooted slot before
//! anything that can collect, **re-read** from that slot afterwards (the slot
//! is mutable, so an evacuating cycle rewrites it and the pushed register is
//! stale — #7114), written back if the consuming call reallocates, and released
//! after the last use.
//!
//! Until #7487 that was spelled with three runtime calls, and the suites
//! asserted the spelling: `ir.contains("call i32 @js_gc_temp_root_push")`.
//! #7487 re-lowered temps onto pooled frame allocas — `push` became a store,
//! `get` a load, `truncate` a slot clear — and those calls now survive only on
//! an FFI fallback arm that neither shipped lowering takes. So ten assertions
//! failed, and the eight `!ir.contains(…push)` NEGATIVES held for every program
//! in the language, rooted or not. The contract never changed; nothing was
//! measuring it in either direction.
//!
//! # What this reader does instead
//!
//! It names the *value*. [`slot_traffic`] returns, per entry alloca, the
//! registers stored into it, the registers loaded out of it and where it was
//! cleared — in program order — recognising **both** spellings:
//!
//! | event | shadow stack | native roots (RS4GC) |
//! |---|---|---|
//! | store | `store i64 %v, ptr %s` | `%t = inttoptr i64 %v to ptr addrspace(1)` + `store ptr addrspace(1) %t, ptr %s` |
//! | load | `%d = load i64, ptr %s` | `%d.rs4p = load ptr addrspace(1), ptr %s` (+ `ptrtoint`/`bitcast` back to `%d`) |
//! | clear | `store i64 0, ptr %s` | `store ptr addrspace(1) null, ptr %s` |
//!
//! The RS4GC retype preserves register NAMES — the value the shadow lowering
//! would have called `%r7` is still `%r7`, reached through `%r7.rs4p` — which
//! is what makes one reader serve both. That in turn is what lets the contract
//! be asserted UNPINNED: it is lowering-independent, and should be stated that
//! way.
//!
//! `ir.contains("call i32 @js_gc_temp_root_push")` only ever proved that a call
//! existed somewhere in the module. [`assert_rooted_across`] proves that THIS
//! value was in a slot, and that THIS call read it back out.

use std::collections::BTreeMap;

/// One event on one slot, tagged with its line index so order is comparable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotEvent {
    /// A value register was stored into the slot.
    Store { value: String, line: usize },
    /// A register was loaded out of the slot.
    Load { into: String, line: usize },
    /// The slot was cleared — #7487's spelling of `js_gc_temp_root_truncate`.
    Clear { line: usize },
}

impl SlotEvent {
    pub fn line(&self) -> usize {
        match self {
            SlotEvent::Store { line, .. }
            | SlotEvent::Load { line, .. }
            | SlotEvent::Clear { line } => *line,
        }
    }
}

/// `%reg` -> the text right of `=` on its defining line.
fn defs(fn_ir: &str) -> BTreeMap<&str, &str> {
    fn_ir
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('%'))
        .filter_map(|line| line.split_once(" = "))
        .map(|(reg, def)| (reg.trim(), def.trim()))
        .collect()
}

fn operand_before(text: &str, sep: &str) -> Option<String> {
    let (value, _) = text.split_once(sep)?;
    let value = value.trim();
    value.starts_with('%').then(|| value.to_string())
}

/// Every rooted-slot event in `fn_ir`, keyed by the alloca.
///
/// Slots with no traffic at all are absent; a slot that is only ever
/// null-initialised at entry is not a root anybody used.
pub fn slot_traffic(fn_ir: &str) -> BTreeMap<String, Vec<SlotEvent>> {
    let defs = defs(fn_ir);
    let mut out: BTreeMap<String, Vec<SlotEvent>> = BTreeMap::new();
    let mut entry_seeds: BTreeMap<String, usize> = BTreeMap::new();

    for (line_no, line) in fn_ir.lines().map(str::trim).enumerate() {
        // ---- stores and clears -------------------------------------------
        if let Some(rest) = line.strip_prefix("store i64 ") {
            if let Some((value, slot)) = rest.split_once(", ptr ") {
                let slot = slot.split(',').next().unwrap_or(slot).trim().to_string();
                if slot.starts_with('%') {
                    if value.trim() == "0" {
                        // The entry seed and a release are the same instruction;
                        // only the first one in a slot's life is a seed.
                        if entry_seeds.contains_key(&slot) || out.contains_key(&slot) {
                            out.entry(slot)
                                .or_default()
                                .push(SlotEvent::Clear { line: line_no });
                        } else {
                            entry_seeds.insert(slot, line_no);
                        }
                    } else if value.trim().starts_with('%') {
                        out.entry(slot).or_default().push(SlotEvent::Store {
                            value: value.trim().to_string(),
                            line: line_no,
                        });
                    }
                }
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("store ptr addrspace(1) ") {
            if let Some((value, slot)) = rest.split_once(", ptr ") {
                let slot = slot.split(',').next().unwrap_or(slot).trim().to_string();
                if !slot.starts_with('%') {
                    continue;
                }
                let value = value.trim();
                if value == "null" {
                    if entry_seeds.contains_key(&slot) || out.contains_key(&slot) {
                        out.entry(slot)
                            .or_default()
                            .push(SlotEvent::Clear { line: line_no });
                    } else {
                        entry_seeds.insert(slot, line_no);
                    }
                } else if let Some(source) = defs
                    .get(value)
                    .and_then(|def| def.strip_prefix("inttoptr i64 "))
                    .and_then(|rest| operand_before(rest, " to ptr addrspace(1)"))
                {
                    // The retype interposed an `inttoptr`; the VALUE is what
                    // fed it, which is the same register the shadow lowering
                    // would have stored directly.
                    out.entry(slot).or_default().push(SlotEvent::Store {
                        value: source,
                        line: line_no,
                    });
                }
                continue;
            }
        }

        if let Some(rest) = line.strip_prefix("store double ") {
            if let Some((value, slot)) = rest.split_once(", ptr ") {
                let slot = slot.split(',').next().unwrap_or(slot).trim().to_string();
                if slot.starts_with('%') {
                    out.entry(slot).or_default().push(SlotEvent::Store {
                        value: value.trim().to_string(),
                        line: line_no,
                    });
                }
                continue;
            }
        }

        // ---- loads --------------------------------------------------------
        let Some((dst, def)) = line.split_once(" = ") else {
            continue;
        };
        let dst = dst.trim();
        let def = def.trim();
        for prefix in ["load i64, ptr ", "load double, ptr "] {
            if let Some(rest) = def.strip_prefix(prefix) {
                let slot = rest.split(',').next().unwrap_or(rest).trim().to_string();
                if slot.starts_with('%') {
                    out.entry(slot).or_default().push(SlotEvent::Load {
                        into: dst.to_string(),
                        line: line_no,
                    });
                }
            }
        }
        if let Some(rest) = def.strip_prefix("load ptr addrspace(1), ptr ") {
            let slot = rest.split(',').next().unwrap_or(rest).trim().to_string();
            // RS4GC renames the load `%r7.rs4p` and rebuilds `%r7` from it, so
            // the register the rest of the function uses is the stem.
            let into = dst.strip_suffix(".rs4p").unwrap_or(dst).to_string();
            if slot.starts_with('%') {
                out.entry(slot).or_default().push(SlotEvent::Load {
                    into,
                    line: line_no,
                });
            }
        }
    }

    out
}

/// Does `reg` derive from `ancestor` within `depth` def-use steps?
///
/// A raw allocation result is NaN-boxed (`or` + `bitcast`) before it reaches a
/// slot, so "the value that was rooted" and "the register the allocator
/// returned" are never the same SSA name. Following the boxing is what makes an
/// assertion about `js_object_alloc`'s result mean anything.
fn derives_from(defs: &BTreeMap<&str, &str>, reg: &str, ancestor: &str, depth: usize) -> bool {
    let mut reg = reg.to_string();
    for _ in 0..depth {
        if reg == ancestor {
            return true;
        }
        let Some(def) = defs.get(reg.as_str()) else {
            return false;
        };
        let Some(next) = def
            .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .find(|w| w.starts_with('%'))
        else {
            return false;
        };
        reg = next.to_string();
    }
    reg == ancestor
}

/// The slot `value` — or the NaN-boxed form of it — was stored into, if any.
pub fn slot_holding(fn_ir: &str, value: &str) -> Option<String> {
    let defs = defs(fn_ir);
    slot_traffic(fn_ir).into_iter().find_map(|(slot, events)| {
        events
            .iter()
            .any(|e| match e {
                SlotEvent::Store { value: v, .. } => derives_from(&defs, v, value, 4),
                _ => false,
            })
            .then_some(slot)
    })
}

/// The `%reg` a `call`-defining line assigns, for the first call to `callee`.
pub fn first_call_result(fn_ir: &str, callee: &str) -> Option<String> {
    let needle = format!("@{callee}(");
    fn_ir
        .lines()
        .map(str::trim)
        .filter_map(|line| line.split_once(" = "))
        .find(|(_, def)| def.starts_with("call ") && def.contains(&needle))
        .map(|(dst, _)| dst.trim().to_string())
}

/// The operands of the first call to `callee`, as printed registers/literals.
pub fn call_operands(fn_ir: &str, callee: &str) -> Option<Vec<String>> {
    let needle = format!("@{callee}(");
    let line = fn_ir
        .lines()
        .map(str::trim)
        .find(|line| line.contains("call ") && line.contains(&needle))?;
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find(')')?;
    Some(
        rest[..end]
            .split(',')
            .filter_map(|operand| operand.trim().rsplit_once(' ').map(|(_, r)| r.to_string()))
            .collect(),
    )
}

/// **The #6951 assertion.** `producer_result` must be parked in a rooted slot,
/// and `consumer`'s operand must be RE-READ out of that slot rather than being
/// the producer's own register.
///
/// This is what `ir.contains("call i32 @js_gc_temp_root_push")` was standing in
/// for, and it is strictly stronger: that only proved a call existed somewhere
/// in the module, and said nothing about which value it protected or whether
/// anybody read it back.
pub fn assert_rooted_across(fn_ir: &str, producer_result: &str, consumer: &str, what: &str) {
    let traffic = slot_traffic(fn_ir);
    let slot = slot_holding(fn_ir, producer_result).unwrap_or_else(|| {
        panic!(
            "{what}: {producer_result} is never stored into a rooted slot — it \
             lives its whole life in an SSA register, which is not a GC root \
             (#6951). Slot traffic: {traffic:#?}\n{fn_ir}"
        )
    });
    let defs = defs(fn_ir);
    let events = &traffic[&slot];
    let store_line = events
        .iter()
        .find_map(|e| match e {
            SlotEvent::Store { value, line } if derives_from(&defs, value, producer_result, 4) => {
                Some(*line)
            }
            _ => None,
        })
        .expect("slot_holding just found this store");

    let operands = call_operands(fn_ir, consumer).unwrap_or_else(|| {
        panic!("{what}: no call to @{consumer} — this assertion has no subject:\n{fn_ir}")
    });
    let consumer_line = fn_ir
        .lines()
        .position(|line| line.contains(&format!("@{consumer}(")))
        .expect("call_operands just found it");

    let reread = events.iter().any(|e| match e {
        SlotEvent::Load { into, line } => {
            *line > store_line
                && *line < consumer_line
                && operands
                    .iter()
                    .any(|operand| derives_from(&defs, operand, into, 4))
        }
        _ => false,
    });
    assert!(
        reread,
        "{what}: @{consumer} takes {operands:?}, none of which was re-read from \
         {slot} between the store at line {store_line} and the call at line \
         {consumer_line}. A root buys a rewritten LOCATION; the consuming call \
         only observes the rewrite if it reads that location again (#7114). \
         Slot traffic: {events:#?}\n{fn_ir}"
    );
    assert!(
        !operands.contains(&producer_result.to_string()),
        "{what}: @{consumer} still takes the producer's own register \
         {producer_result} — that register is stale the moment anything between \
         them collects (#6951):\n{fn_ir}"
    );
}

/// Does `reg` derive, within `depth` def-use steps, from a value loaded out of
/// a rooted slot?
///
/// The consuming call rarely takes the loaded register itself — the pooled
/// lowering loads `i64` and the caller wants a NaN-boxed `double`, so a
/// `bitcast`/`ptrtoint`/`or` sits in between. This is the pooled-form
/// replacement for the hand-rolled "walk back until you hit
/// `@js_gc_temp_root_get`" loops the suites grew.
pub fn derives_from_slot_load(fn_ir: &str, reg: &str, depth: usize) -> bool {
    let defs = defs(fn_ir);
    let loaded: std::collections::BTreeSet<String> = slot_traffic(fn_ir)
        .values()
        .flat_map(|events| events.iter())
        .filter_map(|e| match e {
            SlotEvent::Load { into, .. } => Some(into.clone()),
            _ => None,
        })
        .collect();
    let mut reg = reg.to_string();
    for _ in 0..depth {
        if loaded.contains(&reg) {
            return true;
        }
        let Some(def) = defs.get(reg.as_str()) else {
            return false;
        };
        let Some(next) = def
            .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .find(|w| w.starts_with('%'))
        else {
            return false;
        };
        reg = next.to_string();
    }
    false
}

/// Slots whose FIRST traffic is a zero/null seed — `TempRootPool`'s
/// `entry_allocas_push_store(I64, "0", …)`, which the RS4GC retype re-emits as
/// a null `addrspace(1)` store.
///
/// Required as well as the alloca type because an `alloca i64` is also how
/// codegen spells unrelated scratch cells. The per-class inline-keys cache is
/// now a precise function-lifetime root (#7876), so it has the same seed and
/// alloca type as a temp root; [`temp_root_slots`] excludes that one by the
/// provenance of the value stored into it.
pub fn zero_seeded_slots(fn_ir: &str) -> std::collections::BTreeSet<String> {
    let mut touched: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut seeded: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in fn_ir.lines().map(str::trim) {
        let seed = line
            .strip_prefix("store i64 0, ptr ")
            .or_else(|| line.strip_prefix("store ptr addrspace(1) null, ptr "));
        if let Some(rest) = seed {
            let slot = rest.split(',').next().unwrap_or(rest).trim().to_string();
            if slot.starts_with('%') && !touched.contains(&slot) {
                seeded.insert(slot.clone());
            }
            touched.insert(slot);
            continue;
        }
        // Every other mention of the slot — a store of a real value, a load out
        // of it, an operand — marks it touched. `, ptr %s` covers all of them,
        // loads included (`%d = load i64, ptr %s`), so there is deliberately no
        // second load-specific pass: an extra branch that re-inserts what this
        // one already caught reads as coverage it does not add (#7675 review).
        for slot in line
            .split(", ptr ")
            .skip(1)
            .filter_map(|rest| rest.split(',').next())
            .map(|slot| slot.trim_end_matches(')').trim().to_string())
            .filter(|slot| slot.starts_with('%'))
        {
            touched.insert(slot);
        }
    }
    seeded
}

/// The `undefined` seed every NAMED-local and scalar-replacement slot gets at
/// entry, as codegen prints it.
fn undefined_literal() -> String {
    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
}

/// The slots that belong to #7487's pooled temp roots rather than to a named
/// local, a scalar-replaced field or an array-literal element.
///
/// Two filters, because neither is sufficient on its own:
///
/// 1. **The alloca's type.** `TempRootPool` allocates `alloca_entry(I64)`, so a
///    pooled slot is `alloca i64` — or `alloca ptr addrspace(1)` once the
///    native retype has run. Value storage (a named local, a scalar-replaced
///    field, an array-literal element) is `alloca double`, which excludes it.
/// 2. **The `undefined` seed.** Filter 1 alone is not enough under native
///    roots: RS4GC retypes every pointer-capable alloca to
///    `ptr addrspace(1)`, so a POINTER local's slot is spelled the same as a
///    temp slot. What still separates them is that a named local's slot is
///    seeded with `undefined` — `root_entry_alloca`'s contract, since the
///    collector dereferences it from function entry and must find something
///    decodable — while a pooled temp slot is seeded with zero/null.
pub fn temp_root_slots(fn_ir: &str) -> Vec<String> {
    let defs = defs(fn_ir);
    let undefined = undefined_literal();
    let seeded = zero_seeded_slots(fn_ir);
    let class_key_loads: Vec<&str> = defs
        .iter()
        .filter_map(|(&reg, def)| {
            def.starts_with("load i64, ptr @perry_class_keys_")
                .then_some(reg)
        })
        .collect();
    slot_traffic(fn_ir)
        .into_iter()
        .filter(|(slot, _)| seeded.contains(slot))
        .filter(|(slot, _)| {
            // Match the alloca's TYPE, not the whole def text: one `align 8`
            // suffix away, an exact comparison matches nothing, this filter
            // empties the result and every `assert_no_temp_rooting` in the tree
            // goes vacuous — the exact failure #7503 exists to remove. Raised by
            // review on #7675.
            matches!(
                defs.get(slot.as_str())
                    .copied()
                    .and_then(super::root_slots::alloca_type),
                Some("i64") | Some("ptr addrspace(1)")
            )
        })
        .filter(|(_, events)| events.iter().any(|e| matches!(e, SlotEvent::Store { .. })))
        .filter(|(_, events)| {
            !events.iter().any(|e| match e {
                SlotEvent::Store { value, .. } => {
                    value == &undefined
                        || defs.get(value.as_str()).is_some_and(|def| {
                            def.starts_with("bitcast double ") && def.contains(&undefined)
                        })
                }
                _ => false,
            })
        })
        .filter(|(_, events)| {
            // #7876: the registered class-keys global owns liveness, while a
            // function-local immutable copy is a precise root solely so an
            // old-page move can rewrite it. It lasts for the whole function;
            // it is not one of #7487's scoped expression temporaries. Match
            // the exact registered-global provenance so an ordinary temp that
            // happens to lack its closing clear remains visible to this gate.
            !events.iter().any(|event| match event {
                SlotEvent::Store { value, .. } => class_key_loads
                    .iter()
                    .any(|load| derives_from(&defs, value, load, 4)),
                _ => false,
            })
        })
        .map(|(slot, _)| slot)
        .collect()
}

/// No expression temporary was rooted in `fn_ir`: the #6996 / #6997 direction,
/// where rooting a value that can never be collected is pure cost.
///
/// Named locals' own slots are excluded by [`temp_root_slots`] — this is a
/// claim about TEMPORARIES, and a fixture that declares a `Buffer` local roots
/// that local either way.
pub fn assert_no_temp_rooting(fn_ir: &str, what: &str) {
    let rooted = temp_root_slots(fn_ir);
    assert!(
        rooted.is_empty(),
        "{what}: a temp root was emitted where none is needed — a value that \
         cannot be a heap reference costs a store, a re-read and a clear for \
         nothing (#6996/#6997). Pooled temp slots with traffic: {rooted:?}. \
         Full traffic: {:#?}\n{fn_ir}",
        slot_traffic(fn_ir)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pooled shape, shadow spelling — `console.log("label", {})`.
    const SHADOW: &str = "\
define i32 @main() {
entry.0:
  %s = alloca i64
  store i64 0, ptr %s
  %r1 = call i64 @js_array_alloc(i32 2)
  store i64 %r1, ptr %s
  %r31 = load i64, ptr %s
  %r32 = call i64 @js_array_push_f64(i64 %r31, double 1.0)
  store i64 %r32, ptr %s
  %r40 = load i64, ptr %s
  call void @js_console_log_spread(i64 %r40)
  store i64 0, ptr %s
  ret i32 0
}
";

    /// The same program, native-roots spelling. Register NAMES are preserved by
    /// the RS4GC retype, which is what lets one reader serve both.
    const NATIVE: &str = "\
define i32 @main() gc \"statepoint-example\" {
entry.0:
  %s = alloca ptr addrspace(1)
  store ptr addrspace(1) null, ptr %s
  %r1 = call i64 @js_array_alloc(i32 2)
  %rs4gc.s1 = inttoptr i64 %r1 to ptr addrspace(1)
  store ptr addrspace(1) %rs4gc.s1, ptr %s
  %r31.rs4p = load ptr addrspace(1), ptr %s
  %r31 = ptrtoint ptr addrspace(1) %r31.rs4p to i64
  %r32 = call i64 @js_array_push_f64(i64 %r31, double 1.0)
  %rs4gc.s2 = inttoptr i64 %r32 to ptr addrspace(1)
  store ptr addrspace(1) %rs4gc.s2, ptr %s
  %r40.rs4p = load ptr addrspace(1), ptr %s
  %r40 = ptrtoint ptr addrspace(1) %r40.rs4p to i64
  call void @js_console_log_spread(i64 %r40)
  store ptr addrspace(1) null, ptr %s
  ret i32 0
}
";

    #[test]
    fn both_lowerings_read_as_the_same_contract() {
        for (name, ir) in [("shadow", SHADOW), ("native", NATIVE)] {
            assert_rooted_across(ir, "%r1", "js_array_push_f64", name);
            assert_rooted_across(ir, "%r32", "js_console_log_spread", name);
            let slot = slot_holding(ir, "%r1").unwrap_or_else(|| panic!("{name}: no slot"));
            assert_eq!(slot, "%s", "{name}");
            assert!(
                slot_traffic(ir)[&slot]
                    .iter()
                    .any(|e| matches!(e, SlotEvent::Clear { .. })),
                "{name}: the slot must be released after the consuming call"
            );
        }
    }

    /// The pre-#6951 shape: the accumulator threaded through an SSA register.
    /// This is the sabotage the positive assertion has to catch, and the
    /// spelling-based `ir.contains("…temp_root_push")` catches it only by
    /// accident — it would also "catch" a module that rooted something else.
    #[test]
    fn an_unrooted_accumulator_is_caught() {
        let unrooted = "\
define i32 @main() {
entry.0:
  %r1 = call i64 @js_array_alloc(i32 2)
  %r32 = call i64 @js_array_push_f64(i64 %r1, double 1.0)
  call void @js_console_log_spread(i64 %r32)
  ret i32 0
}
";
        assert!(
            std::panic::catch_unwind(|| assert_rooted_across(
                unrooted,
                "%r1",
                "js_array_push_f64",
                "sabotage"
            ))
            .is_err(),
            "an accumulator that never reaches a slot must fail the assertion"
        );
        assert_no_temp_rooting(unrooted, "sabotage control");
    }

    /// Rooted but not RE-READ — #7114 exactly. The value has liveness and a
    /// rewritten location, and the consuming call still observes the stale
    /// address. The old `contains("…temp_root_push")` passes this happily.
    #[test]
    fn rooted_but_not_re_read_is_caught() {
        let stale = "\
define i32 @main() {
entry.0:
  %s = alloca i64
  store i64 0, ptr %s
  %r1 = call i64 @js_array_alloc(i32 2)
  store i64 %r1, ptr %s
  %r32 = call i64 @js_array_push_f64(i64 %r1, double 1.0)
  ret i32 0
}
";
        assert!(
            std::panic::catch_unwind(|| assert_rooted_across(
                stale,
                "%r1",
                "js_array_push_f64",
                "sabotage"
            ))
            .is_err(),
            "a consuming call that reuses the pushed register must fail (#7114)"
        );
    }

    /// An `align` suffix on the alloca must not empty the slot set.
    ///
    /// This is the shape the exact `Some(&"alloca i64")` compare could not see,
    /// and its failure mode was the silent one: `temp_root_slots` returns
    /// nothing, so every `assert_no_temp_rooting` in the tree passes for a
    /// program that roots. Sabotage: restore the exact compare and this test
    /// fails while none of the positives do.
    #[test]
    fn an_aligned_alloca_is_still_recognised_as_a_temp_slot() {
        let aligned = SHADOW.replace("%s = alloca i64", "%s = alloca i64, align 8");
        assert_ne!(aligned, SHADOW, "the substitution must actually apply");
        assert_eq!(
            temp_root_slots(&aligned),
            vec!["%s".to_string()],
            "an `align` suffix must not hide the slot — an emptied slot set \
             makes every negative gate vacuous (#7675 review)"
        );
        assert!(
            std::panic::catch_unwind(move || assert_no_temp_rooting(&aligned, "sabotage")).is_err()
        );
    }

    #[test]
    fn the_negative_direction_sees_real_rooting() {
        assert!(
            std::panic::catch_unwind(|| assert_no_temp_rooting(SHADOW, "sabotage")).is_err(),
            "a program that DOES root must fail `assert_no_temp_rooting` — \
             otherwise the negative gates are claims about nothing"
        );
        assert!(
            std::panic::catch_unwind(|| assert_no_temp_rooting(NATIVE, "sabotage")).is_err(),
            "…in both lowerings"
        );
    }

    #[test]
    fn a_function_lifetime_class_keys_root_is_not_a_temp_root() {
        let shadow = "\
define i32 @main() {
entry.0:
  %keys = alloca i64
  store i64 0, ptr %keys
  %r1 = load i64, ptr @perry_class_keys_fixture
  store i64 %r1, ptr %keys
  %r2 = load i64, ptr %keys
  call void @consume(i64 %r2)
  ret i32 0
}
";
        let native = "\
define i32 @main() gc \"statepoint-example\" {
entry.0:
  %keys = alloca ptr addrspace(1)
  store ptr addrspace(1) null, ptr %keys
  %r1 = load i64, ptr @perry_class_keys_fixture
  %r1.rs4p = inttoptr i64 %r1 to ptr addrspace(1)
  store ptr addrspace(1) %r1.rs4p, ptr %keys
  %r2.rs4p = load ptr addrspace(1), ptr %keys
  %r2 = ptrtoint ptr addrspace(1) %r2.rs4p to i64
  call void @consume(i64 %r2)
  ret i32 0
}
";
        assert_no_temp_rooting(shadow, "class-key cache shadow root");
        assert_no_temp_rooting(native, "class-key cache native root");

        let unrelated = shadow.replace("@perry_class_keys_fixture", "@some_other_global");
        assert_eq!(
            temp_root_slots(&unrelated),
            vec!["%keys".to_string()],
            "only the registered class-key provenance is exempt; a missing temp-root clear must stay visible"
        );
    }
}
