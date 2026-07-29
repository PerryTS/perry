//! Representation-selection Phase 2 (RFC `docs/representation-selection-rfc.md`
//! §5.4): the specialized calling convention — bounded monomorphization.
//!
//! Generalizes the existing three-symbol typed_abi scheme in three directions:
//!
//! 1. **Full-body specialized entries.** The specialized clone is compiled by
//!    the REAL `compile_function` parameterized on a representation tuple
//!    (`SpecParamRep` per param) — loops, branches, calls, the unrolled
//!    bcryptjs `_encipher` all lower through the ordinary statement lowerer.
//!    The public boxed entry always exists and stays the permanent ABI.
//! 2. **`TaPtr` params.** A proven typed-array param binds at entry as a
//!    proven `BufferViewSlot` (data pointer + length hoisted ONCE from the
//!    header). That hoist is sound because the typed-array HEADER itself never
//!    moves — `typed_array_alloc` places header + inline payload in the OLD
//!    arena (`GC_FLAG_TENURED`), which the nursery copying minor never
//!    relocates and old-page defrag skips (`gc_type_is_movable` is `false`
//!    for `GC_TYPE_TYPED_ARRAY`) — and because a non-view typed array cannot
//!    be detached or resized. The older phrasing here ("typed-array storage
//!    never moves") named the wrong object: what is passed and hoisted THROUGH
//!    is the header, which is an object; only its old-arena residency makes
//!    the address stable (#6981). Do not carry this shortcut to any other
//!    representation without re-arguing it. So element accesses lower
//!    through the strong bare-load machinery with bounds checks against the
//!    entry-hoisted length — NEVER through the per-site guarded fast paths
//!    (measured to LOSE on unrolled bodies: 834 → 2732 ms).
//! 3. **Call-site-driven tuples + static dispatch.** The rep tuple comes from
//!    the `collectors::spec_abi_sites` pre-pass (call sites, not declarations),
//!    and statically-proven sites call the specialized symbol DIRECTLY — no
//!    guard, no diamond, no phi. Declaration-only proofs keep the existing
//!    guarded-diamond shape (`SpecDispatch::Guarded`).
//!
//! Specialized symbols are `internal` and reachable ONLY from same-module
//! direct `FuncRef` call sites — closures, wrappers, exports, and every
//! indirect path resolve to the public boxed symbol (asserted by
//! `spec_abi_symbol_reachability` in the tests module).

use std::collections::HashMap;

pub(crate) use crate::collectors::SpecParamRep;
use crate::types::{LlvmType, DOUBLE, I32, I64};

/// `PERRY_SPECIALIZED_ABI` gate. Default on; `0`/`off`/`false` disables the
/// whole phase (no specialized entries, no static dispatch). Keyed into the
/// object cache (`object_cache.rs`) so A/B arms never share objects.
pub(crate) fn spec_abi_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SPECIALIZED_ABI").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// `PERRY_SPECIALIZED_ABI_MAX`: module-wide cap on emitted specialized
/// entries (anti-bloat budget). Default 64. Also object-cache-keyed.
pub(crate) fn spec_abi_max() -> usize {
    use std::sync::OnceLock;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("PERRY_SPECIALIZED_ABI_MAX")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(64)
    })
}

/// How proven call sites reach the specialized entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecDispatch {
    /// Tier A: every non-Boxed slot proven BY CONSTRUCTION at the call site —
    /// direct `call @{name}__spec_...` with raw args, no guard.
    Static,
    /// Tier B: reps proven only by declared types — the call site keeps the
    /// runtime-guarded diamond (guard → spec entry / boxed fallback → phi).
    Guarded,
}

/// The per-function specialization plan threaded through `CrossModuleCtx`.
#[derive(Debug, Clone)]
pub(crate) struct SpecFnPlan {
    pub reps: Vec<SpecParamRep>,
    pub dispatch: SpecDispatch,
}

impl SpecFnPlan {
    /// Phase-2 budget: exactly ONE specialized entry per function (the
    /// dominant tuple). Kept as an explicit constant so raising it later is a
    /// knob, not a rewrite.
    pub(crate) const MAX_ENTRIES_PER_FUNCTION: usize = 1;
}

/// LLVM parameter type for a rep slot.
pub(crate) fn spec_rep_llvm_ty(rep: SpecParamRep) -> LlvmType {
    match rep {
        SpecParamRep::Boxed | SpecParamRep::F64 => DOUBLE,
        SpecParamRep::I32 => I32,
        SpecParamRep::TaPtr { .. } => I64,
    }
}

