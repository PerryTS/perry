//! Coverage for the root lowering that actually SHIPS: native roots / RS4GC
//! statepoints (#7502).
//!
//! # Why this module exists
//!
//! Since #7370 native roots are the default on every target whose frames the
//! runtime can walk — every target Perry ships to except `arm64_32` watchOS and
//! ARM64 Windows. The two suites that read as this area's coverage
//! (`tests/shadow_slot_hygiene.rs`, `tests/scalar_replaced_slot_roots.rs`) were
//! written against the shadow stack and, since #7493, correctly SAY so with
//! `NativeRootsPin::shadow()`. Both stay: both lowerings are supported and both
//! need coverage. But it left nine root-lowering mechanics with zero assertions
//! against the lowering Perry emits, six of them shapes
//! `docs/src/internals/gc-rooting-invariant.md` records as having already
//! shipped broken.
//!
//! Three of those tests had been *passing vacuously*: they asserted
//! `js_shadow_slot_bind` was absent, which under the native default is true of
//! every program, rooted or not. That is CLAUDE.md hazard 4 — the gate ran, its
//! subject never did. This module is built so that mistake is structurally
//! harder to make; see "Non-vacuity" below.
//!
//! # What is asserted, and against what
//!
//! Three vantage points, weakest to strongest:
//!
//! 1. **Pre-`opt` IR** — what [`compile_module`] returns. Shows the *request*:
//!    which allocas codegen retyped to `ptr addrspace(1)`, and whether the
//!    function carries `gc "statepoint-example"`. This is the right vantage for
//!    the NEGATIVE direction (a numeric local must never become a root), because
//!    it is the only place where "codegen never asked" and "LLVM optimized it
//!    away" are still distinguishable.
//! 2. **Post-RS4GC IR** — [`crate::inprocess::statepoint_rewritten_ir`], which
//!    runs the production pass string. Shows the *result*: every safepoint and
//!    its `"gc-live"` bundle, keyed by callee name — so "is this value a root
//!    ACROSS THAT CALL" becomes a direct question. This is the vantage the
//!    shadow suites never had an equivalent for.
//! 3. **The emitted stack map** — [`crate::gc_map::decode_stack_map_roots`],
//!    the compact `__perry_gcmap` blob the collector reads at run time. Strongest,
//!    and the only one that can catch "the map says nothing lives here".
//!
//! # Non-vacuity, which is the whole point
//!
//! Every assertion here is written so that a broken lowering *cannot* satisfy
//! it by producing nothing:
//!
//! * **Positive claims assert their subject ran.** [`Statepoints::at`] panics if
//!   the callee it is asked about produced no safepoint, and
//!   [`map_records_for`] panics if the function is missing from the map
//!   entirely. "Zero roots" is only ever asserted about a record that exists.
//! * **Every negative claim is paired with a differential control** that must
//!   go the other way in the same test — the numeric-local and numeric-literal
//!   cases each compare against a structurally identical heap-valued program.
//!   A lowering that roots nothing fails the control half.
//! * **Targets are pinned, not host-derived.** `cargo-test` runs on x86_64
//!   Linux and this repo is developed on arm64 macOS; a suite that silently
//!   changed subject with the host is how a target-specific lowering bug
//!   reaches a release. Both shipped native-roots targets are compiled and
//!   emitted for on every host, which LLVM does happily — only the *backend*
//!   has to be initialized, not the OS.
//!
//! Each test's doc comment names the sabotage that was run against it and what
//! it did. An assertion nobody has watched fail is documentation.
//!
//! # Coverage against #7502's table
//!
//! | # | mechanic | native assertion | in |
//! |---|---|---|---|
//! | 1 | a pointer local is a root | `ptr addrspace(1)` slot + live at the next allocation's statepoint + in the map | `mechanics::a_live_pointer_local_is_a_root_in_the_emitted_map` |
//! | 2 | a dead value stops being a root | absent from the live set, against a live control | `mechanics::a_value_that_is_dead_at_a_safepoint_is_not_in_its_live_set` |
//! | 3 | a numeric local reserves nothing | slot counts `(1, 2)` against a heap twin | `mechanics::a_numeric_local_reserves_no_root_and_a_heap_one_does` |
//! | 4 | slot indices unshifted by a numeric local | *subsumed by 3* — native roots have no indices; the substance is that a numeric local does not perturb the root set | — |
//! | 5 | entry roots begin after the init prelude | first rooted safepoint follows `js_gc_init` and `__perry_init_strings_*` | `mechanics::no_entry_module_root_is_live_before_the_gc_is_initialized` |
//! | 6 | a loop's roots do not cross the back edge | in-loop live set is 1, against a 2-root control | `mechanics::a_loop_iterations_dead_root_is_not_live_at_the_next_iteration` |
//! | 7 | scalar-replaced heap field is a root (#6968) | extra slot + non-empty map, against the numeric twin | `mechanics::a_scalar_replaced_field_holding_a_heap_value_is_a_native_root` |
//! | 8 | scalar-replaced numeric literal pays nothing (#6997) | empty map, against a one-heap-field twin | `mechanics::a_numeric_only_scalar_replaced_literal_pays_no_native_rooting` |
//! | 9 | every reserved slot reaches the root set (#7184) | two live locals, two map roots | `mechanics::a_deduplicated_slot_index_still_reaches_the_native_root_set` |
//!
//! **Row 9 is where #7502's table is wrong**, and the correction is worth
//! stating: it marks the row `n/a` because "no frame bound exists; the #7184
//! shape is unrepresentable". The *frame* bound is gone, but the failure is not
//! about a frame — it is about an index silently falling outside the structure
//! that collects roots, and `lower_precise_roots_to_native_stack` still has one
//! (`roots.get_mut(idx)` over a `slot_count`-sized vector). An out-of-range
//! index there drops the alloca from `root_ptrs`, so it is never retyped and
//! never rooted, with no diagnostic — the same silence for the same reason, one
//! layer up from the runtime bounds check. It now has a test and a sabotage.
//!
//! There is also a native-only prerequisite with no shadow counterpart at all,
//! covered in `harness_self_tests::no_root_alloca_survives_the_statepoint_rewrite`:
//! RS4GC relocates SSA values and does not scan allocas, so a root slot that
//! escapes `mem2reg` is a root the collector never rewrites.

