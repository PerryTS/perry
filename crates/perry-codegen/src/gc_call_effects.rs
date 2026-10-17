//! Perry-GC call effects for native-stack safepoint lowering.
//!
//! This is deliberately narrower than LLVM's memory-effect attributes. A
//! helper may mutate runtime metadata, take a lock, or allocate through the
//! system allocator and still be safe to omit as a Perry GC safepoint. The
//! only question answered here is: can this call enter Perry's collector?
//!
//! # Source of truth: the table of the linked archives (RFC deferred collection, S1)
//!
//! The answer for a runtime helper is not a hand-kept allowlist and not a
//! checked-in table. It is the table of the runtime archives this compilation
//! links: computed from their machine code by `perry-gc-effects sidecars` when
//! the archives are built, shipped beside each archive as
//! `<archive>.gc-effects`, and verified against the archive bytes by the
//! compiler before codegen (`perry`'s `gc_effects_sidecar`). The analyzer
//! builds a symbol-level call graph from the object code and classifies every
//! exported symbol by what it can reach (seeds, cuts and every audited
//! exemption, each with its reason, are in `scripts/gc_call_effects/seeds.txt`):
//!
//! * `Leaf`      reaches no collector entry, poll, JS entry, unaudited
//!               indirect call or unknown external symbol;
//! * `AllocOnly` reaches a collector only through a collector entry point
//!               (the allocation trigger), never JS or a poll;
//! * `ThrowOnly` additionally reaches JS only through the throw funnel;
//! * `Reenters`  everything else.
//!
//! Every row also carries a `throws` bit (a fourth solve, see the analyzer's
//! docs): `yes` if a call can reach `js_throw`, an unwinding raise, JavaScript
//! or an unaudited callee. It is independent of the class (a poll is
//! `Reenters` but may be unable to throw); `Leaf` and `AllocOnly` imply `no`.
//! [`GcEffects::runtime_throws`] reads it, and `eh_mode::nothrow_callee` is its
//! only consumer: a symbol absent from the table is `throws=yes`.
//!
//! The table is per target and per archive: one compilation reads exactly the
//! archive it links, and a symbol absent from it is `Reenters`. Under today's
//! runtime (an allocation can still start a collection) only `Leaf` may be
//! marked `"gc-leaf-function"`. `AllocOnly` keeps its previous meaning,
//! [`GcCallEffect::AllocNoReentry`]: a leaf only under the research-only
//! `PERRY_GC_SAFEPOINT_ONLY` contract. `ThrowOnly` is computed for RFC step S5
//! but is not used for leaf-marking yet.
//!
//! The graph found what the hand audit missed: `js_array_length`'s Proxy arm
//! reaches `js_proxy_get` (#11522), and every helper whose
//! `GcRootRegistryGuard` drop can flush a deferred collection reaches the
//! collector (#11523). Both are simply not `Leaf` in the table.
//!
//! [`OVERRIDES`] holds the few symbols the archives cannot speak for. Unknown
//! stays the safe default for everything else.

use std::collections::{HashMap, HashSet};

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

/// A runtime symbol's class in the archive's table, ordered from least to
/// most conservative.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum RuntimeClass {
    Leaf,
    AllocOnly,
    ThrowOnly,
    Reenters,
}

impl RuntimeClass {
    #[cfg(test)]
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

/// What the archive's table says about one runtime symbol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeFacts {
    pub(crate) class: RuntimeClass,
    /// A call can unwind (reach `js_throw`, a raise, JS or an unaudited callee).
    pub(crate) throws: bool,
}

/// Symbols the archives cannot speak for, each with its reason. Kept tiny on
/// purpose: an entry here shadows the graph, so it must name something the
/// archives do not export (checked over the workspace sidecar by
/// `perry-gc-effects`' `workspace_sidecar` test, which reads this list).
pub(crate) const OVERRIDES: &[(&str, GcCallEffect, &str)] = &[
    ("perry_birth_class", GcCallEffect::AllocNoReentry,
     "Codegen-defined shared birth: only slow_alloc can collect; initialization and seeding are leaf"),
    ("perry_birth_empty_array", GcCallEffect::AllocNoReentry,
     "Codegen-defined shared birth: only slow_alloc can collect; initialization and seeding are leaf"),
];