/// Specialized-entry symbol: `{public}__spec_<label>_<label>...`. Labels are
/// `[a-z0-9]`-only (`b`, `i32`, `f64`, `ta<kind>[x<len>]`), so the composed
/// name is byte-stable under `sanitize`/`sanitize_member` (no `$`).
pub(crate) fn spec_function_name(public_name: &str, reps: &[SpecParamRep]) -> String {
    let mangle: Vec<String> = reps.iter().map(|r| r.label()).collect();
    format!("{}__spec_{}", public_name, mangle.join("_"))
}

/// Element-kind → class name, for stamping `local_types` inside the
/// specialized body so every type predicate sees the proof.
pub(crate) fn spec_ta_kind_class_name(kind: u8) -> Option<&'static str> {
    use perry_hir::*;
    Some(match kind {
        TYPED_ARRAY_KIND_INT8 => "Int8Array",
        TYPED_ARRAY_KIND_UINT8 => "Uint8Array",
        TYPED_ARRAY_KIND_UINT8_CLAMPED => "Uint8ClampedArray",
        TYPED_ARRAY_KIND_INT16 => "Int16Array",
        TYPED_ARRAY_KIND_UINT16 => "Uint16Array",
        TYPED_ARRAY_KIND_INT32 => "Int32Array",
        TYPED_ARRAY_KIND_UINT32 => "Uint32Array",
        TYPED_ARRAY_KIND_FLOAT32 => "Float32Array",
        TYPED_ARRAY_KIND_FLOAT64 => "Float64Array",
        _ => return None,
    })
}

/// Element kind → (`BufferElem`, width in bytes) for the entry-bound view
/// slot. Mirrors `stmt/let_stmt.rs`'s private table.
pub(crate) fn spec_ta_kind_elem_width(kind: u8) -> Option<(crate::native_value::BufferElem, u32)> {
    use crate::native_value::BufferElem;
    use perry_hir::*;
    Some(match kind {
        TYPED_ARRAY_KIND_INT8 => (BufferElem::I8, 1),
        TYPED_ARRAY_KIND_UINT8 => (BufferElem::U8, 1),
        TYPED_ARRAY_KIND_UINT8_CLAMPED => (BufferElem::U8Clamped, 1),
        TYPED_ARRAY_KIND_INT16 => (BufferElem::I16, 2),
        TYPED_ARRAY_KIND_UINT16 => (BufferElem::U16, 2),
        TYPED_ARRAY_KIND_INT32 => (BufferElem::I32, 4),
        TYPED_ARRAY_KIND_UINT32 => (BufferElem::U32, 4),
        TYPED_ARRAY_KIND_FLOAT32 => (BufferElem::F32, 4),
        TYPED_ARRAY_KIND_FLOAT64 => (BufferElem::F64, 8),
        _ => return None,
    })
}

/// Whether a tuple justifies emitting an entry at all: at least one slot whose
/// specialized form actually removes work (`I32` canonical binding or `TaPtr`
/// proven view). An all-`Boxed`/`F64` tuple is byte-identical to the public
/// entry — pure bloat.
pub(crate) fn spec_tuple_is_viable(reps: &[SpecParamRep]) -> bool {
    reps.iter()
        .any(|r| matches!(r, SpecParamRep::I32 | SpecParamRep::TaPtr { .. }))
}

/// Select the dominant viable tuple for `fid` from the pre-pass judgments,
/// after demoting slots the CALLEE cannot accept in raw form (reassigned or
/// closure-referenced params must keep the boxed protocol). Returns the tuple
/// and how many sites match it exactly.
pub(crate) fn select_dominant_tuple(
    sites: &[Vec<SpecParamRep>],
    param_count: usize,
    demoted_params: &[bool],
) -> Option<(Vec<SpecParamRep>, usize)> {
    let mut counts: HashMap<Vec<SpecParamRep>, usize> = HashMap::new();
    let mut order: Vec<Vec<SpecParamRep>> = Vec::new();
    for site in sites {
        if site.len() != param_count {
            continue;
        }
        let tuple: Vec<SpecParamRep> = site
            .iter()
            .zip(demoted_params.iter())
            .map(|(rep, demoted)| if *demoted { SpecParamRep::Boxed } else { *rep })
            .collect();
        if !spec_tuple_is_viable(&tuple) {
            continue;
        }
        let entry = counts.entry(tuple.clone()).or_insert(0);
        if *entry == 0 {
            order.push(tuple);
        }
        *entry += 1;
    }
    // Dominant = most frequent; ties break by FIRST appearance in the
    // deterministic walk order, keeping the object-cache input stable
    // (`max_by_key` would return the LAST maximal element, so the strictly-
    // greater comparison below is what pins the first appearance).
    let mut best: Option<(Vec<SpecParamRep>, usize)> = None;
    for tuple in order {
        let n = counts.get(&tuple).copied().unwrap_or(0);
        if best.as_ref().is_none_or(|(_, best_n)| n > *best_n) {
            best = Some((tuple, n));
        }
    }
    best
}

