//! Perry-GC call effects for native-stack safepoint lowering.
//!
//! This is deliberately narrower than LLVM's memory-effect attributes. A
//! helper may mutate runtime metadata, take a lock, or allocate through the
//! system allocator and still be safe to omit as a Perry GC safepoint. The
//! only question answered here is: can this call enter Perry's collector?
//!
//! # Source of truth: the generated table (RFC deferred collection, step S1)
//!
//! The answer for a runtime helper is no longer a hand-kept allowlist. It is
//! read from `gc_effects/<target>.tsv`, generated from the linked runtime and
//! stdlib archives by `scripts/gc_call_effects/callgraph.py` and checked in CI
//! against freshly built archives for Linux x86-64, macOS aarch64 and Windows
//! x86-64 (`cargo xwin`). The generator builds a symbol-level call graph from
//! the object code and classifies every exported symbol by what it can reach
//! (seeds, cuts and every audited exemption, each with its reason, are in
//! `scripts/gc_call_effects/seeds.txt`):
//!
//! * `Leaf`      reaches no collector entry, poll, JS entry, unaudited
//!               indirect call or unknown external symbol;
//! * `AllocOnly` reaches a collector only through a collector entry point
//!               (the allocation trigger), never JS or a poll;
//! * `ThrowOnly` additionally reaches JS only through the throw funnel;
//! * `Reenters`  everything else.
//!
//! Codegen uses the MOST conservative class across the three target tables,
//! and a symbol missing from any table is `Reenters`. Under today's runtime
//! (an allocation can still start a collection) only `Leaf` may be marked
//! `"gc-leaf-function"`. `AllocOnly` keeps its previous meaning,
//! [`GcCallEffect::AllocNoReentry`]: a leaf only under the research-only
//! `PERRY_GC_SAFEPOINT_ONLY` contract. `ThrowOnly` is computed and committed
//! for RFC step S5 but is not used for leaf-marking yet.
//!
//! The graph found what the hand audit missed: `js_array_length`'s Proxy arm
//! reaches `js_proxy_get` (#11522), and every helper whose
//! `GcRootRegistryGuard` drop can flush a deferred collection reaches the
//! collector (#11523). Both are simply not `Leaf` in the table.
//!
//! [`OVERRIDES`] holds the few symbols the archives cannot speak for. Unknown
//! stays the safe default for everything else.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::function::{FinalItem, LlFunction};
use crate::inst::LlInst;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcCallEffect {
    CannotCollect,
    /// May allocate — and therefore arm a GC trigger — but never runs a
    /// collection synchronously inside the call and never re-enters generated
    /// JS (no getters, setters, valueOf/toString coercion, or callbacks).
    ///
    /// Only meaningful under `PERRY_GC_SAFEPOINT_ONLY`: the runtime contract
    /// guarantees any trigger these helpers arm either defers to a declared
    /// safepoint (moving) or collects behind a forced conservative scan (the
    /// alloc-point valve), so the caller's precise frame roots are never
    /// consumed at this call site and it needs no statepoint. Without the
    /// contract these remain safepoints.
    AllocNoReentry,
    Unknown,
}

/// A runtime symbol's class in the generated table, ordered from least to
/// most conservative.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum RuntimeClass {
    Leaf,
    AllocOnly,
    ThrowOnly,
    Reenters,
}

impl RuntimeClass {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "Leaf" => Some(Self::Leaf),
            "AllocOnly" => Some(Self::AllocOnly),
            "ThrowOnly" => Some(Self::ThrowOnly),
            "Reenters" => Some(Self::Reenters),
            _ => None,
        }
    }
}

/// The committed per-target tables. Regenerate with
/// `scripts/gc_call_effects/regen.sh <target>` (or take the CI artifact);
/// never edit by hand.
pub(crate) const GENERATED_TABLES: &[(&str, &str)] = &[
    ("linux-x86_64", include_str!("gc_effects/linux-x86_64.tsv")),
    (
        "macos-aarch64",
        include_str!("gc_effects/macos-aarch64.tsv"),
    ),
    (
        "windows-x86_64",
        include_str!("gc_effects/windows-x86_64.tsv"),
    ),
];

/// Symbols the archives cannot speak for, each with its reason. Kept tiny on
/// purpose: an entry here shadows the graph, so it must name something the
/// graph does not contain (checked by a unit test).
pub(crate) const OVERRIDES: &[(&str, GcCallEffect, &str)] = &[];

