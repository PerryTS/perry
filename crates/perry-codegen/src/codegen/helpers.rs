//! Small standalone helpers used by `compile_module`, the per-function
//! lowering helpers, and other modules in the crate.
//!
//! Split out of `codegen.rs` (now `codegen/mod.rs`). Names, behavior, and
//! visibility are unchanged — every function is re-exported from
//! `crate::codegen` as needed so external callers don't notice.

use std::collections::HashMap;

use anyhow::Result;
use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::opts::{NamespaceEntry, NamespaceEntryKind};

pub(crate) fn function_body_returns_generator_object(body: &[perry_hir::Stmt]) -> bool {
    let has_gen_state = body
        .iter()
        .any(|stmt| matches!(stmt, perry_hir::Stmt::Let { name, .. } if name == "__gen_state"));
    if !has_gen_state {
        return false;
    }
    body.iter().any(|stmt| match stmt {
        // The generator transform may wrap the returned iterator in the
        // instance-prototype linker; unwrap it so the iterator shape remains
        // the stable signal that this is a lowered generator wrapper.
        perry_hir::Stmt::Return(Some(expr)) => {
            let inner = match expr {
                perry_hir::Expr::LinkGeneratorPrototype { obj, .. } => obj.as_ref(),
                other => other,
            };
            matches!(inner, perry_hir::Expr::Object(props)
                if props.len() == 3
                    && props[0].0 == "next"
                    && props[1].0 == "return"
                    && props[2].0 == "throw"
                    && props
                        .iter()
                        .all(|(_, value)| matches!(value, perry_hir::Expr::Closure { .. })))
        }
        _ => false,
    })
}

/// Compile a single user function into the module.
/// Shadow-stack push/pop + slot-set emission for every user
/// function. Default ON as of Phase D part 2 (v0.5.238); set
/// `PERRY_SHADOW_STACK=0`/`off`/`false` to disable for bisection.
/// Cached at first call so subsequent compile_* calls skip the
/// env-var lookup.
///
/// Why on by default now: the shadow stack precisely covers every
/// pointer-typed local in compiled JS frames, complementing the
/// conservative C-stack scan. With Phase A complete and the GC
/// tracer consuming the shadow stack as a parallel root source
/// (v0.5.221), enabling it is a strict-improvement default —
/// fewer over-promoted objects in generational mode, no change
/// in observed correctness, modest per-function-entry overhead
/// (one frame_push call + N slot stores at safepoints) that's
/// invisible on every measured benchmark. Phase D part 2 then
/// uses the shadow stack's authoritative JS-frame coverage to
/// shrink the conservative scanner — which only makes sense once
/// the shadow stack is guaranteed to be live.
pub(super) fn shadow_stack_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SHADOW_STACK").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

pub(super) fn enable_module_init_shadow_frame(
    func: &mut crate::function::LlFunction,
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &std::collections::HashSet<u32>,
) -> (HashMap<u32, u32>, HashMap<usize, Vec<u32>>) {
    if !shadow_stack_enabled() {
        return (HashMap::new(), HashMap::new());
    }

    let shadow_slot_map =
        crate::collectors::collect_pointer_typed_locals(&[], stmts, flat_const_ids);
    func.enable_post_init_shadow_frame(shadow_slot_map.len() as u32);
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(stmts, &shadow_slot_map);
    (shadow_slot_map, shadow_slot_clears_after_stmt)
}

/// Gen-GC write-barrier emission gate. Default ON: emit a
/// `js_write_barrier_slot(parent_bits, slot_addr, child_bits)` call, or
/// the compatibility wrapper, after every heap-store site. Set
/// `PERRY_WRITE_BARRIERS=0`/`off`/`false` to disable emission for
/// benchmark/debug bisection. `=1`/`on`/`true` remain accepted and
/// equivalent to the default.
pub(crate) fn write_barriers_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