/// Declaration-derived tuple for the guarded (Tier B) dispatch: `Int32`-typed
/// params become raw `I32`, everything else stays `Boxed`. `F64`-only tuples
/// are non-viable by construction (`spec_tuple_is_viable`), so this only
/// fires for functions with at least one declared-`Int32` param.
pub(crate) fn declaration_tuple(
    params: &[perry_hir::Param],
    demoted_params: &[bool],
) -> Vec<SpecParamRep> {
    params
        .iter()
        .zip(demoted_params.iter())
        .map(|(p, demoted)| {
            if *demoted {
                return SpecParamRep::Boxed;
            }
            match &p.ty {
                perry_hir::types::Type::Int32 => SpecParamRep::I32,
                _ => SpecParamRep::Boxed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangle_is_sanitize_stable() {
        let reps = [
            SpecParamRep::TaPtr {
                kind: 4,
                const_len: Some(2),
            },
            SpecParamRep::I32,
            SpecParamRep::TaPtr {
                kind: 4,
                const_len: None,
            },
            SpecParamRep::Boxed,
            SpecParamRep::F64,
        ];
        let name = spec_function_name("perry_fn_m___encipher", &reps);
        assert_eq!(name, "perry_fn_m___encipher__spec_ta4x2_i32_ta4_b_f64");
        // Every char is symbol-safe: sanitize/sanitize_member must return the
        // name byte-identical (no `$`, no unicode).
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn dominant_tuple_selection_demotes_and_counts() {
        let ta = SpecParamRep::TaPtr {
            kind: 4,
            const_len: Some(8),
        };
        let sites = vec![
            vec![ta, SpecParamRep::I32],
            vec![ta, SpecParamRep::I32],
            vec![SpecParamRep::Boxed, SpecParamRep::I32],
        ];
        let (tuple, n) = select_dominant_tuple(&sites, 2, &[false, false]).unwrap();
        assert_eq!(tuple, vec![ta, SpecParamRep::I32]);
        assert_eq!(n, 2);

        // Demoting param 0 collapses all sites to (Boxed, I32) — still viable
        // through the I32 slot.
        let (tuple, n) = select_dominant_tuple(&sites, 2, &[true, false]).unwrap();
        assert_eq!(tuple, vec![SpecParamRep::Boxed, SpecParamRep::I32]);
        assert_eq!(n, 3);

        // Arity-mismatched sites never count.
        assert!(select_dominant_tuple(&sites, 3, &[false, false, false]).is_none());
    }

    /// The reachability ratchet: specialized symbols must NEVER be constructed
    /// outside the allowlisted modules (naming + emission + direct-call
    /// dispatch). A `__spec_` string appearing in a closure/wrapper emission
    /// path would mean an indirect route to a raw-ABI entry — unsound by
    /// construction. Scans this crate's sources at test time.
    #[test]
    fn spec_abi_symbol_reachability() {
        let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed: [&str; 4] = [
            "codegen/spec_abi.rs",    // naming + this test
            "codegen/function.rs",    // entry emission
            "codegen/mod.rs",         // eligibility/budget loop
            "lower_call/func_ref.rs", // direct-call dispatch
        ];
        let mut offenders: Vec<String> = Vec::new();
        fn visit(
            dir: &std::path::Path,
            root: &std::path::Path,
            allowed: &[&str],
            out: &mut Vec<String>,
        ) {
            for entry in std::fs::read_dir(dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    visit(&path, root, allowed, out);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if allowed.contains(&rel.as_str()) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("read source file");
                if text.contains("__spec_") {
                    out.push(rel);
                }
            }
        }
        visit(&src_root, &src_root, &allowed, &mut offenders);
        assert!(
            offenders.is_empty(),
            "specialized-ABI symbol fragments found outside the allowlist \
             (closure/wrapper paths must only ever call the public boxed \
             symbol): {offenders:?}"
        );
    }
}