/// Merge target tables: the most conservative class wins, and a symbol absent
/// from any table is dropped (reads as `Reenters`). Panics on a malformed row:
/// a table codegen cannot read must fail the build, never degrade silently.
pub(crate) fn merge_tables(tables: &[(&str, &str)]) -> HashMap<String, RuntimeClass> {
    let mut merged: HashMap<String, (RuntimeClass, usize)> = HashMap::new();
    for (target, text) in tables {
        let mut seen: HashSet<&str> = HashSet::new();
        for (n, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let (sym, class) = line
                .split_once('\t')
                .unwrap_or_else(|| panic!("gc_effects/{target}.tsv:{}: malformed row", n + 1));
            let class = RuntimeClass::parse(class)
                .unwrap_or_else(|| panic!("gc_effects/{target}.tsv:{}: bad class", n + 1));
            assert!(
                seen.insert(sym),
                "gc_effects/{target}.tsv:{}: duplicate {sym}",
                n + 1
            );
            let e = merged.entry(sym.to_string()).or_insert((class, 0));
            e.0 = e.0.max(class);
            e.1 += 1;
        }
    }
    merged
        .into_iter()
        .filter(|(_, (_, count))| *count == tables.len())
        .map(|(sym, (class, _))| (sym, class))
        .collect()
}

fn generated() -> &'static HashMap<String, RuntimeClass> {
    static TABLE: OnceLock<HashMap<String, RuntimeClass>> = OnceLock::new();
    TABLE.get_or_init(|| merge_tables(GENERATED_TABLES))
}

/// The merged generated class of a runtime symbol (`Reenters` if unknown).
pub(crate) fn runtime_class(name: &str) -> RuntimeClass {
    generated()
        .get(name)
        .copied()
        .unwrap_or(RuntimeClass::Reenters)
}

fn effect_of_class(class: RuntimeClass) -> GcCallEffect {
    match class {
        RuntimeClass::Leaf => GcCallEffect::CannotCollect,
        RuntimeClass::AllocOnly => GcCallEffect::AllocNoReentry,
        // The throw cut needs RFC step S3 (allocation stops collecting) first.
        RuntimeClass::ThrowOnly | RuntimeClass::Reenters => GcCallEffect::Unknown,
    }
}

/// Classify one direct LLVM callee name, without the leading `@`.
pub(crate) fn classify_direct_callee(name: &str) -> GcCallEffect {
    if let Some(&(_, effect, _)) = OVERRIDES.iter().find(|(n, _, _)| *n == name) {
        return effect;
    }
    effect_of_class(runtime_class(name))
}

/// Whether a direct external call is a Perry-GC leaf in this compile.
///
/// `AllocNoReentry` is deliberately conditional: without the strict
/// safepoint-only contract those helpers may collect synchronously at their
/// allocation site, so every caller frame on the stack still needs a
/// statepoint at the edge that reached them.
fn external_callee_cannot_collect(name: &str) -> bool {
    name.starts_with("llvm.")
        || match classify_direct_callee(name) {
            GcCallEffect::CannotCollect => true,
            GcCallEffect::AllocNoReentry => {
                crate::codegen::helpers::gc_safepoint_only_contract_enabled()
            }
            GcCallEffect::Unknown => false,
        }
}

