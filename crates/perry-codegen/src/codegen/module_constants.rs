//! Module-level constants and return facts computed once, before the
//! dispatch tables: the target's `__platform__` number, the compile-time
//! constant locals and the class each factory function returns.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::helpers::collect_return_class;

/// Derive the `__platform__` number from the target triple:
///   0 = macOS, 1 = iOS, 2 = Android, 3 = Windows, 4 = Linux,
///   5 = Web, 6 = tvOS, 7 = watchOS, 8 = visionOS, 9 = HarmonyOS
pub(super) fn platform_number(triple: &str) -> f64 {
    let t = triple.to_lowercase();
    // HarmonyOS check must precede the plain `linux` arm: the OHOS triple is
    // `*-unknown-linux-ohos`, so a naive `contains("linux")` would classify it as 4.
    if t.contains("ohos") {
        9.0
    } else if t.contains("visionos") || t.contains("xros") {
        8.0
    } else if t.contains("watchos") {
        7.0
    } else if t.contains("ios") {
        1.0
    } else if t.contains("tvos") {
        6.0
    } else if t.contains("android") {
        2.0
    } else if t.contains("windows") || t.contains("mingw") || t.contains("msvc") {
        3.0
    } else if t.contains("linux") {
        4.0
    } else if t.contains("wasm") || t.contains("emscripten") {
        5.0
    } else {
        0.0
    } // macOS / darwin default
}

pub(super) fn compile_time_constants(
    logical_entry_stmts: &[&perry_hir::Stmt],
    platform_number: f64,
) -> HashMap<u32, f64> {
    // Pre-scan hir.init for compile-time constant variables. These are
    // `declare const __platform__: number` / `declare const __plugins__: number`
    // that other backends (JS, WASM) inject at build time. The LLVM backend
    // uses these to constant-fold platform checks in `lower_if`, eliminating
    // dead branches that reference extern FFI functions absent on the target.
    let mut compile_time_constants: HashMap<u32, f64> = HashMap::new();
    for s in logical_entry_stmts.iter().copied() {
        if let perry_hir::Stmt::Let {
            id,
            name,
            init: None,
            ..
        } = s
        {
            match name.as_str() {
                "__platform__" => {
                    compile_time_constants.insert(*id, platform_number);
                }
                "__plugins__" => {
                    compile_time_constants.insert(*id, 0.0);
                }
                _ => {}
            }
        }
    }
    // Representation-selection Phase 2: top-level `const <name> = <numeric
    // literal>` module bindings are compile-time constants by ECMAScript
    // semantics (a `const` reassignment is a parse-time error), so their
    // reads constant-fold — this is what proves `P[BLOWFISH_NUM_ROUNDS + 1]`
    // in-bounds against a constant-length view. Excluded: any id carried by a
    // `PreallocateBoxes`/`PreallocateTdzBoxes` statement — a box-backed slot
    // holds a box pointer, and a TDZ-flagged binding must keep its
    // ReferenceError on pre-declaration reads instead of folding to a value.
    {
        let mut prealloc_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for s in logical_entry_stmts.iter().copied() {
            if let perry_hir::Stmt::PreallocateBoxes(ids)
            | perry_hir::Stmt::PreallocateTdzBoxes(ids) = s
            {
                prealloc_ids.extend(ids.iter().copied());
            }
        }
        for s in logical_entry_stmts.iter().copied() {
            if let perry_hir::Stmt::Let {
                id,
                mutable: false,
                init: Some(init),
                ..
            } = s
            {
                if prealloc_ids.contains(id) {
                    continue;
                }
                let value = match init {
                    perry_hir::Expr::Integer(n) => Some(*n as f64),
                    perry_hir::Expr::Number(n) if n.is_finite() => Some(*n),
                    _ => None,
                };
                if let Some(value) = value {
                    compile_time_constants.entry(*id).or_insert(value);
                }
            }
        }
    }
    compile_time_constants
}

/// Refs #915 (gap 3 / #321 follow-up): tag functions whose body
/// unconditionally returns a `ClassRef` (or transitively returns
/// another such factory) so call sites of the form
/// `Literal(value).pipe(...)` can dispatch the `.pipe` lookup as a
/// static-method call on the returned class. Iterate until
/// fixed-point so `Literal(value)` (which calls `makeLiteralClass`)
/// resolves to the same class as `makeLiteralClass(...)`.
pub(super) fn func_returns_class_map(hir: &HirModule) -> std::collections::HashMap<u32, String> {
    let mut func_returns_class_map: std::collections::HashMap<u32, String> =
        std::collections::HashMap::new();
    let n_funcs_for_factory_pass = hir.functions.len();
    for _ in 0..n_funcs_for_factory_pass {
        let mut changed = false;
        for f in &hir.functions {
            if func_returns_class_map.contains_key(&f.id) {
                continue;
            }
            let mut produced: Option<String> = None;
            let mut disqualified = false;
            collect_return_class(
                &f.body,
                &mut produced,
                &mut disqualified,
                &func_returns_class_map,
            );
            if !disqualified {
                if let Some(class_name) = produced {
                    func_returns_class_map.insert(f.id, class_name);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    func_returns_class_map
}