use crate::testing::NativeRootsPin;
use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Param, Stmt};

mod harness_self_tests;
mod mechanics;

/// The two targets native roots ship on, one per object format and
/// architecture. Pinned rather than host-derived — see the module docs.
///
/// `arm64_32` watchOS and ARM64 Windows are deliberately absent: they take the
/// shadow-stack lowering (`codegen::helpers::set_native_roots_for_target`), and
/// the suites that cover *that* lowering are the shadow-pinned ones.
pub(crate) const NATIVE_TARGETS: [&str; 2] =
    ["arm64-apple-macosx15.0.0", "x86_64-unknown-linux-gnu"];

// ---------------------------------------------------------------------------
// HIR fixtures
// ---------------------------------------------------------------------------

pub(crate) fn ir_opts(target: &str, is_entry: bool) -> CompileOptions {
    CompileOptions {
        target: Some(target.to_string()),
        is_entry_module: is_entry,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: crate::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn bare_module(name: &str) -> Module {
    Module {
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

/// A module whose only content is one non-exported function `probe(n)`.
///
/// The parameter exists so a loop bound can be opaque: a constant-trip loop
/// unrolls, and an unrolled loop has no back edge to make claims about.
pub(crate) fn probe_module(name: &str, body: Vec<Stmt>) -> Module {
    let mut module = bare_module(name);
    module.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 100,
            name: "n".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
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
    }];
    module
}

/// A module whose statements are top-level init — the shape that lands in
/// `main` for an entry module.
pub(crate) fn entry_module(name: &str, init: Vec<Stmt>) -> Module {
    let mut module = bare_module(name);
    module.init = init;
    module
}

/// The LLVM symbol `probe_module`'s function gets.
pub(crate) fn probe_symbol(module_name: &str) -> String {
    format!(
        "perry_fn_{}__probe",
        module_name.replace(['.', '-', '/'], "_")
    )
}

pub(crate) fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// A fresh heap value bound to nothing else, so the slot under test is the only
/// reference to it.
pub(crate) fn heap_value() -> Expr {
    Expr::Object(vec![("k".to_string(), Expr::Number(1.0))])
}

pub(crate) fn field_get(local: u32, field: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(local)),
        property: field.to_string(),
        byte_offset: 0,
    }
}