/// The direct callee token and its argument-list opening parenthesis.
///
/// Perry's closed IR dialect emits unquoted `[-A-Za-z0-9_.$]` symbols. Search
/// for the first `%name(`/`@name(` token after the call opcode rather than the
/// first `(`: return types such as `ptr addrspace(1)` contain parentheses too.
/// Choosing the first sigil also fails closed for an indirect call whose
/// arguments later contain a direct-function constant.
fn direct_callee_span(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    // Prefer invoke before searching for `call`: a later argument or inline
    // constant may contain those bytes, but it is never the opcode of an
    // invoke line.
    let opcode = if let Some(rest) = trimmed.strip_prefix("invoke ") {
        trimmed.len() - rest.len()
    } else if let Some(pos) = trimmed.find(" = invoke ") {
        pos + " = invoke ".len()
    } else if let Some(pos) = trimmed.find("call ") {
        pos + "call ".len()
    } else {
        return None;
    };
    let tail = &trimmed[opcode..];
    let bytes = tail.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if matches!(bytes[i], b'@' | b'%') {
            let sigil = bytes[i];
            let start = i + 1;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric()
                    || matches!(bytes[end], b'_' | b'.' | b'$' | b'-'))
            {
                end += 1;
            }
            if end > start && bytes.get(end) == Some(&b'(') {
                if sigil == b'%' {
                    return None;
                }
                return Some((&tail[start..end], leading + opcode + end));
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

fn line_is_call_like(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("call ")
        || t.starts_with("tail call ")
        || t.starts_with("musttail call ")
        || t.starts_with("notail call ")
        || t.contains(" = call ")
        || t.contains(" = tail call ")
        || t.contains(" = musttail call ")
        || t.contains(" = notail call ")
        || t.starts_with("invoke ")
        || t.contains(" = invoke ")
}

fn matching_call_paren(line: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in line[open..].char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Add LLVM's call-site leaf marker to direct calls of `known_leaf_callees`.
///
/// This runs after Perry's native-root lowering, which already annotates the
/// audited runtime-helper table. It handles both `call` and `invoke`; for an
/// invoke the attribute belongs between `@callee(args)` and `to label`.
pub(crate) fn annotate_transitive_leaf_calls(
    ir: &str,
    known_leaf_callees: &HashSet<String>,
) -> String {
    if known_leaf_callees.is_empty() {
        return ir.to_string();
    }
    let mut out = String::with_capacity(ir.len());
    for line in ir.lines() {
        let rewritten = (line_is_call_like(line) && !line.contains(" asm "))
            .then(|| direct_callee_span(line))
            .flatten()
            .and_then(|(callee, open)| {
                if !known_leaf_callees.contains(callee) || line.contains("\"gc-leaf-function\"") {
                    return None;
                }
                let close = matching_call_paren(line, open)?;
                let mut marked = String::with_capacity(line.len() + 19);
                marked.push_str(&line[..=close]);
                marked.push_str(" \"gc-leaf-function\"");
                marked.push_str(&line[close + 1..]);
                Some(marked)
            });
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

#[derive(Default)]
struct FunctionEffects {
    internal_callees: HashSet<String>,
    has_collecting_edge: bool,
}

fn note_direct_callee(effects: &mut FunctionEffects, callee: &str, defined: &HashSet<&str>) {
    if defined.contains(callee) {
        effects.internal_callees.insert(callee.to_string());
    } else if !external_callee_cannot_collect(callee) {
        effects.has_collecting_edge = true;
    }
}

fn note_text_effects(line: &str, effects: &mut FunctionEffects, defined: &HashSet<&str>) {
    if !line_is_call_like(line) || line.contains("\"gc-leaf-function\"") || line.contains(" asm ") {
        return;
    }
    match direct_callee_span(line) {
        Some((callee, _)) => note_direct_callee(effects, callee, defined),
        None => effects.has_collecting_edge = true,
    }
}

fn effects_of(function: &LlFunction, defined: &HashSet<&str>) -> FunctionEffects {
    let mut effects = FunctionEffects::default();
    function
        .for_each_final_item::<std::convert::Infallible>(&mut |item| {
            match item {
                FinalItem::Inst(LlInst::Call { callee, .. }) => {
                    note_direct_callee(&mut effects, callee, defined)
                }
                FinalItem::Inst(LlInst::CallIndirect { .. }) => effects.has_collecting_edge = true,
                FinalItem::Inst(LlInst::AsmBarrier) => {}
                FinalItem::Inst(LlInst::Raw(line)) => {
                    note_text_effects(line, &mut effects, defined)
                }
                FinalItem::Text(line) => note_text_effects(line, &mut effects, defined),
                FinalItem::Label(_) | FinalItem::Blank | FinalItem::Inst(_) => {}
            }
            Ok(())
        })
        .unwrap_or_else(|e| match e {});
    effects
}

/// Compute the largest sound set of module-defined functions that cannot
/// reach Perry's collector.
///
/// Start with every definition as a candidate and remove functions with an
/// unknown/indirect/collecting external edge, then propagate removal backwards
/// through direct calls. This greatest-fixed-point formulation admits pure
/// recursive SCCs while rejecting an SCC as soon as any member can allocate,
/// poll, throw through an allocating helper, call indirectly, or leave the
/// module through an unaudited symbol.
///
/// A caller suspended below a collecting callee still needs a statepoint even
/// when collection begins only at an allocation or loop poll: the moving
/// collector must find and rewrite that caller's frame. Consequently this set
/// is the safe part of polling-style density reduction; calls outside it must
/// remain statepoints.
pub(crate) fn transitive_leaf_functions(functions: &[&LlFunction]) -> HashSet<String> {
    let defined: HashSet<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    let effects: HashMap<&str, FunctionEffects> = functions
        .iter()
        .map(|f| (f.name.as_str(), effects_of(f, &defined)))
        .collect();

    let mut collecting: HashSet<&str> = effects
        .iter()
        .filter_map(|(&name, effect)| effect.has_collecting_edge.then_some(name))
        .collect();
    // Reverse edges make propagation O(functions + calls). Re-scanning every
    // function once per newly-unsafe layer is quadratic on a long generated
    // call chain -- exactly the scale this optimization is meant to help.
    let mut callers: HashMap<&str, Vec<&str>> = HashMap::new();
    for (&caller, effect) in &effects {
        for callee in &effect.internal_callees {
            callers.entry(callee.as_str()).or_default().push(caller);
        }
    }
    let mut work: Vec<&str> = collecting.iter().copied().collect();
    while let Some(callee) = work.pop() {
        if let Some(direct_callers) = callers.get(callee) {
            for &caller in direct_callers {
                if collecting.insert(caller) {
                    work.push(caller);
                }
            }
        }
    }

    defined
        .into_iter()
        .filter(|name| !collecting.contains(name))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #11522: `js_array_length`'s Proxy arm runs the `get` trap and a
    /// `valueOf` coercion; the other three reach JS or the collector too. The
    /// hand audit called them leaf / alloc-no-reentry; the graph proves they
    /// are neither, and S1 classifies them conservatively.
    const ISSUE_11522: &[&str] = &[
        "js_array_length",
        "js_object_alloc_class_inline_keys",
        "js_object_alloc_class_inline_keys_stamped",
        "js_object_get_own_field_or_undef",
    ];

    /// #11523: these take the typed-feedback registry's root lock. With an
    /// ordinary `GcRootRegistryGuard` the release can flush a deferred
    /// collection, and the graph proved all of them collecting. The registry
    /// now uses `NonCollectingRootRegistryGuard`, a distinct type whose drop
    /// has no path to `flush_deferred_gc_request`, so the generated table
    /// proves them `Leaf` again. If this goes red, a flushing lock is back on
    /// their path: run `scripts/gc_call_effects/why.py`.
    const ISSUE_11523: &[&str] = &[
        "js_gc_note_slot_layout",
        "js_gc_note_slot_layout_aware",
        "js_gc_key_add_layout_unknown",
        "js_gc_init_typed_shape_layout",
        "js_gc_declare_typed_shape_layout",
        "js_typed_feedback_record_guard_pass",
        "js_typed_feedback_record_guard_fail",
        "js_typed_feedback_record_fallback_call",
        "js_typed_feedback_observe_property_get",
        "js_typed_feedback_observe_property_set",
        "js_typed_feedback_closure_direct_call_guard",
        "js_typed_feedback_plain_array_index_get_guard",
        "js_typed_feedback_numeric_array_index_get_guard",
        "js_typed_feedback_plain_array_index_set_guard",
        "js_typed_feedback_numeric_array_index_set_guard",
        "js_typed_feedback_numeric_array_push_guard",
        "js_packed_arraylike_loop_revalidate_live",
        "js_array_clear_numeric_layout",
        "js_array_note_numeric_write",
        "js_array_declare_all_pointer_elements",
        "js_closure_set_capture_bits",
        "js_closure_set_box_capture_ptr",
        "js_closure_set_capture_ptr",
    ];

    /// Helpers the graph proves `Leaf` on every checked target. Pinned by
    /// name because they carry most of today's leaf-marked call sites; a
    /// runtime change that taints one of them turns this red, which is the
    /// moment to look at `why.py`, not after a relocation regression.
    const PINNED_LEAF: &[&str] = &[
        "js_nanbox_pointer",
        "js_nanbox_get_pointer",
        "js_is_truthy",
        "js_string_compare",
        "js_string_addref",
        "js_gc_temp_root_push",
        "js_shadow_frame_enter",
        "js_shadow_frame_push",
        "js_shadow_frame_pop",
        "js_shadow_state_addr",
        "js_shadow_slot_bind",
        "js_shadow_slot_set",
        "js_write_barrier",
        "js_write_barrier_root_nanbox",
        "js_gc_register_global_root",
        "js_object_get_class_id",
        "js_closure_get_capture_bits",
        "js_closure_exact_func_guard",
        "js_box_alloc_bits",
        "js_box_set_bits",
        "js_i32_box_get",
        "js_box_release",
        "js_box_scope_release",
        "js_implicit_this_get",
        "js_tdz_suppress_begin",
        "js_tdz_suppress_end",
    ];

    #[test]
    fn generated_tables_parse_and_are_live() {
        for (target, text) in GENERATED_TABLES {
            assert!(
                text.contains(&format!("# target: {target}\n")),
                "gc_effects/{target}.tsv does not name its own target"
            );
            let rows = text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .count();
            assert!(
                rows >= 1000,
                "gc_effects/{target}.tsv has only {rows} rows -- a vacuous table would \
                 classify every helper Unknown and this suite would still pass"
            );
        }
        let merged = generated();
        let leaves = merged
            .values()
            .filter(|c| **c == RuntimeClass::Leaf)
            .count();
        assert!(
            merged.len() >= 1000 && leaves >= 200,
            "merged table: {} symbols, {leaves} Leaf",
            merged.len()
        );
    }

    #[test]
    fn merge_takes_the_most_conservative_class_and_drops_partial_symbols() {
        let a = "# target: a\na_only\tLeaf\nboth\tLeaf\nweaker\tAllocOnly\n";
        let b = "# target: b\nboth\tLeaf\nweaker\tReenters\n";
        let m = merge_tables(&[("a", a), ("b", b)]);
        assert_eq!(m.get("both"), Some(&RuntimeClass::Leaf));
        assert_eq!(m.get("weaker"), Some(&RuntimeClass::Reenters));
        assert_eq!(
            m.get("a_only"),
            None,
            "a symbol one target does not prove must read as Reenters"
        );
    }

    #[test]
    #[should_panic(expected = "bad class")]
    fn a_malformed_table_fails_the_build() {
        merge_tables(&[("x", "sym\tMaybeLeaf\n")]);
    }

    /// The classification of every runtime callee flows from the table:
    /// Leaf -> CannotCollect, AllocOnly -> AllocNoReentry, else Unknown.
    #[test]
    fn classification_flows_from_the_generated_table() {
        let merged = generated();
        let mut seen = [0usize; 4];
        for (sym, class) in merged {
            if OVERRIDES.iter().any(|(n, _, _)| n == sym) {
                continue;
            }
            let want = match class {
                RuntimeClass::Leaf => GcCallEffect::CannotCollect,
                RuntimeClass::AllocOnly => GcCallEffect::AllocNoReentry,
                RuntimeClass::ThrowOnly | RuntimeClass::Reenters => GcCallEffect::Unknown,
            };
            assert_eq!(classify_direct_callee(sym), want, "{sym} ({class:?})");
            seen[*class as usize] += 1;
        }
        assert!(
            seen[0] > 0 && seen[1] > 0 && seen[3] > 0,
            "table exercises too few classes: {seen:?}"
        );
        assert_eq!(
            classify_direct_callee("user_function_not_in_any_archive"),
            GcCallEffect::Unknown
        );
    }

    #[test]
    fn overrides_name_only_symbols_the_graph_cannot_see() {
        for (name, _, reason) in OVERRIDES {
            assert!(reason.len() >= 20, "override {name} needs a real reason");
            assert!(
                !generated().contains_key(*name),
                "override {name} shadows a symbol the generated table already classifies"
            );
        }
    }

    #[test]
    fn pinned_hot_helpers_are_leaf() {
        for name in PINNED_LEAF {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name}: run scripts/gc_call_effects/why.py to see what now taints it"
            );
        }
    }

    #[test]
    fn issue_11522_helpers_are_not_leaf() {
        for name in ISSUE_11522 {
            assert_ne!(runtime_class(name), RuntimeClass::Leaf, "{name}");
            assert_ne!(runtime_class(name), RuntimeClass::AllocOnly, "{name}");
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name}"
            );
        }
    }

    #[test]
    fn issue_11523_noncollecting_guard_helpers_are_provably_leaf() {
        let mut leaf = 0;
        for name in ISSUE_11523 {
            // Six (the numeric index guards and the lazy-array layout
            // helpers) reach JS or a materializing allocator on their own
            // paths; none may be collecting through the root lock alone,
            // which is exactly what AllocOnly would mean here.
            assert_ne!(
                runtime_class(name),
                RuntimeClass::AllocOnly,
                "{name} reaches a collector again (a flushing root lock?)"
            );
            leaf += usize::from(classify_direct_callee(name) == GcCallEffect::CannotCollect);
        }
        assert!(leaf >= 15, "only {leaf} of the #11523 helpers are Leaf");
    }

    /// The box/closure family's containment in the root-dominance checker's
    /// `NONCOLLECTING` authority: every member the generated table proves
    /// `Leaf` must be there, so codegen never leaf-marks a call the checker
    /// would call collecting (the #7510 drift). The members #11523 demoted
    /// are simply no longer leaves; containment is only required in the safe
    /// direction.
    #[test]
    fn box_and_closure_helpers_stay_contained_in_the_checker_authority() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let py_path = format!("{manifest}/../../scripts/gc_root_dominance_check.py");
        let py_src = std::fs::read_to_string(&py_path).expect("read gc_root_dominance_check.py");
        let noncollecting = extract_python_set(&py_src, "NONCOLLECTING");
        assert!(
            noncollecting.len() > 50,
            "parsed only {} NONCOLLECTING names -- the extraction broke",
            noncollecting.len()
        );
        let family: Vec<&str> = [
            "js_closure_get_capture_bits",
            "js_closure_get_capture_ptr",
            "js_box_alloc_bits",
            "js_i32_box_alloc",
            "js_bool_box_alloc",
            "js_box_set_bits",
            "js_box_set_bits_trusted_no_barrier",
            "js_i32_box_set",
            "js_bool_box_set",
            "js_i32_box_get",
            "js_bool_box_get",
            "js_box_release",
            "js_i32_box_release",
            "js_bool_box_release",
            "js_box_scope_release",
            "js_i32_box_scope_release",
            "js_bool_box_scope_release",
        ]
        .into_iter()
        .filter(|n| classify_direct_callee(n) == GcCallEffect::CannotCollect)
        .collect();
        assert!(
            family.len() >= 8,
            "only {} of the box/closure family is Leaf ({family:?})",
            family.len()
        );
        let missing: Vec<&str> = family
            .iter()
            .filter(|n| !noncollecting.contains(**n))
            .copied()
            .collect();
        assert!(
            missing.is_empty(),
            "Leaf box/closure callees absent from NONCOLLECTING: {missing:?}"
        );
    }

    /// Collect the string literals of a top-level `NAME = {...}` python set.
    fn extract_python_set<'a>(src: &'a str, name: &str) -> std::collections::HashSet<&'a str> {
        let mut out = std::collections::HashSet::new();
        let mut inside = false;
        for line in src.lines() {
            if !inside {
                if line.starts_with(&format!("{name} = {{")) {
                    inside = true;
                }
                continue;
            }
            if line == "}" {
                break;
            }
            let code = line.split('#').next().unwrap_or("");
            out.extend(quoted_literals(code));
        }
        out
    }

    fn quoted_literals(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(a) = rest.find('"') {
            let tail = &rest[a + 1..];
            let Some(b) = tail.find('"') else { break };
            out.push(&tail[..b]);
            rest = &tail[b + 1..];
        }
        out
    }

    /// #11522's runtime split: the plain-array fast lane is a separate export
    /// the graph proves leaf, and `js_array_length` itself stays collecting.
    #[test]
    fn array_length_is_split_into_a_leaf_lane_and_a_collecting_call() {
        assert_eq!(
            classify_direct_callee("js_array_length"),
            GcCallEffect::Unknown,
            "js_array_length reaches js_proxy_get / js_number_coerce"
        );
        assert_eq!(
            classify_direct_callee("js_array_length_leaf"),
            GcCallEffect::CannotCollect
        );
        assert!(!external_callee_cannot_collect("js_array_length"));
        assert!(external_callee_cannot_collect("js_array_length_leaf"));
    }

    #[test]
    fn register_global_root_tracks_the_barrier_it_wraps() {
        assert_eq!(
            classify_direct_callee("js_gc_register_global_root"),
            classify_direct_callee("js_write_barrier_root_heap_word"),
            "js_gc_register_global_root's entire body is that barrier plus a \
             TLS Vec::push; they cannot have different GC effects"
        );
    }

    /// Helpers that allocate, run JS or throw must never be `CannotCollect`.
    /// `js_nanbox_string`'s null guard allocates an empty string; the TDZ box
    /// getters throw a ReferenceError; the sloppy `this` reader boxes
    /// primitives.
    #[test]
    fn allocating_reentering_and_throwing_helpers_are_not_cannot_collect() {
        for name in [
            "js_nanbox_string",
            "js_string_from_bytes",
            "js_array_alloc",
            "js_string_compare_value",
            "js_rel_lt",
            "js_rel_gt",
            "js_object_get_field_ic_miss_packed",
            "js_object_get_field_ic_slow",
            "js_object_get_field_ic_nonptr",
            "js_implicit_this_get_sloppy",
            "js_box_get_bits",
            "js_box_get_bits_trusted",
            "js_box_get_bits_named",
            "js_box_get_bits_trusted_named",
            "js_value_length_f64",
            "js_value_length_property_f64",
            "js_value_length_property_ic_f64",
            "js_array_get_f64",
        ] {
            assert_ne!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name} can allocate, throw or run JS and must not be marked gc-leaf"
            );
        }
    }

    #[test]
    fn collection_polls_and_reentering_calls_stay_conservative() {
        for name in [
            "js_gc_collect",
            "js_gc_loop_safepoint",
            "js_array_map",
            "js_array_sort_with_comparator",
            "js_number_coerce",
            "js_dynamic_string_or_number_add",
            "js_object_get_field_by_name_f64",
            "js_native_call_value",
            "js_proxy_get",
            "user_function",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name} is a poll or can re-enter JS: never a leaf, never AllocNoReentry"
            );
        }
    }

    fn void_function(name: &str, calls: &[&str]) -> LlFunction {
        let mut f = LlFunction::new(name, crate::types::VOID, vec![]);
        let entry = f.create_block("entry");
        for callee in calls {
            entry.call_void(callee, &[]);
        }
        entry.ret_void();
        f
    }

    /// #8596: the greatest fixed point admits a pure recursive component, but
    /// one collecting exit poisons every direct caller that can reach it.
    #[test]
    fn transitive_leaf_closure_handles_recursion_and_collecting_exits() {
        let leaf = void_function("leaf", &["js_nanbox_pointer"]);
        let wrapper = void_function("wrapper", &["leaf"]);
        let recursive_a = void_function("recursive_a", &["recursive_b"]);
        let recursive_b = void_function("recursive_b", &["recursive_a"]);
        let mut allocating = LlFunction::new("allocating", crate::types::I64, vec![]);
        let entry = allocating.create_block("entry");
        entry.call_void("js_array_alloc", &[]);
        entry.ret(crate::types::I64, "0");
        let reaches_allocating = void_function("reaches_allocating", &["allocating"]);
        let functions = [
            &leaf,
            &wrapper,
            &recursive_a,
            &recursive_b,
            &allocating,
            &reaches_allocating,
        ];

        let safe = transitive_leaf_functions(&functions);
        for name in ["leaf", "wrapper", "recursive_a", "recursive_b"] {
            assert!(safe.contains(name), "{name} should be transitively leaf");
        }
        for name in ["allocating", "reaches_allocating"] {
            assert!(
                !safe.contains(name),
                "{name} reaches Perry allocation and must remain a safepoint callee"
            );
        }
    }

    #[test]
    fn indirect_and_unknown_external_edges_fail_closed() {
        let unknown = void_function("unknown", &["cross_module_function"]);
        let mut indirect = LlFunction::new("indirect", crate::types::VOID, vec![]);
        let entry = indirect.create_block("entry");
        entry.call_indirect(crate::types::I64, "%callback", &[]);
        entry.ret_void();
        let mut guarded = LlFunction::new("guarded", crate::types::VOID, vec![]);
        let entry = guarded.create_block("entry");
        entry.call_indirect_gc_leaf(crate::types::I64, "%callback", &[]);
        entry.ret_void();
        let allocating = void_function("allocating", &["js_array_alloc"]);
        let mut guarded_direct = LlFunction::new("guarded_direct", crate::types::VOID, vec![]);
        let entry = guarded_direct.create_block("entry");
        entry.call_gc_leaf(crate::types::I64, "allocating", &[]);
        entry.ret_void();
        let functions = [&unknown, &indirect, &guarded, &allocating, &guarded_direct];

        let safe = transitive_leaf_functions(&functions);
        assert!(
            safe.is_empty(),
            "unknown, indirect, and collecting direct calls must fail closed; a guarded call-site marker must not turn its containing function transitively leaf"
        );
    }

    #[test]
    fn annotates_call_and_invoke_at_the_llvm_attribute_position() {
        let known = HashSet::from(["pure".to_string()]);
        let ir = "  %a = call ptr addrspace(1) @pure(ptr addrspace(1) %p)\n\
                  %b = invoke preserve_nonecc double @pure(double %x) to label %ok unwind label %pad\n\
                  %c = call double @collecting()\n\
                  %d = call double %callback(ptr @pure)\n\
                  ; call void @pure() is documentation, not an instruction\n";
        let marked = annotate_transitive_leaf_calls(ir, &known);
        assert!(marked.contains(
            "%a = call ptr addrspace(1) @pure(ptr addrspace(1) %p) \"gc-leaf-function\""
        ));
        assert!(marked.contains(
            "%b = invoke preserve_nonecc double @pure(double %x) \"gc-leaf-function\" to label %ok unwind label %pad"
        ));
        assert!(marked.contains("%c = call double @collecting()\n"));
        assert!(marked.contains("%d = call double %callback(ptr @pure)\n"));
        assert_eq!(
            marked.matches("\"gc-leaf-function\"").count(),
            2,
            "only the two proven direct calls may be annotated:\n{marked}"
        );
    }

    /// End-to-end emission witness: the analysis is module-wide and the leaf
    /// set reaches a rooted caller's final IR. The allocating sibling is the
    /// discriminating control and must remain unmarked for RS4GC to rewrite.
    #[test]
    fn module_marks_only_transitively_noncollecting_generated_calls() {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let mut module = crate::module::LlModule::new(crate::codegen::default_target_triple());
        module.declare_function("js_array_alloc", crate::types::I64, &[crate::types::I32]);
        module.declare_function(
            "js_shadow_slot_bind",
            crate::types::VOID,
            &[crate::types::I32, crate::types::PTR],
        );

        let pure = module.define_function("pure_generated", crate::types::VOID, vec![]);
        pure.create_block("entry").ret_void();

        let allocating = module.define_function("allocating_generated", crate::types::VOID, vec![]);
        let entry = allocating.create_block("entry");
        entry.call(
            crate::types::I64,
            "js_array_alloc",
            &[(crate::types::I32, "0")],
        );
        entry.ret_void();

        let caller = module.define_function("rooted_caller", crate::types::VOID, vec![]);
        caller.enable_shadow_frame(0);
        let slot = caller.reserve_shadow_slot().expect("reserve native root");
        let root = caller.alloca_entry(crate::types::I64);
        caller.entry_allocas_push_store(crate::types::I64, "0", &root);
        caller.entry_setup_call_void(
            "js_shadow_slot_bind",
            &[
                (crate::types::I32, &slot.to_string()),
                (crate::types::PTR, &root),
            ],
        );
        let entry = caller.create_block("entry");
        entry.call_void("pure_generated", &[]);
        entry.call_void("allocating_generated", &[]);
        entry.ret_void();

        let ir = module.to_ir();
        assert!(ir.contains("call void @pure_generated() \"gc-leaf-function\""));
        assert!(
            ir.contains("call void @allocating_generated()")
                && !ir.contains("call void @allocating_generated() \"gc-leaf-function\""),
            "allocating generated callee must remain a statepoint edge:\n{ir}"
        );

        #[cfg(feature = "llvm-inprocess")]
        {
            let target = crate::codegen::default_target_triple();
            let rewritten = crate::inprocess::statepoint_rewritten_ir(
                &ir,
                &target,
                "transitive_leaf_generated_calls",
            )
            .expect("module-wide leaf witness must survive RS4GC");
            assert!(
                rewritten.contains("call void @pure_generated()"),
                "proven leaf call was unexpectedly rewritten:\n{rewritten}"
            );
            assert!(
                rewritten.lines().any(|line| {
                    line.contains("@llvm.experimental.gc.statepoint")
                        && line.contains("@allocating_generated")
                }),
                "collecting generated call did not become a statepoint:\n{rewritten}"
            );
        }
    }
}
