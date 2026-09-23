//! The per-module Layer 1 migration ledger and its tests, split out of
//! `rooting/mod.rs` to stay under the 2000-line cap.
//!
//! Pure move: the `include_str!` targets below are unchanged because this file
//! is a sibling of `mod.rs`, so every relative path resolves identically.

// ---------------------------------------------------------------------------
// The per-module migration ledger (RFC step 3).
//
// "Migrate one family at a time [...] `#[deny]` the escape hatch per-module as
// each module finishes, so migrated code cannot regress." Rust has no attribute
// that denies calling a `pub(crate)` function from one module, so the deny is
// spelled as a test over the module's own source, inlined at COMPILE time by
// `include_str!` -- no path, no working directory, no stale checkout.
//
// `expr::temp_root` IS the escape hatch. It is the raw, order-sensitive API
// (push / get / set / truncate, guards the caller must remember to release),
// and every bug in the #7341 family was an ordering mistake against it. A
// migrated module names `crate::rooting` and nothing else.
// ---------------------------------------------------------------------------

/// Modules that have completed the Layer 1 migration, with their source
/// inlined at compile time.
///
/// Adding a line here is how a migration slice finishes. Removing one is a
/// regression, not a cleanup.
/// A module is listed here only when it is migrated **end to end**. Slice 1's
/// `lower_array_method.rs` is one file and lands whole; `expr/url_main.rs` sat
/// half-migrated from #7461 to #7617, which is the reason the rule exists. When
/// a module genuinely cannot land in one PR, the boundary goes in this comment
/// with the slice that will finish it — an unlisted module is indistinguishable
/// from an unstarted one, and that is what let the half-migration hide.
///
/// No boundary is outstanding today.
///
/// **Listing a module that never used the escape hatch passes vacuously.** That
/// was true of both slice-1 modules: they named no `temp_root` symbol before
/// the migration, so `migrated_modules_do_not_reach_past_the_rooting_api` went
/// green the instant the line was added. The listing only means something if the
/// slice ALSO ran the sabotage arm — inject a real, compiling `temp_root_push_*`
/// / `temp_root_truncate` pair into the migrated module, confirm the ledger test
/// goes red and names the lines, then revert. Every slice so far has, and
/// recorded it in its PR; a slice that skips it is adding a line that asserts
/// nothing.
///
/// Slice 2's three modules are the first since the template where the listing is
/// **not** vacuous: all three named `expr::temp_root` before the migration
/// (`lower_exprs_rooted`, `guard_store_operand_across`, `rooted_handle_*`,
/// `temp_root_{push,get,set}_double`), so the ledger line is load-bearing on the
/// committed source and not only under sabotage. The sabotage arm was still run
/// per module — the assert stops at the first offender, so one run cannot speak
/// for three.
///
/// Slice 3 is three of each. `objects_arrays_lit.rs`, `array_literal.rs` and
/// `object_literal.rs` all named `expr::temp_root` before the migration
/// (`temp_root_{push,get,set}_i64`, `lower_exprs_rooted`/`temp_root_release`,
/// `rooted_handle_*`/`temp_root_{push,get}_double`), so their lines are
/// load-bearing. `array_push.rs` named none and its line is vacuous on the
/// committed source — the sabotage arm is the only thing that makes it an
/// assertion, exactly as for both slice-1 modules, and the audit that earned
/// the listing is written into that file's header rather than into this one.
///
/// Slice 4 is three load-bearing and six vacuous. `index_get.rs`,
/// `index_set.rs` and `property_set.rs` all named the `StoreOperandGuard`
/// family (`guard_store_operand`, `guard_store_operand_across`,
/// `reread_store_operand`, `release_store_operand`, `expr_may_trigger_gc`)
/// before the migration, so their lines hold on the committed source. The other
/// six — `index_get/guarded_array.rs`, `index_get/inline_dyn_typed_array.rs`,
/// `index_set_typed_array.rs`, `property_get.rs`, `property_get/globalget.rs`,
/// `property_get/helpers.rs` — named none, and each carries the audit that
/// earned its listing in its own file header.
///
/// ★ **A listed module is not an audited module, and slice 4 is where that
/// distinction became load-bearing.** These two properties are not the same:
///
///   1. every rooting decision the module makes goes through `crate::rooting`
///      — which is what this ledger checks, and what the listing means;
///   2. every window in the module HAS a rooting decision.
///
/// `index_set.rs` and `index_get.rs` satisfy (1) and do not satisfy (2). The
/// migration itself surfaced the gap: translating the six guarded arms of
/// `index_set.rs` made it obvious that its `#5525` typed-array arm, its
/// bounded-index array store and ten arms of `index_get.rs` lower a receiver,
/// then lower more user code, then use the receiver — and root nothing. Three
/// of those were adjacent to arms this slice was already rewriting and are
/// fixed here (#7637, #7638, #7639); the rest are filed as #7640 rather than
/// fixed, because they sit on inline fast paths where a temp root is a measured
/// cost rather than plumbing.
///
/// So do not read a ledger line as "this module has no rooting bugs". It says
/// the module cannot make an ORDERING mistake against the raw API, because it
/// no longer names it. A window with no decision at all is invisible to this
/// check, and the only instrument for it is reading the module.
///
/// Slice 5 is two modules of the `lower_call/` family, both load-bearing:
/// `extern_timers.rs` and `namespace_call.rs` each named
/// `lower_exprs_rooted` / `temp_root_release` before the migration, so their
/// lines hold on the committed source and not only under sabotage.
///
/// It is two rather than the six the family contains, and the four left out are
/// left out for one reason worth recording, because it is a **statement about
/// this API rather than about those files**: three of them need a re-read at
/// more than one point, and every `with_operands_rooted*` form has exactly one.
///
///   * `lower_call/mod.rs` — `lower_call_args_rooted` and
///     `lower_rest_call_args_rooted` return a guard *deliberately*: their
///     consumers in `func_ref.rs` are block-splitting specialized-ABI diamonds,
///     so the release must sit in a merge block that post-dominates four
///     dispatch paths, ~200 lines below the lowering. A closure form can express
///     that only by swallowing the whole dispatch chain, in a file outside the
///     slice.
///   * `lower_call/new.rs` — `refresh_rooted_args` re-reads the SAME operand
///     group at three caller-chosen points (after the instance allocation,
///     before the field initializers, before an inlined constructor body), under
///     a `temp_root_scope_begin`/`_end` marker spanning ~20 return paths.
///   * `lower_call/console_promise.rs` — `lower_dynamic_closure_call` re-reads
///     the receiver and callee below the arguments, then re-reads the arguments
///     again below the allocating rebind unbox. Two stages, one combinator.
///   * `lower_call/early_branches.rs` — its only escape-hatch uses are
///     `implicit_this_save`/`implicit_this_restore`, which is already a paired
///     combinator rather than the raw ordering API. Migrating it means
///     re-exporting that pair through `crate::rooting`, which is a rename that
///     would make the ledger line look substantive while asserting nothing new.
///
/// The honest reading: a module that cannot be migrated because the API cannot
/// say what it means is a gap in the API, and recording it here is what stops
/// the next slice from rediscovering it. The variadic/rest shape (per-element
/// re-reads between allocating pushes) is the concrete missing combinator.
///
/// **Slice 6 built that combinator and it is not the one slice 5 named.**
/// Slice 5's hypothesis was the variadic/rest shape; three modules wanted it and
/// only one of them is variadic. What all three want is
/// [`RootedGroup`] — one temp-root scope, re-readable at ANY number of
/// caller-chosen points — of which the rest shape is the case that also holds
/// an accumulator array. The block comment above [`RootedGroup`] argues the
/// shape and the two entry points; the reason there are two is that
/// `func_ref.rs`'s release must post-dominate four block-splitting dispatch
/// diamonds, which no closure form can own without swallowing the dispatch
/// chain.
///
/// Slice 6 lists three modules, all load-bearing on the committed source:
///
///   * `lower_call/mod.rs` — `lower_call_args_rooted` /
///     `lower_rest_call_args_rooted` / `emit_rooted_call`. Named
///     `lower_exprs_rooted`, `root_operands_begin`, `rooted_array_begin`,
///     `temp_rooted_array_push`, `rooted_array_read` and `temp_root_release`.
///     It now hands its callers a [`RootedGroup`] instead of an
///     `Option<String>` slot index, which is what makes `extern_func.rs`,
///     `namespace_call.rs` and `func_ref.rs` unable to truncate at the wrong
///     slot even though they still hold the scope.
///   * `lower_call/func_ref.rs` — the escaping-release consumer, plus
///     `implicit_this_save` / `implicit_this_restore`.
///   * `lower_call/console_promise.rs` — the two-stage dynamic closure call,
///     the `js_native_call_method_by_id` dispatch (which turned out to be the
///     plain single-re-read shape after all), and eight `console.*` arms.
///
/// **`implicit_this_save` / `implicit_this_restore` MOVED here** rather than
/// being re-exported, so the pair has one spelling. That incidentally clears
/// the escape hatch out of `early_branches.rs`, `method_override.rs` and both
/// `property_get` dispatchers, whose only uses were that pair. They are
/// deliberately NOT listed: a line here would assert that the module makes
/// every rooting decision through this API, and nobody has read those four
/// modules for windows with no decision at all. An unlisted module is honest;
/// a listed unaudited one is the distinction slice 4 had to draw the hard way.
///
/// Slice 7 lists three `expr/` modules, all load-bearing on the committed
/// source (`temp_root_{push,get}_double`, `temp_root_truncate`,
/// `guard_store_operand{,_across}`, `reread_store_operand`,
/// `release_store_operand`, `expr_may_trigger_gc`):
///
///   * `expr/child_proc.rs` — every `child_process` entry point. Three arms
///     rooted unconditionally through the raw API and five rooted nothing at
///     all while holding RAW heap pointers across arbitrary user lowerings.
///   * `expr/proxy_reflect.rs` — the densest unaudited module in the campaign.
///     One arm made a rooting decision (the `PutValueSet` write-IC, #7201) and
///     twenty-eight made none.
///   * `expr/fs_await.rs` — the await loop, whose root was correct and simply
///     never released.
///
/// **Slice 7 added the one combinator this file had refused to add ahead of a
/// caller**, and the refusal was right: the shape it predicted is the shape
/// that turned up. [`RootedGroup::adopt_emitted`] roots a GC-managed value that
/// an emitted step produced rather than one lowered from an `Expr` — the
/// coerced key of `process.env[k] = v`, and the assimilated promise the await
/// loop polls. Its doc carries the argument for why [`call_rooted`] cannot
/// serve and the note on what it weakens. Which is the shape of the answer this
/// campaign keeps arriving at: an API gap recorded in slice N is a combinator
/// in slice N+1, and writing the gap down is what makes the next slice cheap.
///
/// # Slice 8 — the campaign's last slice, and its terminal condition
///
/// The plan's terminal condition was `expr/temp_root.rs` "going
/// `pub(in crate::rooting)` — the raw accessor unreachable, not merely
/// uncounted". As literally spelled that is **not expressible in Rust**:
/// `pub(in path)` requires `path` to be an ANCESTOR module of the item
/// (E0742), and `crate::rooting` is not an ancestor of `crate::expr::temp_root`.
/// So the file MOVED — it is `crate::rooting::temp_root` now, declared with a
/// private `mod temp_root;` and with every accessor additionally carrying an
/// explicit `pub(in crate::rooting)`. Either alone would do it; both are here
/// because the module declaration is one keyword away from re-widening
/// twenty-five items at once.
///
/// Two items keep `pub(crate)` and are re-exported at the top of this file,
/// and neither is an accessor: [`TempRootPool`] is the compile-time slot
/// bookkeeping `FnCtx` owns (no runtime behaviour, no ordering), and
/// `expr_is_inert_primitive` is the shared "can evaluating this run user
/// code?" predicate the loop back-edge poll consults
/// (`crate::loop_purity`). A predicate cannot be called in the wrong order.
///
/// **Fourteen items were DELETED rather than narrowed**, because slice 8 left
/// them with no caller at all: `lower_exprs_rooted`,
/// `lower_operand_pair_rooted`, `any_later_ref_may_trigger_gc`,
/// `RootedOperands::is_rooted`, the whole `StoreOperandGuard` family
/// (`guard_store_operand`, `guard_store_operand_across`,
/// `reread_store_operand`, `release_store_operand`), the whole `RootedHandle`
/// family (`rooted_handle_begin`/`_get`/`_release`) and
/// `temp_root_scope_begin`/`_end`. CLAUDE.md's kill-policy is explicit that
/// "the losing mode should stop compiling", and each of these WAS a losing
/// mode: a caller-managed guard whose combinator replacement owns the release.
///
/// ## Modules migrated, and how they were told apart from the decision-free ones
///
/// The brief for this slice listed 14 files by `expr::temp_root` mention.
/// That count conflates two populations, and the ledger is only meaningful for
/// one of them. Sorting them is the first half of the work:
///
/// **Eight modules made rooting decisions and are listed below.** Seven are
/// load-bearing on the committed source (each named the raw API before the
/// migration):
///
///   * `expr/binary.rs` — five `lower_operand_pair_rooted` +
///     `temp_root_release` pairs, one per dynamic-dispatch arm, each with the
///     release on its own `return` path. They collapse into one
///     `lower_rooted_dynamic_binary` helper over [`with_operands_rooted`]:
///     five chances to misplace a release become none.
///   * `expr/math_simple.rs` — `Expr::MapSet` is a [`RootedGroup`] (two
///     operands with UNEQUAL windows, re-read at eight arm-specific points,
///     released once); `MapGet`/`MapHas` are the plain single-re-read shape.
///     `Expr::ArrayMap` is a live bug, below.
///   * `expr/static_field_meta.rs` — `ClassExprFresh` is a [`RootedGroup`]
///     over the class object with a nested [`with_rooted_accumulator`] for the
///     `__perry_ctor_caps` snapshot array and a nested
///     [`with_operands_rooted`] per symbol static.
///   * `expr/dyn_extern_i18n.rs` — the namespace-object build (#7280's
///     269-member zod case) is exactly [`with_rooted_accumulator`]'s shape.
///   * `lower_string_method.rs` — the receiver root that spans ~60 return
///     paths, via [`open_rooted_group`].
///   * `lower_string_concat.rs` — split out of `lower_string_method.rs` this
///     slice; load-bearing because the code it contains named
///     `lower_exprs_rooted`, `lower_operand_pair_rooted` and four raw
///     push/get/truncate/release spellings on `main`.
///   * `lower_call/new.rs` — `refresh_rooted_args` re-reads one operand group
///     at three caller-chosen points under a scope marker spanning ~20 return
///     paths. Slice 5 named it as the shape the API could not express and
///     slice 6 built [`RootedGroup`] for exactly it; this is the collection.
///
/// The eighth, `lower_call/new_alloc.rs`, is **vacuous on the committed
/// source** — it never named the raw API, because it is the instance
/// allocation carved out of `new.rs` this slice and everything it emits sits
/// above the instance root. It is listed anyway, for the reason slice 3 listed
/// `array_push.rs`: an unlisted sibling of a listed module is the obvious
/// place to put a raw push and escape the check. Its listing means something
/// only because the sabotage arm was run on it.
///
/// **Nine files mention the raw API and make no rooting decision at all.**
/// They are deliberately NOT listed, because a ledger line on a module that
/// never had a decision to make looks substantive and asserts nothing:
///
///   * `expr/mod.rs` — declared `mod temp_root` and typed the `temp_roots`
///     field. Structural; both are gone with the move.
///   * `codegen/entry.rs`, `codegen/method.rs`, `codegen/function.rs`,
///     `codegen/closure.rs` — `TempRootPool::default()` at each `FnCtx`
///     construction. Constructing the pool is not using it.
///   * `stmt/loops.rs` — one call to `expr_is_inert_primitive`, a purity
///     predicate for the back-edge poll.
///   * `loop_purity.rs` — a doc link and nothing else (zero code sites; the
///     brief's count included the comment).
///   * `root_reload.rs`, `gc_call_effects.rs`, `runtime_decls/arrays.rs` —
///     the STRING LITERALS `"js_gc_temp_root_push"` and friends, which name
///     runtime symbols, not this module. So do five test files
///     (`expr/slice7_rooting_tests.rs`, `lower_call/console_rooting_tests.rs`,
///     `lower_call/timer_rooting_tests.rs`,
///     `codegen/testing_feature_gate_tests.rs`) plus
///     `linker_temp_lifecycle_tests.rs`, whose `temp_root_if_clang_available`
///     is about a temporary DIRECTORY.
///
/// ## The three unverified leads slice 7 handed over
///
///   * `static_field_meta.rs`'s `caps_arr` — **a real accumulator shape with a
///     provably empty window.** The array holds the only reference to
///     everything pushed so far while the next element is lowered, which is
///     #6951 exactly; but `captured_args` is built at one site
///     (`lower/lower_expr/arm_class.rs`) as
///     `ids.iter().map(|id| Expr::LocalGet(*id))`, and `expr_may_trigger_gc`
///     answers `false` for every `LocalGet`. So it is rooted through
///     [`with_rooted_accumulator`] with `protect` computed rather than
///     assumed: today that is `false` and the IR is byte for byte unchanged,
///     and the day a non-inert expression reaches the list it is rooted by
///     construction.
///   * `math_simple.rs`'s `ArrayMap` — **CONFIRMED live.** The receiver was
///     lowered, `callback` was lowered, and only then was the receiver
///     unboxed: `unbox_to_i64` below its own window masks a stale box rather
///     than repairing it (#7280 taxonomy (c)). `arr.map(x => …)` allocates a
///     closure at minimum. Fixed via [`with_operands_rooted`].
///   * `dyn_extern_i18n.rs`'s `path_handle` — **DISMISSED, and the premise is
///     wrong about the CFG.** The lead says the raw handle is reused across a
///     compare loop "that runs module `__init` bodies". It does not: each
///     `<prefix>__init()` is emitted into that iteration's MATCH block, which
///     branches straight to the join, so no `__init` dominates any later use
///     of `path_handle`. Along the fallthrough chain the only emissions
///     between the handle's production and its last use are
///     `js_get_string_pointer_unified` and `js_string_equals` — neither
///     re-enters user code nor enumerates an object, which is the standard
///     [`with_operands_rooted_across_call`]'s doc sets for an emitted step
///     (#7198). What that module DID have was the namespace-object
///     accumulator, which is migrated above.
#[cfg(test)]
const MIGRATED_MODULES: &[(&str, &str)] = &[
    (
        "crates/perry-codegen/src/expr/url_main.rs",
        include_str!("../expr/url_main.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_array_method.rs",
        include_str!("../lower_array_method.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/arrays_finds.rs",
        include_str!("../expr/arrays_finds.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_methods.rs",
        include_str!("../expr/array_methods.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/instance_misc1.rs",
        include_str!("../expr/instance_misc1.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/logical_collections.rs",
        include_str!("../expr/logical_collections.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/bigint_set.rs",
        include_str!("../expr/bigint_set.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/property_get/map_set.rs",
        include_str!("../lower_call/property_get/map_set.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/objects_arrays_lit.rs",
        include_str!("../expr/objects_arrays_lit.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_literal.rs",
        include_str!("../expr/array_literal.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/object_literal.rs",
        include_str!("../expr/object_literal.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_push.rs",
        include_str!("../expr/array_push.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get.rs",
        include_str!("../expr/index_get.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get/guarded_array.rs",
        include_str!("../expr/index_get/guarded_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get/inline_dyn_typed_array.rs",
        include_str!("../expr/index_get/inline_dyn_typed_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_set.rs",
        include_str!("../expr/index_set.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_set_typed_array.rs",
        include_str!("../expr/index_set_typed_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get.rs",
        include_str!("../expr/property_get.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get/globalget.rs",
        include_str!("../expr/property_get/globalget.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get/helpers.rs",
        include_str!("../expr/property_get/helpers.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_set.rs",
        include_str!("../expr/property_set.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/extern_timers.rs",
        include_str!("../lower_call/extern_timers.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/namespace_call.rs",
        include_str!("../lower_call/namespace_call.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/mod.rs",
        include_str!("../lower_call/mod.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/func_ref.rs",
        include_str!("../lower_call/func_ref.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/console_promise.rs",
        include_str!("../lower_call/console_promise.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/child_proc.rs",
        include_str!("../expr/child_proc.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/proxy_reflect.rs",
        include_str!("../expr/proxy_reflect.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/fs_await.rs",
        include_str!("../expr/fs_await.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/binary.rs",
        include_str!("../expr/binary.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/math_simple.rs",
        include_str!("../expr/math_simple.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/static_field_meta.rs",
        include_str!("../expr/static_field_meta.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/dyn_extern_i18n.rs",
        include_str!("../expr/dyn_extern_i18n.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_string_method.rs",
        include_str!("../lower_string_method.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_string_concat.rs",
        include_str!("../lower_string_concat.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/new.rs",
        include_str!("../lower_call/new.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/new_alloc.rs",
        include_str!("../lower_call/new_alloc.rs"),
    ),
];

/// `rooting/temp_root.rs`, inlined at compile time for the terminal-condition
/// test below.
#[cfg(test)]
const RAW_ROOTING_API_SRC: &str = include_str!("temp_root.rs");

/// The two items in `temp_root.rs` that are allowed to stay `pub(crate)`.
///
/// Neither is an accessor. Adding to this list is how the campaign's terminal
/// condition would be given back, so it is spelled out rather than derived.
#[cfg(test)]
const RAW_API_PUBLIC_EXCEPTIONS: &[&str] = &["struct TempRootPool", "fn expr_is_inert_primitive"];

/// Lines in `src` that reach past [`crate::rooting`] into the raw rooting API.
#[cfg(test)]
fn escape_hatch_uses(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("temp_root") || code.contains("rooted_handle")
        })
        .map(|(i, line)| (i + 1, line.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod migration_ledger {
    use super::{
        escape_hatch_uses, MIGRATED_MODULES, RAW_API_PUBLIC_EXCEPTIONS, RAW_ROOTING_API_SRC,
    };

    /// Every `pub`-ish item declared in `temp_root.rs`, as
    /// `("pub(crate)" | "pub(in crate::rooting)" | "pub", "fn name")`.
    fn declared_items(src: &str) -> Vec<(String, String)> {
        src.lines()
            .filter_map(|line| {
                let code = line.split("//").next().unwrap_or(line).trim_start();
                for vis in ["pub(in crate::rooting) ", "pub(crate) ", "pub "] {
                    if let Some(rest) = code.strip_prefix(vis) {
                        let mut it = rest.split_whitespace();
                        let kind = it.next()?;
                        if !matches!(kind, "fn" | "struct" | "enum" | "mod" | "const" | "type") {
                            return None;
                        }
                        let name = it.next()?.split(['(', '<', '{', ':']).next()?.to_string();
                        return Some((vis.trim().to_string(), format!("{kind} {name}")));
                    }
                }
                None
            })
            .collect()
    }

    /// **The campaign's terminal condition** (#7615): the raw rooting API is
    /// unreachable outside `crate::rooting`, not merely unnamed.
    ///
    /// The ledger above can only report what a module NAMES, which is why this
    /// is a separate assertion rather than a stronger phrasing of that one.
    /// Both halves are checked, because either alone can be undone by one
    /// keyword: the module declaration must stay private, and every accessor
    /// must carry `pub(in crate::rooting)` so re-opening the module does not
    /// silently widen twenty-five items at once.
    #[test]
    fn the_raw_rooting_api_is_unreachable_outside_this_module() {
        let items = declared_items(RAW_ROOTING_API_SRC);
        assert!(
            items.len() > 15,
            "expected temp_root.rs to declare the raw API; found {} items — the \
             include_str! target moved and this check is measuring nothing",
            items.len()
        );
        let widened: Vec<&(String, String)> = items
            .iter()
            .filter(|(vis, item)| {
                vis != "pub(in crate::rooting)" && !RAW_API_PUBLIC_EXCEPTIONS.contains(&&**item)
            })
            .collect();
        assert!(
            widened.is_empty(),
            "the Layer 1 campaign's terminal condition is that every accessor in \
             rooting/temp_root.rs is pub(in crate::rooting). These are not, and \
             are not on the two-item exception list:\n{}",
            widened
                .iter()
                .map(|(vis, item)| format!("  {vis} {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let decl = include_str!("mod.rs");
        assert!(
            decl.lines().any(|line| line == "mod temp_root;"),
            "rooting/temp_root.rs must be declared with a PRIVATE `mod temp_root;` — \
             a pub(crate) module would make every item in it reachable crate-wide \
             regardless of its own visibility"
        );
    }

    /// Sabotage duty for the terminal-condition check: a widened accessor must
    /// be reported, and the two allowed exceptions must not be.
    #[test]
    fn the_terminal_condition_check_reports_a_widened_accessor() {
        let planted = "\
pub(crate) fn temp_root_push_i64(ctx: &mut FnCtx<'_>, v: &str) -> String {}
pub(in crate::rooting) fn temp_root_truncate(ctx: &mut FnCtx<'_>, idx: &str) {}
pub(crate) struct TempRootPool {}
pub(crate) fn expr_is_inert_primitive(ctx: &FnCtx<'_>, e: &Expr) -> bool {}
";
        let items = declared_items(planted);
        assert_eq!(items.len(), 4, "parsed {items:?}");
        let widened: Vec<&(String, String)> = items
            .iter()
            .filter(|(vis, item)| {
                vis != "pub(in crate::rooting)" && !RAW_API_PUBLIC_EXCEPTIONS.contains(&&**item)
            })
            .collect();
        assert_eq!(
            widened.len(),
            1,
            "exactly the widened accessor must be reported, got {widened:?}"
        );
        assert_eq!(widened[0].1, "fn temp_root_push_i64");
    }

    /// An empty ledger passes vacuously, which is hazard 4 in CLAUDE.md applied
    /// to this test. Assert the subject exists before asserting it is clean.
    #[test]
    fn the_ledger_is_not_empty() {
        assert!(
            !MIGRATED_MODULES.is_empty(),
            "the Layer 1 ledger is empty; a clean verdict over nothing is not a check"
        );
    }

    #[test]
    fn migrated_modules_do_not_reach_past_the_rooting_api() {
        for (path, src) in MIGRATED_MODULES {
            let hits = escape_hatch_uses(src);
            assert!(
                hits.is_empty(),
                "{path} has completed the Layer 1 migration, so it must root only \
                 through crate::rooting. Reaching back into expr::temp_root \
                 restores the ordering hazard the migration removed:\n{}",
                hits.iter()
                    .map(|(n, l)| format!("  {path}:{n}: {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    /// Sabotage duty: a ledger that cannot report a violation is documentation.
    /// Plant each escape-hatch spelling and require the checker to name it.
    #[test]
    fn the_ledger_check_still_reports_a_planted_violation() {
        let planted = "\
fn lower(ctx: &mut FnCtx<'_>) {
    let p = ctx.block().call(I64, \"js_url_coerce_string\", &[]);
    let slot = super::temp_root::temp_root_push_i64(ctx, &p);
    let h = super::temp_root::rooted_handle_begin(ctx, &p, true);
}
";
        let hits = escape_hatch_uses(planted);
        assert_eq!(
            hits.len(),
            2,
            "planted escape-hatch uses must be reported, got {hits:?}"
        );
        assert!(hits[0].1.contains("temp_root_push_i64"));
        assert!(hits[1].1.contains("rooted_handle_begin"));
    }

    /// ...and must NOT report the migrated form, or the check would make the
    /// migration impossible to finish.
    #[test]
    fn the_ledger_check_clears_the_migrated_form() {
        let clean = "\
fn lower(ctx: &mut FnCtx<'_>) {
    let slot = crate::rooting::call_rooted(ctx, I64, \"js_url_coerce_string\", &[]);
    let obj = crate::rooting::call_with_roots(ctx, I64, \"js_url_new\", &[Arg::Root(&slot)]);
    slot.release(ctx);
}
";
        assert!(escape_hatch_uses(clean).is_empty());
    }
}