pub(crate) fn console_log(args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "log".to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Compile `module` under the native-roots lowering for `target`.
///
/// The pin is not redundant with the default: it also overrides `PERRY_RS4GC`
/// from the environment, so these assertions mean the same thing during a
/// `PERRY_RS4GC=0` bisection as they do in CI.
pub(crate) fn native_ir(module: &Module, target: &str, is_entry: bool) -> String {
    let _pin = NativeRootsPin::native();
    let bytes = compile_module(module, ir_opts(target, is_entry))
        .unwrap_or_else(|e| panic!("codegen failed for {}: {e}", module.name));
    String::from_utf8(bytes).expect("LLVM IR should be UTF-8")
}

/// The whole `define … { … }` body of `name`.
pub(crate) fn function_slice<'a>(ir: &'a str, name: &str) -> &'a str {
    let marker = format!("@{}(", name);
    let start = ir
        .match_indices("define ")
        .find_map(|(idx, _)| {
            let line_end = ir[idx..].find('\n').map(|o| idx + o)?;
            ir[idx..line_end].contains(&marker).then_some(idx)
        })
        .unwrap_or_else(|| panic!("no function `{name}` in IR:\n{ir}"));
    let end = ir[start..]
        .find("\n}\n")
        .map(|o| start + o + 3)
        .unwrap_or(ir.len());
    &ir[start..end]
}

/// Root slots codegen ASKED for in this function: allocas it retyped to
/// `ptr addrspace(1)` so RS4GC will treat their contents as GC references.
pub(crate) fn root_allocas(fn_ir: &str) -> usize {
    fn_ir.matches("alloca ptr addrspace(1)").count()
}

/// Entry-block slots codegen left as plain scalars — a local it decided can
/// never hold a collectable value.
pub(crate) fn scalar_allocas(fn_ir: &str) -> usize {
    fn_ir.matches("= alloca double").count() + fn_ir.matches("= alloca i64").count()
}

// ---------------------------------------------------------------------------
// Post-RS4GC IR: safepoints and their live sets
// ---------------------------------------------------------------------------

/// One `gc.statepoint`: the call it wraps, and the GC values LLVM recorded as
/// live across it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Statepoint {
    /// Direct callee name (no `@`), or `<indirect>`.
    pub callee: String,
    /// SSA registers in the `"gc-live"` bundle, in printed order.
    pub live: Vec<String>,
}

/// Every safepoint in one function, in program order.
#[derive(Debug, Clone)]
pub(crate) struct Statepoints {
    function: String,
    points: Vec<Statepoint>,
}

impl Statepoints {
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Statepoint> {
        self.points.iter()
    }

    /// Every safepoint whose callee is `callee`.
    ///
    /// **Panics when there are none.** That is the point: a mechanic asserted
    /// about "the safepoint at `js_object_alloc`" must not quietly become a
    /// claim about the empty set because the call was inlined, renamed, or
    /// classified `gc-leaf-function`.
    pub fn at(&self, callee: &str) -> Vec<&Statepoint> {
        let hits: Vec<&Statepoint> = self
            .points
            .iter()
            .filter(|sp| sp.callee == callee)
            .collect();
        assert!(
            !hits.is_empty(),
            "no `{callee}` safepoint in @{} — this assertion has no subject. \
             Safepoints present: {:?}",
            self.function,
            self.points.iter().map(|sp| &sp.callee).collect::<Vec<_>>()
        );
        hits
    }

    /// The largest live set at any safepoint in the function.
    pub fn max_live(&self) -> usize {
        self.points
            .iter()
            .map(|sp| sp.live.len())
            .max()
            .unwrap_or(0)
    }
}

/// Run the production statepoint rewrite over `ir` and read back every
/// safepoint in `function`.
pub(crate) fn statepoints_of(ir: &str, target: &str, function: &str) -> Statepoints {
    let rewritten = crate::inprocess::statepoint_rewritten_ir(ir, target, "native_root_coverage")
        .unwrap_or_else(|e| panic!("statepoint rewrite failed for {target}: {e:#}"));
    let body = function_slice(&rewritten, function);
    let points = body
        .lines()
        .filter(|line| line.contains("llvm.experimental.gc.statepoint"))
        .map(parse_statepoint)
        .collect();
    Statepoints {
        function: function.to_string(),
        points,
    }
}

/// Parse one printed `gc.statepoint` call/invoke line.
///
/// Two facts about LLVM's printer make this a line-at-a-time job: it prints one
/// instruction per line, and it prints the live set as a `"gc-live"` operand
/// bundle after the argument list.
///
/// The callee is the operand after the `elementtype(...)` attribute, and
/// finding it needs paren MATCHING, not a substring search — the statepoint
/// intrinsic's own signature `(i64, i32, ptr, i32, i32, ...)` and the wrapped
/// callee's type `elementtype(i64 (i32))` both contain `) @`, and the first one
/// belongs to `@llvm.experimental.gc.statepoint.p0`. Reading that as the callee
/// makes every safepoint in every function look identical, which is exactly how
/// [`Statepoints::at`] would then find no subject anywhere — the self-test
/// `the_statepoint_parser_reads_callee_and_live_set` is what caught it.
fn parse_statepoint(line: &str) -> Statepoint {
    let callee = callee_after_elementtype(line).unwrap_or_else(|| "<indirect>".to_string());

    let live = match group_after(line, "\"gc-live\"(") {
        None => Vec::new(),
        Some(operands) => operands
            .split(',')
            .filter_map(|operand| {
                operand
                    .trim()
                    .rsplit_once(' ')
                    .map(|(_, reg)| reg.trim().to_string())
            })
            .filter(|reg| reg.starts_with('%'))
            .collect(),
    };

    Statepoint { callee, live }
}