/// The runtime call-effect facts one compilation reads.
///
/// Built once per compilation and carried explicitly (`CompileOptions::
/// gc_effects`, then the module and every function's register counter), never
/// read from a process global: two compilations in one process may hold
/// different tables, and the object-cache key carries [`GcEffects::digest`] so
/// a module compiled under one table is never served for another.
///
/// The rows are the verified sidecar table of the runtime archive the program
/// links ([`GcEffects::from_sidecar`]), or nothing at all
/// ([`GcEffects::all_unsafe`]) when no table speaks for the link set.
pub struct GcEffects {
    rows: HashMap<String, RuntimeFacts>,
    digest: u64,
}

impl std::fmt::Debug for GcEffects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcEffects")
            .field("rows", &self.rows.len())
            .field("digest", &format_args!("{:016x}", self.digest))
            .finish()
    }
}

/// The default proves nothing ([`GcEffects::all_unsafe`]): a table is only
/// ever the table of the archives a compilation links, and the compiler
/// installs it in every compilation's `CompileOptions`. perry-codegen's own
/// unit tests, which build `CompileOptions::default()` with no archives, get
/// their explicit test world instead ([`test_effects`]), exactly as a module
/// built outside a compilation does ([`GcEffects::outside_compilation`]).
impl Default for GcEffects {
    fn default() -> Self {
        #[cfg(test)]
        {
            table_from_rows(TEST_WORLD)
        }
        #[cfg(not(test))]
        {
            Self::all_unsafe()
        }
    }
}

impl GcEffects {
    /// The table of a verified sidecar section: the runtime archive (alone or
    /// with its stdlib) this compilation links, as the analyzer classified its
    /// bytes.
    pub fn from_sidecar(table: &perry_gc_effects::Table) -> Self {
        use perry_gc_effects::Class;
        let rows: HashMap<String, RuntimeFacts> = table
            .iter()
            .map(|(sym, e)| {
                let class = match e.class {
                    Class::Leaf => RuntimeClass::Leaf,
                    Class::AllocOnly => RuntimeClass::AllocOnly,
                    Class::ThrowOnly => RuntimeClass::ThrowOnly,
                    Class::Reenters => RuntimeClass::Reenters,
                };
                (
                    sym.clone(),
                    RuntimeFacts {
                        class,
                        throws: e.throws,
                    },
                )
            })
            .collect();
        let digest = digest_of(&rows);
        Self { rows, digest }
    }

    /// The table that proves nothing: every runtime symbol reads `Reenters`,
    /// `throws=yes` ([`OVERRIDES`] still apply). Sound for any link set. A
    /// compilation no table speaks for compiles under it: an installed perry
    /// whose sidecar is missing or does not match the archives, a WASI build
    /// (no native link set), a `--no-link` compile with no runtime archive, a
    /// runtime cargo builds after codegen (Geisterhand), and a module built
    /// outside a compilation.
    pub fn all_unsafe() -> Self {
        let rows = HashMap::new();
        let digest = digest_of(&rows);
        Self { rows, digest }
    }

    /// The table a function or module built outside a compilation reads. In a
    /// unit test it is the test world ([`test_effects`]); in the compiler it
    /// proves nothing, because only a compilation knows its archives and every
    /// compilation installs its own `CompileOptions::gc_effects`.
    pub(crate) fn outside_compilation() -> std::sync::Arc<Self> {
        #[cfg(test)]
        {
            std::sync::Arc::clone(test_effects())
        }
        #[cfg(not(test))]
        {
            std::sync::Arc::new(Self::all_unsafe())
        }
    }

    /// The number of symbols the table proves anything about (diagnostics).
    pub fn proven_rows(&self) -> usize {
        self.rows
            .values()
            .filter(|f| f.class != RuntimeClass::Reenters || !f.throws)
            .count()
    }

    /// A stable digest of everything an answer depends on: the merged rows
    /// (sorted, so map order never leaks in) and the codegen-defined
    /// [`OVERRIDES`]. Part of every module's object-cache key.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// The merged class of a runtime symbol (`Reenters` if unknown).
    pub(crate) fn runtime_class(&self, name: &str) -> RuntimeClass {
        self.rows
            .get(name)
            .map_or(RuntimeClass::Reenters, |f| f.class)
    }

    /// Whether a call to this runtime symbol can unwind, merged over the
    /// target tables. A symbol no table proves (user code, a new helper, a
    /// name missing on one target) answers `true`: the caller keeps its
    /// `invoke`.
    pub(crate) fn runtime_throws(&self, name: &str) -> bool {
        self.rows.get(name).is_none_or(|f| f.throws)
    }