thread_local! {
    static FULL_OUTLINE_IC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Lever B (#5334) full-outline gate for class-field IC diamonds. Set ONCE per
/// module at the top of `compile_module` (see [`decide_full_outline_ic`]), read
/// at each class-field-set lowering. Thread-local — NOT a process-global
/// `OnceLock` — because codegen runs one module per `compile_module` call and a
/// process-global would wrongly pin the first module's decision for the rest of
/// a multi-module build (and across tests). Codegen within a module is
/// sequential, so a thread-local is safe and avoids threading a flag through all
/// six `FnCtx` construction sites.
pub(crate) fn full_outline_ic_enabled() -> bool {
    FULL_OUTLINE_IC.with(|c| c.get())
}

pub(crate) fn set_full_outline_ic(enabled: bool) {
    FULL_OUTLINE_IC.with(|c| c.set(enabled));
}

/// Total number of LLVM functions a module will emit — top-level functions
/// plus every class callable (constructor, instance/static methods, computed
/// members, accessor get/set bodies). Used as the lever-B size proxy: class
/// methods and closures
/// do NOT live in `hir.functions`, so a class-heavy minified bundle (the exact
/// pathology lever B targets) can have a small `functions.len()` yet emit tens
/// of thousands of LLVM functions. Counting class callables keeps the gate from
/// silently under-counting and never firing on those modules.
pub(crate) fn module_callable_count(hir: &perry_hir::Module) -> usize {
    let class_callables: usize = hir
        .classes
        .iter()
        .map(|c| {
            usize::from(c.constructor.is_some())
                + c.methods.len()
                + c.static_methods.len()
                + c.computed_members.len()
                + c.getters.len()
                + c.setters.len()
        })
        .sum();
    hir.functions.len() + class_callables
}

/// Decide whether a module is large enough to warrant full-outlining its
/// class-field IC diamonds (#5334 lever B). Oversized modules are forced to
/// `clang -O0` (#4880), where the inline diamond's ~15-line-per-site expansion
/// is never optimized away; collapsing each site to one
/// `call @js_class_field_set_ic(...)` keeps the IR small enough for clang to
/// compile at all. Gated on the module's total callable count (see
/// [`module_callable_count`]) — the defining trait of the pathological
/// minified-bundle case (tens of thousands of callables in one module);
/// ordinary per-file modules stay on the inline diamond and keep the hot fast
/// store.
///
/// `PERRY_FULL_OUTLINE_IC=1`/`on`/`true` forces ON, `=0`/`off`/`false` forces
/// OFF; otherwise auto: `callable_count >= PERRY_FULL_OUTLINE_IC_MIN_FUNCS`
/// (default 4000).
pub(crate) fn decide_full_outline_ic(callable_count: usize) -> bool {
    match std::env::var("PERRY_FULL_OUTLINE_IC").as_deref() {
        Ok("1") | Ok("on") | Ok("true") => return true,
        Ok("0") | Ok("off") | Ok("false") => return false,
        _ => {}
    }
    let threshold = std::env::var("PERRY_FULL_OUTLINE_IC_MIN_FUNCS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4000);
    callable_count >= threshold
}

/// Decide how many codegen units to split a module's object compilation into
/// (#5391). A single huge translation unit makes `clang -c` OOM (~15GB on the
/// 13MB bundle); splitting bounds peak compiler memory to roughly whole/N.
///
/// `PERRY_CODEGEN_UNITS=N` forces exactly N units (1 disables splitting).
/// Otherwise auto: 1 unit until the module's callable count crosses a floor,
/// then `ceil(callables / target_per_unit)`, capped — so ordinary per-file
/// modules stay on the single-unit path (default 1, zero behavior change).
/// `PERRY_CODEGEN_UNIT_SIZE` overrides the target callables-per-unit.
pub(crate) fn decide_codegen_units(callable_count: usize) -> usize {
    if let Ok(v) = std::env::var("PERRY_CODEGEN_UNITS") {
        if let Ok(n) = v.parse::<usize>() {
            return n.max(1);
        }
    }
    const MIN_CALLABLES_TO_SPLIT: usize = 8000;
    const MAX_UNITS: usize = 48;
    let target = std::env::var("PERRY_CODEGEN_UNIT_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(6000);
    if callable_count < MIN_CALLABLES_TO_SPLIT {
        return 1;
    }
    callable_count.div_ceil(target).clamp(1, MAX_UNITS)
}

pub(super) fn scoped_fn_name(module_prefix: &str, hir_name: &str) -> String {
    // Use the INJECTIVE sanitizer (same as scoped_static_method_name): plain
    // `sanitize` maps every non-`[A-Za-z0-9_]` char to `_`, so distinct minified
    // function names like `$Z5` and `_Z5` both became `perry_fn_<mod>___Z5` and
    // clang rejected the module with "invalid redefinition of function". `func_names`
    // is keyed by func id and every reference resolves through it, so changing the
    // mangling here keeps all local-function call sites consistent. Byte-identical
    // to `sanitize` for plain `[A-Za-z0-9_]` names (the overwhelming common case).
    format!("perry_fn_{}__{}", module_prefix, sanitize_member(hir_name))
}

pub(super) fn scoped_static_method_name(
    module_prefix: &str,
    class_id: u32,
    class_name: &str,
    method_name: &str,
) -> String {
    format!(
        "perry_static_{}__{}__c{}__{}",
        module_prefix,
        sanitize_member(class_name),
        class_id,
        sanitize_member(method_name)
    )
}

/// Walk a function body looking for `Return(Some(expr))` shapes that
/// identify the function as a factory returning a class. Sets
/// `*produced` to the resolved class name when the first qualifying
/// return is seen; sets `*disqualified` when a return points at
/// something we can't classify as a class. Used by
/// `func_returns_class_map` fixed-point in `compile_module` to recognise
/// Effect's `Literal` / `makeLiteralClass` / `make` factories. Refs
/// #915 (gap 3 / #321 follow-up).
///
/// Recognised return shapes:
///   - `Return(Some(ClassRef(name)))` — direct class literal return.
///   - `Return(Some(Call { callee: FuncRef(other_fid), .. }))` — call
///     to another already-tagged factory (transitive).
///   - `Return(Some(Conditional { then, else, .. }))` — both branches
///     must independently resolve to the same class. Effect's
///     `Literal(...)` has this shape — the body is
///     `array_.isNonEmptyReadonlyArray(literals) ? makeLiteralClass(literals) : Never`.
///   - `Return(Some(Sequence([..., ClassRef(name)])))` — the HIR's
///     inliner sometimes collapses a factory call to
///     `Sequence([RegisterClassParentDynamic, ClassRef(name)])`. Treat
///     the trailing class as the produced value.
///
/// Anything else inside a `Return(Some(_))` disqualifies the function:
/// we'd rather miss a factory than mis-classify a non-factory.
/// Returns inside nested closures are SKIPPED — those belong to the
/// inner function (the walker doesn't recurse into Expr).
pub(super) fn collect_return_class(
    stmts: &[perry_hir::Stmt],
    produced: &mut Option<String>,
    disqualified: &mut bool,
    func_returns_class: &std::collections::HashMap<u32, String>,
) {
    use perry_hir::{Expr, Stmt};

    fn resolve_class(
        expr: &perry_hir::Expr,
        func_returns_class: &std::collections::HashMap<u32, String>,
    ) -> Option<String> {
        match expr {
            Expr::ClassRef(name) => Some(name.clone()),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
                _ => None,
            },
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let lhs = resolve_class(then_expr, func_returns_class)?;
                let rhs = resolve_class(else_expr, func_returns_class)?;
                if lhs == rhs {
                    Some(lhs)
                } else {
                    None
                }
            }
            Expr::Sequence(exprs) => exprs
                .last()
                .and_then(|e| resolve_class(e, func_returns_class)),
            _ => None,
        }
    }

    for stmt in stmts {
        if *disqualified {
            return;
        }
        match stmt {
            Stmt::Return(Some(expr)) => {
                let resolved = resolve_class(expr, func_returns_class);
                match resolved {
                    Some(name) => match produced {
                        None => *produced = Some(name),
                        Some(existing) if *existing == name => {}
                        Some(_) => {
                            // Mixed return shapes — bail.
                            *disqualified = true;
                        }
                    },
                    None => {
                        *disqualified = true;
                    }
                }
            }
            Stmt::Return(None) => {
                // Returning undefined — disqualify (caller can't
                // depend on the receiver being a class).
                *disqualified = true;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_class(then_branch, produced, disqualified, func_returns_class);
                if let Some(eb) = else_branch {
                    collect_return_class(eb, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_return_class(body, produced, disqualified, func_returns_class);
                if let Some(cc) = catch {
                    collect_return_class(&cc.body, produced, disqualified, func_returns_class);
                }
                if let Some(blk) = finally {
                    collect_return_class(blk, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_return_class(&case.body, produced, disqualified, func_returns_class);
                }
            }
            Stmt::Labeled { body, .. } => {
                let slice = std::slice::from_ref(body.as_ref());
                collect_return_class(slice, produced, disqualified, func_returns_class);
            }
            _ => {}
        }
    }
}

/// Mangle a class method name into an LLVM symbol, scoped by module
/// prefix and class name.
///
/// `perry_method_<modprefix>__<class>__<method>`.
pub(super) fn scoped_method_name(
    module_prefix: &str,
    class_name: &str,
    method_name: &str,
) -> String {
    format!(
        "perry_method_{}__{}__{}",
        module_prefix,
        sanitize_member(class_name),
        sanitize_member(method_name)
    )
}

/// Sanitize a name for use in an LLVM symbol — replace anything that isn't
/// `[A-Za-z0-9_]` with an underscore. LLVM IR identifiers cannot start with
/// a digit, so prefix with `_` if the first character would be one (this
/// happens with module names like `05_fibonacci.ts`).
///
/// NOTE: this mapping is *lossy* — every special character collapses to `_`,
/// so distinct inputs can share an output. That is fine for the module-prefix
/// and static-field components (whose values are recorded once and re-derived
/// identically at every reference site), but NOT for class/method name
/// components, where distinct private names like `#$`, `#_`, `#℘` would all
/// collapse to the same `perry_method_…` symbol and clang would reject the
/// module with `invalid redefinition of function`. Those components use the
/// injective [`sanitize_member`] instead. Keep `sanitize` byte-for-byte stable:
/// changing it desyncs cross-module symbol references (a module's prefix is
/// `sanitize(module_name)` at the definition site and must match the prefix the
/// importing module re-derives).
pub(super) fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        s.insert(0, '_');
    }
    s
}

/// Injective variant of [`sanitize`] for the class-name and method-name
/// components of `perry_method_*` / `perry_static_*` symbols.
///
/// Names made up entirely of `[A-Za-z0-9_]` are returned IDENTICAL to what
/// `sanitize` produces (only a leading digit is `_`-prefixed), so every
/// ordinary method/class symbol is byte-for-byte unchanged. Names containing
/// any character outside `[A-Za-z0-9_]` — chiefly private member names (`#$`,
/// `#℘`, `#\u{6F}`, ZWJ/ZWNJ escapes) — are escaped to an unambiguous form
/// (`u_` tag + `_<hex>_` per non-alphanumeric character) so distinct source
/// names always yield distinct symbols. `sanitize` collapsed all of these to a
/// single `_`, so `#$`, `#_` and `#℘` mangled to the same symbol and clang
/// rejected the module with `invalid redefinition of function`.
///
/// Must be applied at BOTH the definition site and every reference site for a
/// given symbol component, or the symbols desync and the linker fails.
pub(super) fn sanitize_member(name: &str) -> String {
    let is_plain = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_plain {
        // Byte-identical to `sanitize` for plain names (incl. leading-digit fix).
        return sanitize(name);
    }
    // A plain (pure-`[A-Za-z0-9_]`) name never reaches this branch, so it can
    // never collide with an escaped name: every escaped name carries a
    // `_<hex>_` group a plain name cannot reproduce.
    let mut s = String::from("u_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else {
            s.push('_');
            s.push_str(&format!("{:x}", c as u32));
            s.push('_');
        }
    }
    s
}

/// Host default triple.
/// Host-default LLVM target triple. Used when `CompileOptions.target`
/// is `None`. Also re-exposed via `pub(crate)` so `linker.rs` can pin
/// clang's `-target` even on host builds — without that pin a clang
/// whose own default triple is GNU/MinGW silently overrides the IR's
/// stated msvc triple and emits a `__main` libgcc reference that
/// lld-link/link.exe can't resolve. (The bug used to surface as
/// `LNK2019: unresolved external symbol __main referenced in
/// function main` even though the .ll says `target triple =
/// "x86_64-pc-windows-msvc"`.)
pub(crate) fn default_target_triple() -> String {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-macosx15.0.0".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu".to_string()
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu".to_string()
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc".to_string()
    } else {
        "arm64-apple-macosx15.0.0".to_string()
    }
}

/// Map a Perry `--target <name>` string to the LLVM triple used by
/// `clang -target <triple>` / `llc -mtriple=<triple>`. The short
/// names are the public `--target` surface exposed by the CLI;
/// returning `None` leaves the triple to the host default.
///
/// Supported:
///  * `ios`, `ios-simulator`           → aarch64-apple-ios
///  * `visionos`, `visionos-simulator` → arm64-apple-xros1.0{,-simulator}
///  * `watchos`                        → aarch64-apple-watchos (arm64, S9+ / watchOS 26)
///  * `watchos-simulator`              → arm64-apple-watchos10.0-simulator
///  * `tvos`, `tvos-simulator`         → aarch64-apple-tvos
///  * `android`                        → aarch64-unknown-linux-android
///  * `linux` (x86_64 alias)           → x86_64-unknown-linux-gnu
///  * `linux-aarch64`                  → aarch64-unknown-linux-gnu
///  * `linux-musl` (x86_64 alias)      → x86_64-unknown-linux-musl (fully static)
///  * `linux-aarch64-musl`             → aarch64-unknown-linux-musl (fully static)
///  * `macos` (aarch64 alias)          → arm64-apple-macosx15.0.0
///  * `macos-x86_64`                   → x86_64-apple-macosx15.0.0
///  * `windows`                        → x86_64-pc-windows-msvc
///  * anything else                    → None (use host default)
pub fn resolve_target_triple(name: &str) -> Option<String> {
    match name {
        "ios" => Some("aarch64-apple-ios".to_string()),
        "ios-simulator" => Some("arm64-apple-ios17.0-simulator".to_string()),
        "visionos" => Some("arm64-apple-xros1.0".to_string()),
        "visionos-simulator" => Some("arm64-apple-xros1.0-simulator".to_string()),
        // arm64_32 (Series 4-8 / SE) when opted in via PERRY_WATCHOS_ARM64_32;
        // otherwise arm64 (S9+). Sets the arch of the emitted TS object files,
        // which must match the runtime/native-lib/link triples.
        "watchos" if std::env::var("PERRY_WATCHOS_ARM64_32").is_ok() => {
            Some("arm64_32-apple-watchos".to_string())
        }
        "watchos" => Some("aarch64-apple-watchos".to_string()),
        "watchos-simulator" => Some("arm64-apple-watchos10.0-simulator".to_string()),
        "tvos" => Some("aarch64-apple-tvos".to_string()),
        "tvos-simulator" => Some("arm64-apple-tvos17.0-simulator".to_string()),
        "harmonyos" => Some("aarch64-unknown-linux-ohos".to_string()),
        "harmonyos-simulator" => Some("x86_64-unknown-linux-ohos".to_string()),
        "android" => Some("aarch64-unknown-linux-android".to_string()),
        // #5742: Android x86_64 (emulator / x86_64 devices). Without this arm
        // `resolve_target_triple` returned None for `--target android-x86_64`,
        // so codegen fell back to the host triple (e.g. x86_64-pc-windows-msvc)
        // and emitted COFF objects, which the Android linker rejects with
        // `unknown file type`. Mirrors the `-unknown-` LLVM-triple form of the
        // arm64 `android` arm above (cf. harmonyos / harmonyos-simulator).
        "android-x86_64" => Some("x86_64-unknown-linux-android".to_string()),
        // Wear OS is Android-on-a-watch: same arm64 Android object format.
        "wearos" => Some("aarch64-unknown-linux-android".to_string()),
        "linux" => Some("x86_64-unknown-linux-gnu".to_string()),
        "linux-aarch64" => Some("aarch64-unknown-linux-gnu".to_string()),
        // musl targets — fully static binaries that run on Lambda
        // provided.al2023, scratch/distroless containers, Cloud Run, etc.
        // (no glibc loader dependency). See link/platform_cmd.rs for the
        // `-static` musl link path and #4826.
        "linux-musl" | "linux-x86_64-musl" => Some("x86_64-unknown-linux-musl".to_string()),
        "linux-aarch64-musl" => Some("aarch64-unknown-linux-musl".to_string()),
        "macos" => Some("arm64-apple-macosx15.0.0".to_string()),
        "macos-x86_64" => Some("x86_64-apple-macosx15.0.0".to_string()),
        "windows" | "windows-winui" => Some("x86_64-pc-windows-msvc".to_string()),
        _ => None,
    }
}

/// True for macOS triples only (`*-apple-macosx*` LLVM-style, or
/// `*-apple-darwin*` rustc-style when a raw triple is passed through).
/// Deliberately false for every other Apple platform (`apple-ios`,
/// `apple-tvos`, `apple-xros`, `apple-watchos`): the `.app` CWD fix in
/// `perry_macos_bundle_chdir` is macOS-only, and emitting the call on
/// non-macOS targets makes their links depend on the runtime archive
/// carrying a macOS-only symbol (#4856).
pub(super) fn is_macos_triple(triple: &str) -> bool {
    triple.contains("apple-macosx") || triple.contains("apple-darwin")
}

pub(super) fn emit_buffer_alias_metadata(llmod: &mut LlModule, count: u32) {
    if count == 0 {
        return;
    }
    // Shared domain.
    llmod.add_metadata_line("!100 = distinct !{!100}".to_string());
    // Per-buffer scope nodes.
    for i in 0..count {
        let sid = 101 + i;
        llmod.add_metadata_line(format!("!{} = distinct !{{!{}, !100}}", sid, sid));
    }
    // Single-element alias-scope lists (one per buffer).
    for i in 0..count {
        let list_id = 201 + i;
        let scope_id = 101 + i;
        llmod.add_metadata_line(format!("!{} = !{{!{}}}", list_id, scope_id));
    }
    // Noalias lists: for buffer i, every *other* buffer's scope.
    for i in 0..count {
        let list_id = 301 + i;
        let others: Vec<String> = (0..count)
            .filter(|j| *j != i)
            .map(|j| format!("!{}", 101 + j))
            .collect();
        if others.is_empty() {
            // Single buffer: empty noalias set — LLVM accepts `!{}` but
            // it's a no-op. Still emit so `!noalias !{N}` references resolve.
            llmod.add_metadata_line(format!("!{} = !{{}}", list_id));
        } else {
            llmod.add_metadata_line(format!("!{} = !{{{}}}", list_id, others.join(", ")));
        }
    }
}

pub(super) fn register_module_globals_as_gc_roots(
    ctx: &mut crate::expr::FnCtx<'_>,
    module_globals: &HashMap<u32, String>,
) {
    // Sort by id for deterministic emit order (helps with diff-testing
    // the generated IR and matches the existing `class_keys` pattern).
    let mut entries: Vec<(&u32, &String)> = module_globals.iter().collect();
    entries.sort_by_key(|(id, _)| **id);
    for (_, global_name) in entries {
        let addr = ctx.block().ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }

    // Static class-field globals (`@perry_static_<mod>__<Class>__<field>`)
    // hold NaN-boxed any-values just like module globals but were never
    // registered (2026-07-02 audit P0 — byte-for-byte the #5042 class-keys
    // shape): the global's copy was never rewritten on evacuation, and when
    // the write-site class-id lookup misses it is the ONLY reference, so the
    // value was collectable while `C.field` still read it. The map can hold
    // one global under several keys (class name + import alias) — dedupe.
    // Cross-module duplicates (defining module + importers both register the
    // same linked address) are harmless: GLOBAL_ROOTS tolerates duplicate
    // entries (double mark/rewrite is idempotent).
    let static_names: std::collections::BTreeSet<&String> =
        ctx.static_field_globals.values().collect();
    for global_name in static_names {
        let addr = ctx.block().ptrtoint(&format!("@{}", global_name), I64);
        ctx.block()
            .call_void("js_gc_register_global_root", &[(I64, &addr)]);
    }
}

/// Early static-field setup: registrations that don't read any
/// module-level binding's value (Error-extending classes, well-known
/// symbol method hooks). Safe to emit before `stmt::lower_stmts` —
/// values referenced are either compile-time constants (class ids,
/// function pointers) or computed entirely from `hir` metadata.
///
/// The split (early vs. late) was introduced for issue #894 (effect's
/// `make()` factory's `static [TypeId] = variance` — both the key and
/// the init reference module-level lets that haven't been initialized
/// at the point the old combined `init_static_fields` ran).
pub(super) fn init_static_fields_early(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
    // Phase C.3: register user classes that extend the built-in Error
    // (or any of its subclasses) with the runtime, so `instanceof Error`
    // walks the chain and returns true. Without this, `new HttpError(...)
    // instanceof Error` returns false because the runtime's
    // `EXTENDS_ERROR_REGISTRY` is empty for user classes.
    for c in &hir.classes {
        // Walk this class's extends_name chain; if any ancestor is a
        // built-in error subclass, register this class's id.
        let mut cur: Option<String> = c.extends_name.clone();
        let mut extends_error = false;
        let mut extends_data_view = false;
        let mut depth = 0usize;
        while let Some(name) = cur {
            if matches!(
                name.as_str(),
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "AggregateError"
            ) {
                extends_error = true;
                break;
            }
            if name == "DataView" {
                extends_data_view = true;
                break;
            }
            // Walk user-defined ancestor chain.
            if let Some(parent) = ctx.classes.get(&name) {
                cur = parent.extends_name.clone();
                depth += 1;
                if depth > 32 {
                    break;
                }
            } else {
                cur = None;
            }
        }
        if extends_error {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                let cid_str = cid.to_string();
                ctx.block().call_void(
                    "js_register_class_extends_error",
                    &[(crate::types::I32, &cid_str)],
                );
            }
        }
        if extends_data_view {
            if let Some(&cid) = ctx.class_ids.get(&c.name) {
                let cid_str = cid.to_string();
                ctx.block().call_void(
                    "js_register_class_extends_data_view",
                    &[(crate::types::I32, &cid_str)],
                );
            }
        }
    }
    // Well-known symbol class hooks: HIR lifts `static [Symbol.hasInstance]`
    // and `get [Symbol.toStringTag]` to top-level functions with the
    // prefixes `__perry_wk_hasinstance_<class>` / `__perry_wk_tostringtag_<class>`.
    // Scan `hir.functions`, compute the LLVM symbol via `scoped_fn_name`,
    // and emit `js_register_class_<hook>(class_id, ptrtoint(@func, i64))`
    // at module init so the runtime's `js_instanceof` / `js_object_to_string`
    // can dispatch through them.
    let module_prefix = ctx.strings.module_prefix().to_string();
    for f in &hir.functions {
        let (registrar, class_name): (&str, &str) =
            if let Some(rest) = f.name.strip_prefix("__perry_wk_hasinstance_") {
                ("js_register_class_has_instance", rest)
            } else if let Some(rest) = f.name.strip_prefix("__perry_wk_tostringtag_") {
                ("js_register_class_to_string_tag", rest)
            } else {
                continue;
            };
        let Some(&cid) = ctx.class_ids.get(class_name) else {
            continue;
        };
        let cid_str = cid.to_string();
        let llvm_sym = format!("perry_fn_{}__{}", module_prefix, sanitize(&f.name));
        let func_ref = format!("@{}", llvm_sym);
        let blk = ctx.block();
        let func_ptr_i64 = blk.ptrtoint(&func_ref, I64);
        blk.call_void(
            registrar,
            &[(crate::types::I32, &cid_str), (I64, &func_ptr_i64)],
        );
    }
    // Uninitialized, non-computed static fields (`static foo;`, `static "g";`,
    // `static 0;`) are own data properties of the constructor with value
    // `undefined` per ClassDefinitionEvaluation. Their value is a compile-time
    // constant (`undefined`) with no dependency on user lets, and a class name
    // is in TDZ before its declaration, so registering them here — before user
    // code — is observably identical to registering at the class-decl position
    // and strictly earlier than the `init_static_fields_late` fallback that
    // previously handled them (which ran AFTER user statements, so
    // `Object.keys(C)` / `getOwnPropertyDescriptor(C, "foo")` immediately after
    // the declaration saw nothing). test262 class/elements static-as-valid-
    // static-field & friends. Initialized and computed-key fields are emitted
    // inline at their source position elsewhere and are skipped here.
    for c in &hir.classes {
        // Next.js wall 54: a nested class (declared inside a function) has its
        // static-field initializers run when the enclosing function evaluates
        // the class, NOT at module init — skip them here.
        if c.is_nested {
            continue;
        }
        let Some(&class_id) = ctx.class_ids.get(&c.name) else {
            continue;
        };
        if class_id == 0 {
            continue;
        }
        for sf in &c.static_fields {
            if sf.key_expr.is_some() || sf.init.is_some() || sf.name.starts_with('#') {
                continue;
            }
            let idx = ctx.strings.intern(&sf.name);
            let entry = ctx.strings.entry(idx);
            let bytes_ref = format!("@{}", entry.bytes_global);
            let len_str = entry.byte_len.to_string();
            let cid_str = class_id.to_string();
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            ctx.block().call_void(
                "js_class_register_static_field",
                &[
                    (crate::types::I32, &cid_str),
                    (crate::types::PTR, &bytes_ref),
                    (crate::types::I64, &len_str),
                    (DOUBLE, &undef),
                ],
            );
        }
    }
    Ok(())
}