/// The contents of the parenthesised group opened by `marker`, honouring
/// nesting.
///
/// The nesting is not hypothetical and reading to the first `)` is not a near
/// miss: **every operand of a live set is spelled `ptr addrspace(1) %r`**, so a
/// first-paren scan truncates the list to `"ptr addrspace(1"`, drops it for
/// having no `%`, and reports an EMPTY LIVE SET for every statepoint in every
/// program. Half this module's assertions are "nothing is live here"; all of
/// them would have passed. `the_statepoint_parser_reads_callee_and_live_set` is
/// what caught it.
fn group_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let open = line.find(marker)? + marker.len();
    let rest = &line[open..];
    let mut depth = 1usize;
    let close = rest.char_indices().find_map(|(index, ch)| match ch {
        '(' => {
            depth += 1;
            None
        }
        ')' => {
            depth -= 1;
            (depth == 0).then_some(index)
        }
        _ => None,
    })?;
    Some(&rest[..close])
}

/// The `@name` immediately after the balanced `elementtype(…)` attribute, or
/// `None` for an indirect callee (a `%reg` there instead).
fn callee_after_elementtype(line: &str) -> Option<String> {
    let group = group_after(line, "elementtype(")?;
    let after =
        line[line.find("elementtype(")? + "elementtype(".len() + group.len() + 1..].trim_start();
    let name: String = after
        .strip_prefix('@')?
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
        .collect();
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------
// The emitted stack map
// ---------------------------------------------------------------------------

/// Emit `ir` as assembly for `target`, through the production emission path.
///
/// `-O0` on purpose. The claim under test is about the root SET, and at `-O3`
/// LLVM is free to delete an allocation whose result is unused — which would
/// turn "no roots live here" into a true statement about a program that no
/// longer contains the code being asserted about.
pub(crate) fn assembly_for(ir: &str, target: &str) -> String {
    let context = inkwell::context::Context::create();
    let module = crate::inprocess::parse_ir_text(&context, ir, "native_root_coverage")
        .unwrap_or_else(|e| panic!("IR does not parse for {target}: {e:#}"));
    let bytes = crate::inprocess::optimize_and_emit_module(
        &module,
        target,
        &["-O0".to_string(), "-S".to_string()],
    )
    .unwrap_or_else(|e| panic!("assembly emission failed for {target}: {e:#}"));
    String::from_utf8(bytes).expect("assembler text should be UTF-8")
}

/// Per-safepoint root lists for `symbol` from the compact map the binary ships.
///
/// **Panics if the function is absent from the map.** A missing function and a
/// function with no roots are the two answers this whole module exists to tell
/// apart.
pub(crate) fn map_records_for(asm: &str, target: &str, symbol: &str) -> Vec<Vec<(u16, i32)>> {
    let functions = crate::gc_map::decode_stack_map_roots(asm, target)
        .unwrap_or_else(|e| panic!("stack map for {target} did not decode: {e}"));
    // Mach-O prefixes global symbols with `_`; ELF does not.
    let underscored = format!("_{symbol}");
    functions
        .iter()
        .find(|(name, _)| name == symbol || name == &underscored)
        .map(|(_, records)| records.clone())
        .unwrap_or_else(|| {
            panic!(
                "`{symbol}` has no stack-map entry for {target} — the collector \
                 would find no roots in it at all. Functions in the map: {:?}",
                functions.iter().map(|(n, _)| n).collect::<Vec<_>>()
            )
        })
}

/// The most roots any one safepoint of `symbol` records.
pub(crate) fn map_max_roots(asm: &str, target: &str, symbol: &str) -> usize {
    let records = map_records_for(asm, target, symbol);
    assert!(
        !records.is_empty(),
        "`{symbol}` is in the {target} stack map with zero safepoints — nothing \
         was measured"
    );
    records.iter().map(|r| r.len()).max().unwrap_or(0)
}