    /// Classify one direct LLVM callee name, without the leading `@`.
    pub(crate) fn classify_direct_callee(&self, name: &str) -> GcCallEffect {
        if let Some(&(_, effect, _)) = OVERRIDES.iter().find(|(n, _, _)| *n == name) {
            return effect;
        }
        effect_of_class(self.runtime_class(name))
    }

    /// Whether a direct external call is a Perry-GC leaf in this compile.
    ///
    /// `AllocNoReentry` is deliberately conditional: without the strict
    /// safepoint-only contract those helpers may collect synchronously at
    /// their allocation site, so every caller frame on the stack still needs a
    /// statepoint at the edge that reached them.
    fn external_callee_cannot_collect(&self, name: &str) -> bool {
        name.starts_with("llvm.")
            || match self.classify_direct_callee(name) {
                GcCallEffect::CannotCollect => true,
                GcCallEffect::AllocNoReentry => {
                    crate::codegen::helpers::gc_safepoint_only_contract_enabled()
                }
                GcCallEffect::Unknown => false,
            }
    }
}

/// FNV-1a over the sorted rows and the overrides.
fn digest_of(rows: &HashMap<String, RuntimeFacts>) -> u64 {
    fn eat(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Field separator, so ("ab","c") and ("a","bc") differ.
        *h ^= 0xff;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut sorted: Vec<(&String, &RuntimeFacts)> = rows.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (sym, f) in sorted {
        eat(&mut h, sym.as_bytes());
        eat(&mut h, &[f.class as u8, u8::from(f.throws)]);
    }
    eat(&mut h, b"overrides");
    for (name, effect, _) in OVERRIDES {
        eat(&mut h, name.as_bytes());
        eat(&mut h, &[*effect as u8]);
    }
    h
}

fn effect_of_class(class: RuntimeClass) -> GcCallEffect {
    match class {
        RuntimeClass::Leaf => GcCallEffect::CannotCollect,
        RuntimeClass::AllocOnly => GcCallEffect::AllocNoReentry,
        // The throw cut needs RFC step S3 (allocation stops collecting) first.
        RuntimeClass::ThrowOnly | RuntimeClass::Reenters => GcCallEffect::Unknown,
    }
}

/// An explicit table from `symbol<TAB>class<TAB>throws` rows (`#` lines are
/// comments), for unit tests: a test states the table it means. Panics on a
/// malformed row.
#[cfg(test)]
pub(crate) fn table_from_rows(text: &str) -> GcEffects {
    let mut rows: HashMap<String, RuntimeFacts> = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(sym), Some(class), Some(throws), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            panic!("test table line {}: malformed row", n + 1)
        };
        let class = RuntimeClass::parse(class)
            .unwrap_or_else(|| panic!("test table line {}: bad class", n + 1));
        let throws = match throws {
            "yes" => true,
            "no" => false,
            _ => panic!("test table line {}: bad throws", n + 1),
        };
        assert!(
            rows.insert(sym.to_string(), RuntimeFacts { class, throws })
                .is_none(),
            "test table line {}: duplicate {sym}",
            n + 1
        );
    }
    let digest = digest_of(&rows);
    GcEffects { rows, digest }
}

/// The test world: the runtime facts the codegen unit tests assume, stated
/// explicitly (`gc_effects_test_world.tsv`). It is not a description of any
/// runtime archive and production never reads it; the facts about the real
/// archives are checked where the archives are: the pinned-facts test of
/// `perry-gc-effects` over the workspace sidecar.
#[cfg(test)]
pub(crate) fn test_effects() -> &'static std::sync::Arc<GcEffects> {
    static WORLD: std::sync::OnceLock<std::sync::Arc<GcEffects>> = std::sync::OnceLock::new();
    WORLD.get_or_init(|| std::sync::Arc::new(table_from_rows(TEST_WORLD)))
}

#[cfg(test)]
const TEST_WORLD: &str = include_str!("gc_effects_test_world.tsv");

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

fn note_direct_callee(
    gc: &GcEffects,
    effects: &mut FunctionEffects,
    callee: &str,
    defined: &HashSet<&str>,
) {
    if defined.contains(callee) {
        effects.internal_callees.insert(callee.to_string());
    } else if !gc.external_callee_cannot_collect(callee) {
        effects.has_collecting_edge = true;
    }
}