/// Late static-field setup: per-class static-field initializer evaluation,
/// computed-Symbol-key registration, and static-block invocation. Must
/// run AFTER `stmt::lower_stmts` so module-level lets referenced by
/// these initializers (e.g. `static [TypeId] = variance` where both
/// `TypeId` and `variance` are top-level `const`s) read their populated
/// global slots rather than the zero default.
///
/// Issue #894: effect's `function make(ast) { return class { static
/// [TypeId] = variance } }` factory pattern hit this; the `TypeId`
/// symbol and `variance` value were both top-level module lets, and
/// the pre-#894 combined `init_static_fields` ran before user init,
/// so `js_class_register_static_symbol(class_id, 0.0, 0.0)` registered
/// nothing reachable. `isSchema(C)` then returned false on a class
/// returned from `make`, dual()'s predicate failed, and the failing
/// `.annotations({...})` chain eventually fed `undefined` to a `make`
/// call that read `ast._tag` → `TypeError: Cannot read properties of
/// undefined (reading '_tag')` during Schema.ts module init.
pub(super) fn init_static_fields_late(
    ctx: &mut crate::expr::FnCtx<'_>,
    hir: &HirModule,
) -> Result<()> {
    // Issue #685: nested classes (declared as expressions inside a
    // factory function body, e.g. `return class X extends Y { static
    // params = params.slice() }` in effect's `TemplateLiteralParser`)
    // are hoisted into `module.classes` by HIR lowering, but their
    // static-field initializers may reference parameters of the
    // enclosing function — those LocalIds aren't in the module-init
    // scope. The fallback at `expr.rs::LocalGet` returns `0.0`, so the
    // hoisted init becomes `(0.0).slice()` and throws
    // `TypeError: (number).slice is not a function` deep in
    // `<module>__init`, before any user code runs.
    //
    // Skip such inits at module level — the static field's storage
    // remains the zero default, which is wrong but harmless (the class
    // is built fresh on each factory invocation and the static slot
    // would need re-emitting per-invocation to be correct). The full
    // fix is to emit the init at the class-expression site inside the
    // factory body; tracking the eager-eval-of-inner-class-statics
    // separately.
    let mut module_local_scope: std::collections::HashSet<u32> =
        ctx.module_globals.keys().copied().collect();
    // Top-level `let` / `const` bindings may not appear in
    // `module_globals` (the global table only includes vars referenced
    // from inner functions or exported). For the purpose of "is this
    // LocalId in the module's own scope," count every top-level
    // `Stmt::Let` id too — otherwise a valid
    // `static foo = topLevelConst` would be wrongly skipped.
    for s in &hir.init {
        if let perry_hir::Stmt::Let { id, .. } = s {
            module_local_scope.insert(*id);
        }
    }
    let init_references_out_of_scope_local = |init_expr: &perry_hir::Expr| -> bool {
        let mut refs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        crate::collectors::collect_ref_ids_in_expr(init_expr, &mut refs);
        refs.iter().any(|id| !module_local_scope.contains(id))
    };
    for c in &hir.classes {
        // Next.js wall 54: a nested class's static-field initializers must run
        // when the enclosing function evaluates the class, not at module init.
        // Running a side-effectful one eagerly (e.g. `static #a = new Self()`)
        // both mistimes it and can crash before user code.
        if c.is_nested {
            continue;
        }
        for sf in &c.static_fields {
            // Computed-key static fields go through the class-static-symbol
            // side table. Refs #420 — drizzle's `static [entityKind] =
            // "Table"` is consulted by `Object.prototype.hasOwnProperty.call(
            // type, entityKind)` in drizzle's `is(value, type)`.
            if let (Some(key_expr), Some(init_expr)) = (sf.key_expr.as_ref(), sf.init.as_ref()) {
                if init_references_out_of_scope_local(init_expr)
                    || init_references_out_of_scope_local(key_expr)
                {
                    continue;
                }
                let Some(&class_id) = ctx.class_ids.get(&c.name) else {
                    continue;
                };
                let key_v = crate::expr::lower_expr(ctx, key_expr)?;
                let val_v = crate::expr::lower_expr(ctx, init_expr)?;
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
                continue;
            }
            let key = (c.name.clone(), sf.name.clone());
            // Register the field in the runtime CLASS_DYNAMIC_PROPS side
            // table (mirroring the StaticFieldSet lowering) so dynamic
            // class-ref reads and `getOwnPropertyDescriptor(C, name)` see an
            // own data property. Uninitialized fields (`static h;`) register
            // `undefined` — per spec they are still own properties.
            let emit_static_field_registration = |ctx: &mut crate::expr::FnCtx<'_>, value: &str| {
                if let Some(&class_id) = ctx.class_ids.get(&c.name) {
                    if class_id != 0 {
                        let idx = ctx.strings.intern(&sf.name);
                        let entry = ctx.strings.entry(idx);
                        let bytes_ref = format!("@{}", entry.bytes_global);
                        let len_str = entry.byte_len.to_string();
                        let cid_str = class_id.to_string();
                        ctx.block().call_void(
                            "js_class_register_static_field",
                            &[
                                (crate::types::I32, &cid_str),
                                (crate::types::PTR, &bytes_ref),
                                (crate::types::I64, &len_str),
                                (DOUBLE, value),
                            ],
                        );
                    }
                }
            };
            let Some(global_name) = ctx.static_field_globals.get(&key).cloned() else {
                continue;
            };
            if let Some(init_expr) = &sf.init {
                if init_references_out_of_scope_local(init_expr) {
                    continue;
                }
                // Skip fields whose initializer the HIR already emitted as an
                // inline `StaticFieldSet` at the class's source position (the
                // spec evaluation point). Re-running it here would (a) fire
                // initializer side effects twice and (b) clobber any user
                // reassignment made between the class decl and end of module
                // init. Mirrors the static-block dedup below. The inline
                // lowering also registers the field in CLASS_DYNAMIC_PROPS.
                let inline_initialized = hir.init.iter().any(|s| {
                    matches!(
                        s,
                        perry_hir::Stmt::Expr(perry_hir::Expr::StaticFieldSet {
                            class_name,
                            field_name,
                            ..
                        }) if *class_name == c.name && *field_name == sf.name
                    )
                });
                if inline_initialized {
                    continue;
                }
                // `this` in a static field initializer is the class
                // constructor (`static g = this.f + '262'`). Seed the same
                // class-ref NaN-box a static method binds (see
                // `compile_static_method`) for the init's duration.
                let seeded_this = ctx.class_ids.get(&c.name).copied().map(|cid| {
                    let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                    let class_ref_lit = crate::nanbox::double_literal(f64::from_bits(bits));
                    let this_slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.block().store(DOUBLE, &class_ref_lit, &this_slot);
                    ctx.this_stack.push(this_slot);
                });
                let v = crate::expr::lower_expr(ctx, init_expr);
                if seeded_this.is_some() {
                    ctx.this_stack.pop();
                }
                let v = v?;
                let g_ref = format!("@{}", global_name);
                crate::expr::emit_root_nanbox_store_on_block(ctx.block(), &v, &g_ref);
                emit_static_field_registration(ctx, &v);
            }
            // Uninitialized non-computed static fields are now registered in
            // `init_static_fields_early` (before user code) with value
            // `undefined`. Re-registering here — after user statements — would
            // clobber any `C.foo = …` the program performed between the class
            // declaration and module-init end, so the no-init `else` branch was
            // intentionally removed.
        }
    }
    // Static blocks — emitted as synthetic static methods with the
    // name prefix `__perry_static_init_`. HIR lowering injects an inline
    // `StaticMethodCall` for each one at the class-decl source position
    // (right after that class's static-field-init stmts), so blocks
    // normally run from `hir.init`. This loop is a fallback for any
    // class whose static_methods include a block not yet hooked via
    // init (e.g. class expressions that bypass the stmt-decl path);
    // calling it here keeps the legacy behavior of "always run, just
    // late" for those. (#2278)
    for c in &hir.classes {
        for sm in &c.static_methods {
            if !sm.name.starts_with("__perry_static_init_") {
                continue;
            }
            let key = (
                c.name.clone(),
                crate::codegen::static_method_registry_key(&sm.name),
            );
            // Skip if the init stream already invokes this block. The
            // typical class-decl path emits a `StaticMethodCall` for
            // each block; if we find one referencing this (class,
            // method) pair, the user-init lowering above has already
            // run it and a duplicate call here would double-fire any
            // observable side effects.
            if hir
                .init
                .iter()
                .any(|s| init_calls_static_block(s, &c.name, &sm.name))
            {
                continue;
            }
            if let Some(llvm_name) = ctx.methods.get(&key).cloned() {
                ctx.block().call(DOUBLE, &llvm_name, &[]);
            }
        }
    }
    Ok(())
}

/// Returns true if `stmt` contains, at any nesting depth (through
/// if/while/do-while/for/labeled/try/switch bodies), an `Expr(
/// StaticMethodCall)` invoking the (`class_name`, `method_name`) pair —
/// the shape HIR lowering emits at the class-decl position for each
/// `__perry_static_init_*` synthetic method. Used by
/// `init_static_fields_late` to skip per-(class, block) pairs that
/// have already been invoked inline. (#2278)
///
/// Must recurse: a class declared inside `try { class C { static {...}
/// } }` (test262 static-init-abrupt.js wraps its whole class this way)
/// lowers its inline `StaticMethodCall` into the `Try`'s `body`, not at
/// `hir.init`'s top level. A shallow top-level-only scan missed it, so
/// this late fallback re-invoked the block a second time — outside the
/// user's `try`, so a throwing block's second run surfaced as an
/// uncaught exception instead of staying silently absent.
fn init_calls_static_block(stmt: &perry_hir::Stmt, class_name: &str, method_name: &str) -> bool {
    use perry_hir::Stmt;
    let any_calls = |stmts: &[Stmt]| {
        stmts
            .iter()
            .any(|s| init_calls_static_block(s, class_name, method_name))
    };
    match stmt {
        Stmt::Expr(perry_hir::Expr::StaticMethodCall {
            class_name: c,
            method_name: m,
            ..
        }) => c == class_name && m == method_name,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => any_calls(then_branch) || else_branch.as_ref().is_some_and(|b| any_calls(b)),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            any_calls(body)
        }
        Stmt::Labeled { body, .. } => init_calls_static_block(body, class_name, method_name),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            any_calls(body)
                || catch.as_ref().is_some_and(|c| any_calls(&c.body))
                || finally.as_ref().is_some_and(|f| any_calls(f))
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|case| any_calls(&case.body)),
        _ => false,
    }
}