fn note_text_effects(
    gc: &GcEffects,
    line: &str,
    effects: &mut FunctionEffects,
    defined: &HashSet<&str>,
) {
    if !line_is_call_like(line) || line.contains("\"gc-leaf-function\"") || line.contains(" asm ") {
        return;
    }
    match direct_callee_span(line) {
        Some((callee, _)) => note_direct_callee(gc, effects, callee, defined),
        None => effects.has_collecting_edge = true,
    }
}

fn effects_of(gc: &GcEffects, function: &LlFunction, defined: &HashSet<&str>) -> FunctionEffects {
    let mut effects = FunctionEffects::default();
    function
        .for_each_final_item::<std::convert::Infallible>(&mut |item| {
            match item {
                FinalItem::Inst(LlInst::Call { callee, .. }) => {
                    note_direct_callee(gc, &mut effects, callee, defined)
                }
                FinalItem::Inst(LlInst::CallIndirect { .. }) => effects.has_collecting_edge = true,
                FinalItem::Inst(LlInst::AsmBarrier) => {}
                FinalItem::Inst(LlInst::Raw(line)) => {
                    note_text_effects(gc, line, &mut effects, defined)
                }
                FinalItem::Text(line) => note_text_effects(gc, line, &mut effects, defined),
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
pub(crate) fn transitive_leaf_functions(
    gc: &GcEffects,
    functions: &[&LlFunction],
) -> HashSet<String> {
    let defined: HashSet<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    let effects: HashMap<&str, FunctionEffects> = functions
        .iter()
        .map(|f| (f.name.as_str(), effects_of(gc, f, &defined)))
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

    // The unit tests have no compilation to take a table from: they read the
    // test world (`test_effects`) or build the table they mean. The facts
    // about the real runtime archives (pinned hot leaves, #11522, #11523, the
    // polls, the root-dominance checker's authority) are checked over the
    // workspace sidecar by `perry-gc-effects`' `workspace_sidecar` test.
    fn classify_direct_callee(name: &str) -> GcCallEffect {
        test_effects().classify_direct_callee(name)
    }
    fn external_callee_cannot_collect(name: &str) -> bool {
        test_effects().external_callee_cannot_collect(name)
    }
    fn transitive_leaf_functions(functions: &[&LlFunction]) -> HashSet<String> {
        super::transitive_leaf_functions(test_effects(), functions)
    }

    #[test]
    fn a_symbol_no_table_proves_is_a_collecting_edge() {
        // A helper the archive does not export (new, renamed, or user code)
        // is unknown: conservative in ordinary and safepoint-only builds.
        for name in [
            "js_object_finalize_constfn_static",
            "user_function_not_in_any_archive",
        ] {
            assert!(!test_effects().rows.contains_key(name));
            assert_eq!(classify_direct_callee(name), GcCallEffect::Unknown);
            assert!(!external_callee_cannot_collect(name));
            assert!(test_effects().runtime_throws(name));
            // No handwritten override may hide a helper's effects.
            assert!(!OVERRIDES.iter().any(|(symbol, _, _)| *symbol == name));
        }
    }

    #[test]
    fn the_digest_follows_the_rows_and_only_the_rows() {
        let a = "# target: a\nx\tLeaf\tno\ny\tReenters\tyes\n";
        let a_reordered = "# target: a\ny\tReenters\tyes\nx\tLeaf\tno\n";
        let a_other_comment = "# target: z\nx\tLeaf\tno\ny\tReenters\tyes\n";
        let class_changed = "# target: a\nx\tAllocOnly\tno\ny\tReenters\tyes\n";
        let throws_changed = "# target: a\nx\tLeaf\tyes\ny\tReenters\tyes\n";
        let row_added = "# target: a\nx\tLeaf\tno\ny\tReenters\tyes\nw\tLeaf\tno\n";
        let d = |t: &str| table_from_rows(t).digest();
        assert_eq!(
            d(a),
            d(a_reordered),
            "row order never leaks into the digest"
        );
        assert_eq!(d(a), d(a_other_comment), "comments are not rows");
        for changed in [class_changed, throws_changed, row_added] {
            assert_ne!(d(a), d(changed));
        }
        assert_ne!(d(a), GcEffects::all_unsafe().digest());
    }

    #[test]
    fn two_tables_answer_independently() {
        let leaf = table_from_rows("helper\tLeaf\tno\n");
        let slow = table_from_rows("helper\tReenters\tyes\n");
        assert_eq!(
            leaf.classify_direct_callee("helper"),
            GcCallEffect::CannotCollect
        );
        assert_eq!(slow.classify_direct_callee("helper"), GcCallEffect::Unknown);
        assert!(!leaf.runtime_throws("helper"));
        assert!(slow.runtime_throws("helper"));
    }

    #[test]
    #[should_panic(expected = "bad class")]
    fn a_malformed_table_fails() {
        table_from_rows("sym\tMaybeLeaf\tno\n");
    }

    #[test]
    #[should_panic(expected = "malformed row")]
    fn a_row_without_the_throws_column_fails() {
        table_from_rows("sym\tLeaf\n");
    }

    #[test]
    #[should_panic(expected = "bad throws")]
    fn a_bad_throws_value_fails() {
        table_from_rows("sym\tLeaf\tmaybe\n");
    }

    fn sidecar_table(rows: &[(&str, perry_gc_effects::Class, bool)]) -> perry_gc_effects::Table {
        rows.iter()
            .map(|(sym, class, throws)| {
                (
                    (*sym).to_string(),
                    perry_gc_effects::Entry {
                        class: *class,
                        throws: *throws,
                    },
                )
            })
            .collect()
    }

    /// The classification of every runtime callee flows from the sidecar
    /// table: Leaf -> CannotCollect, AllocOnly -> AllocNoReentry, else
    /// Unknown, with the throws column carried as is.
    #[test]
    fn classification_flows_from_the_sidecar_table() {
        use perry_gc_effects::Class;
        let t = GcEffects::from_sidecar(&sidecar_table(&[
            ("js_l", Class::Leaf, false),
            ("js_a", Class::AllocOnly, false),
            ("js_t", Class::ThrowOnly, true),
            ("js_r", Class::Reenters, true),
            ("js_poll", Class::Reenters, false),
        ]));
        assert_eq!(t.proven_rows(), 4);
        assert_eq!(
            t.classify_direct_callee("js_l"),
            GcCallEffect::CannotCollect
        );
        assert_eq!(
            t.classify_direct_callee("js_a"),
            GcCallEffect::AllocNoReentry
        );
        assert_eq!(t.classify_direct_callee("js_t"), GcCallEffect::Unknown);
        assert_eq!(t.classify_direct_callee("js_r"), GcCallEffect::Unknown);
        assert_eq!(t.runtime_class("js_poll"), RuntimeClass::Reenters);
        assert!(!t.runtime_throws("js_poll"));
        assert!(t.runtime_throws("js_t"));
        assert!(!t.runtime_throws("js_l"));
        assert_eq!(
            t.classify_direct_callee("user_function_not_in_any_archive"),
            GcCallEffect::Unknown
        );
        let same = GcEffects::from_sidecar(&sidecar_table(&[
            ("js_poll", Class::Reenters, false),
            ("js_r", Class::Reenters, true),
            ("js_t", Class::ThrowOnly, true),
            ("js_a", Class::AllocOnly, false),
            ("js_l", Class::Leaf, false),
        ]));
        assert_eq!(t.digest(), same.digest());
    }

    #[test]
    fn overrides_carry_a_reason() {
        for (name, _, reason) in OVERRIDES {
            assert!(reason.len() >= 20, "override {name} needs a real reason");
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

    /// The no-table fallback: nothing proven, overrides kept.
    #[test]
    fn all_unsafe_proves_no_runtime_symbol() {
        let u = GcEffects::all_unsafe();
        assert_eq!(u.proven_rows(), 0);
        for name in ["js_nanbox_pointer", "js_is_truthy", "js_string_compare"] {
            assert_eq!(u.runtime_class(name), RuntimeClass::Reenters);
            assert!(u.runtime_throws(name));
            assert_eq!(u.classify_direct_callee(name), GcCallEffect::Unknown);
        }
        for (name, effect, _) in OVERRIDES {
            assert_eq!(u.classify_direct_callee(name), *effect);
        }
        // In these tests the default is the test world (outside the tests it
        // is the all-unsafe table: `not(test)` arm of `Default`).
        assert_eq!(GcEffects::default().digest(), test_effects().digest());
        assert!(test_effects().proven_rows() > 0);
        assert_ne!(u.digest(), test_effects().digest());
    }
}