/// Issue #100: emit the IR that populates this module's
/// `@__perry_ns_<module_prefix>` global from the resolved namespace
/// entry list. Called at the end of `__perry_init_<prefix>` (or `main`
/// for the entry module) AFTER the module's top-level statements have
/// finished — at that point every local export's binding is set, and
/// every dependency's `__init` has already run (topo-sort guarantees
/// `ExportAll` / `ReExport` sources are initialised first), so
/// cross-module getters are safe to call.
///
/// The IR sequence per call:
///
///   1. Alloca three parallel stack arrays sized `[N x ?]` — keys (ptr),
///      key_lens (i32), values (double).
///   2. For each entry i in `namespace_entries`:
///      - Store `getelementptr inbounds [L x i8], ptr @.strK, i64 0, i64 0`
///        into `keys[i]` and `L` into `key_lens[i]`.
///      - Compute the value JSValue per `NamespaceEntryKind` and store
///        into `values[i]`.
///   3. Call `js_create_namespace(N, ptr keys, ptr key_lens, ptr values)`.
///   4. Store the result into `@__perry_ns_<module_prefix>`.
///
/// Always emits the `js_create_namespace` call + store, even when
/// `entries` is empty. This is required for Issue #842 (side-effect-only
/// dynamic-import targets — no exports, but the consumer still needs a
/// non-NaN `@__perry_ns_<prefix>` to load). The runtime tolerates
/// `n == 0` and returns an empty NaN-boxed object. The caller is
/// responsible for ensuring `key_globals.len() == entries.len()`.
pub(super) fn emit_namespace_populator(
    ctx: &mut crate::expr::FnCtx<'_>,
    entries: &[NamespaceEntry],
    key_globals: &[(String, usize)],
    module_prefix: &str,
) {
    debug_assert_eq!(entries.len(), key_globals.len());
    // Issue #842: side-effect-only dynamic-import targets land here
    // with `entries.is_empty()`. The runtime `js_create_namespace`
    // tolerates `n == 0` and returns a fresh empty object — exactly
    // what an export-less module's namespace should look like. We
    // still alloca minimum-size buffers (`[1 x ?]`) and pass the
    // pointers + n=0 so the runtime never dereferences them; the
    // per-entry loop simply doesn't execute.
    let n = entries.len();
    let buf_len = n.max(1);
    let blk = ctx.block();

    // Alloca the three parallel buffers.
    let keys_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x ptr]", keys_buf, buf_len));
    let lens_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x i32]", lens_buf, buf_len));
    let vals_buf = blk.next_reg();
    blk.emit_raw(format!("{} = alloca [{} x double]", vals_buf, buf_len));

    // Per-entry: store key ptr + len + value.
    for (i, entry) in entries.iter().enumerate() {
        let (key_global, key_len) = &key_globals[i];
        let idx_str = format!("{}", i);
        let blk = ctx.block();

        // keys[i] = @<key_global> as ptr
        let key_slot = blk.gep(PTR, &keys_buf, &[(I64, &idx_str)]);
        blk.store(PTR, &format!("@{}", key_global), &key_slot);

        // key_lens[i] = byte_len
        let len_slot = blk.gep(I32, &lens_buf, &[(I64, &idx_str)]);
        blk.store(I32, &format!("{}", key_len), &len_slot);

        // Materialise the value per kind. We drop the `blk` borrow so
        // each sub-emission can re-borrow ctx mutably for runtime calls
        // / declares; then re-acquire for the store.
        let val_str = match &entry.kind {
            NamespaceEntryKind::LocalVar { global_name } => {
                ctx.block().load(DOUBLE, &format!("@{}", global_name))
            }
            NamespaceEntryKind::LocalFunction { wrap_symbol } => {
                let blk = ctx.block();
                let handle = blk.call(
                    I64,
                    "js_closure_alloc_singleton",
                    &[(PTR, &format!("@{}", wrap_symbol))],
                );
                crate::expr::nanbox_pointer_inline(blk, &handle)
            }
            NamespaceEntryKind::LocalClass { class_id } => {
                // INT32-tagged class-id NaN-box: 0x7FFE_0000_0000_0000 |
                // (class_id & 0xFFFFFFFF). Matches `Expr::ClassRef`.
                let bits = crate::nanbox::INT32_TAG | (*class_id as u64 & 0xFFFF_FFFF);
                crate::nanbox::double_literal(f64::from_bits(bits))
            }
            NamespaceEntryKind::ForeignVar {
                source_prefix,
                source_local,
            } => {
                let getter = format!("perry_fn_{}__{}", source_prefix, sanitize(source_local));
                ctx.pending_declares.push((getter.clone(), DOUBLE, vec![]));
                ctx.block().call(DOUBLE, &getter, &[])
            }
            NamespaceEntryKind::ForeignFunction {
                source_prefix,
                source_local,
                param_count,
            } => {
                // Function-shaped re-exports must materialize a function
                // value, not call the function while building the namespace.
                // Source modules emit `__perry_wrap_perry_fn_<src>__<name>`
                // for every user function; hand that wrapper to the same
                // singleton allocator used by local function exports.
                let wrapper_name = format!(
                    "__perry_wrap_perry_fn_{}__{}",
                    source_prefix,
                    sanitize_member(source_local)
                );
                let arity = (*param_count).min(16);
                let mut wrapper_params: Vec<crate::types::LlvmType> = vec![I64];
                wrapper_params.extend(std::iter::repeat_n(DOUBLE, arity));
                ctx.pending_declares
                    .push((wrapper_name.clone(), DOUBLE, wrapper_params));
                let blk = ctx.block();
                let handle = blk.call(
                    I64,
                    "js_closure_alloc_singleton",
                    &[(PTR, &format!("@{}", wrapper_name))],
                );
                crate::expr::nanbox_pointer_inline(blk, &handle)
            }
            NamespaceEntryKind::NestedNamespace { source_prefix } => ctx
                .block()
                .load(DOUBLE, &format!("@__perry_ns_{}", source_prefix)),
        };

        let blk = ctx.block();
        let val_slot = blk.gep(DOUBLE, &vals_buf, &[(I64, &idx_str)]);
        blk.store(DOUBLE, &val_str, &val_slot);
    }

    // Call `js_create_namespace(n, keys, key_lens, values)` and store
    // the result into the namespace global. The result is a NaN-boxed
    // POINTER_TAG ObjectHeader; the global is already GC-rooted by
    // `register_module_globals_as_gc_roots` is NOT — namespace globals
    // aren't in `module_globals`. Register the address as a root here
    // so the object survives subsequent GC cycles.
    let n_str = format!("{}", n);
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_create_namespace",
        &[
            (I32, &n_str),
            (PTR, &keys_buf),
            (PTR, &lens_buf),
            (PTR, &vals_buf),
        ],
    );
    let ns_name = format!("__perry_ns_{}", module_prefix);
    crate::expr::emit_root_nanbox_store_on_block(blk, &result, &format!("@{}", ns_name));
    let addr_i64 = blk.ptrtoint(&format!("@{}", ns_name), I64);
    blk.call_void("js_gc_register_global_root", &[(I64, &addr_i64)]);
}

#[cfg(test)]
mod resolve_target_triple_tests {
    use super::resolve_target_triple;

    #[test]
    fn android_targets_resolve_to_android_elf_triples() {
        // #5742: `android-x86_64` must resolve (it previously returned None and
        // codegen fell back to the host triple, emitting COFF the Android
        // linker rejects). Both Android arms use the `-unknown-linux-android`
        // LLVM form so the emitted objects are ELF for the right arch.
        assert_eq!(
            resolve_target_triple("android").as_deref(),
            Some("aarch64-unknown-linux-android")
        );
        assert_eq!(
            resolve_target_triple("android-x86_64").as_deref(),
            Some("x86_64-unknown-linux-android")
        );
        // Unknown targets still fall through to None.
        assert_eq!(resolve_target_triple("android-mips"), None);
    }
}
