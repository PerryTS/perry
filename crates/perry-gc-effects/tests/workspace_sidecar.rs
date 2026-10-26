//! Pinned facts about the real runtime, checked over a freshly built
//! workspace sidecar (GC effects design §5). Nothing is checked in: perry
//! reads the sidecar of the archive it links, so a runtime change that taints
//! a hot helper, or undoes #11522/#11523, shows up here, in the table codegen
//! will read, and nowhere else.
//!
//! Needs built archives and their sidecar, so it is `#[ignore]`d by default:
//!
//!     scripts/gc_call_effects/sidecars.sh target/release
//!     PERRY_GC_EFFECTS_SIDECAR=target/release/libperry_runtime.a.gc-effects \
//!         cargo test -p perry-gc-effects --test workspace_sidecar -- --ignored
//!
//! When a fact goes red, `perry-gc-effects why --archive <runtime.a>
//! --archive <stdlib.a> <symbol>` prints what now taints it.

use perry_gc_effects::sidecar::{archive_key, combination_name, Sidecar};
use perry_gc_effects::{Class, Table};
use std::path::{Path, PathBuf};

/// The table of the sidecar `PERRY_GC_EFFECTS_SIDECAR` names, after checking
/// it still describes the archives beside it: the `runtime+stdlib`
/// combination (what a program that uses the stdlib links), or the runtime
/// alone when the directory has no stdlib archive.
fn table() -> Table {
    let path = PathBuf::from(std::env::var("PERRY_GC_EFFECTS_SIDECAR").expect(
        "PERRY_GC_EFFECTS_SIDECAR must name a workspace sidecar \
         (target/release/libperry_runtime.a.gc-effects); this test never passes without one",
    ));
    let sidecar = Sidecar::read(&path).expect("read the sidecar");
    let section = [combination_name(true), combination_name(false)]
        .into_iter()
        .find_map(|name| sidecar.combinations.iter().find(|c| c.name == name))
        .unwrap_or_else(|| panic!("{} has no runtime combination", path.display()));
    let combination = section.name.as_str();
    eprintln!("{} [combination {combination}]", path.display());
    let dir = path.parent().unwrap_or(Path::new("."));
    let archives: Vec<PathBuf> = section.archives.iter().map(|k| dir.join(&k.name)).collect();
    let keys: Vec<_> = archives
        .iter()
        .map(|a| archive_key(a).unwrap_or_else(|e| panic!("{}: {e:#}", a.display())))
        .collect();
    let pairs: Vec<(&Path, _)> = archives
        .iter()
        .map(PathBuf::as_path)
        .zip(keys.iter())
        .collect();
    sidecar
        .verified_table(combination, &pairs)
        .unwrap_or_else(|m| panic!("{} is stale: {m}", path.display()))
        .clone()
}

fn class(t: &Table, name: &str) -> Class {
    t.get(name).map_or(Class::Reenters, |e| e.class)
}
fn throws(t: &Table, name: &str) -> bool {
    t.get(name).is_none_or(|e| e.throws)
}
/// What codegen makes of a class: only `Leaf` is a GC leaf; `AllocOnly` is a
/// leaf only under the research-only safepoint-only contract.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    CannotCollect,
    AllocNoReentry,
    Unknown,
}
fn effect(t: &Table, name: &str) -> Effect {
    match class(t, name) {
        Class::Leaf => Effect::CannotCollect,
        Class::AllocOnly => Effect::AllocNoReentry,
        Class::ThrowOnly | Class::Reenters => Effect::Unknown,
    }
}

/// #11522: `js_array_length`'s Proxy arm runs the `get` trap and a `valueOf`
/// coercion; the other two reach JS or the collector too. The hand audit
/// called them leaf / alloc-no-reentry; the graph proves they are neither.
/// (`js_object_get_own_field_or_undef` was the fourth: its only path to JS was
/// the keys-array length fallback of #12743, so it is a pinned leaf now.)
const ISSUE_11522: &[&str] = &[
    "js_array_length",
    "js_object_alloc_class_inline_keys",
    "js_object_alloc_class_inline_keys_stamped",
];

/// #11523: these take the typed-feedback registry's root lock. With an
/// ordinary `GcRootRegistryGuard` the release can flush a deferred
/// collection. The registry uses `NonCollectingRootRegistryGuard`, whose drop
/// has no path to `flush_deferred_gc_request`, so the table proves them Leaf
/// again, except six that reenter on their own paths.
const ISSUE_11523: &[&str] = &[
    "js_gc_note_slot_layout",
    "js_gc_note_slot_layout_aware",
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

/// Helpers the graph proves `Leaf`. Pinned by name because they carry most of
/// the leaf-marked call sites; a runtime change that taints one of them turns
/// this red, which is the moment to run `why`, not after a regression.
/// `js_box_alloc_bits` is deliberately not here: #11179 made box allocation
/// reach `gc_try_emergency_reclaim`, so it is (correctly) `AllocOnly`.
const PINNED_LEAF: &[&str] = &[
    "js_nanbox_pointer",
    "js_nanbox_get_pointer",
    "js_is_truthy",
    "js_string_compare",
    "js_string_addref",
    "js_gc_temp_root_push",
    "js_write_barrier",
    "js_write_barrier_root_nanbox",
    "js_gc_register_global_root",
    "js_object_get_class_id",
    "js_closure_get_capture_bits",
    "js_closure_exact_func_guard",
    "js_box_set_bits",
    "js_i32_box_get",
    "js_box_release",
    "js_box_scope_release",
    "js_tdz_suppress_begin",
    "js_tdz_suppress_end",
    // #11522's split: the plain-array fast lane; RFC S2's IC hit (the store
    // probe; #12743: its keys-array length fallback must not reach the
    // generic, JS-capable `js_array_length`).
    "js_array_length_leaf",
    "perry_probe_set",
    // #9480's own-field dispatch probe; it walks a shape's keys array the
    // same way.
    "js_object_get_own_field_or_undef",
];

#[test]
#[ignore = "needs built archives: set PERRY_GC_EFFECTS_SIDECAR (see the module docs)"]
fn workspace_sidecar_pins_the_runtime_facts_codegen_relies_on() {
    let t = table();
    let leaves = t.values().filter(|e| e.class == Class::Leaf).count();
    assert!(
        t.len() >= 1000 && leaves >= 200,
        "{} symbols, {leaves} Leaf: a vacuous table would make every helper a safepoint",
        t.len()
    );

    for name in PINNED_LEAF {
        assert_eq!(class(&t, name), Class::Leaf, "{name} is no longer Leaf");
    }

    // Leaf and AllocOnly reach no throw seed, so they can never throw.
    for (sym, e) in &t {
        if matches!(e.class, Class::Leaf | Class::AllocOnly) {
            assert!(!e.throws, "{sym}: {:?} but throws=yes", e.class);
        }
    }

    for name in ISSUE_11522 {
        assert_eq!(effect(&t, name), Effect::Unknown, "#11522: {name}");
    }
    let mut leaf = 0;
    for name in ISSUE_11523 {
        assert_ne!(
            class(&t, name),
            Class::AllocOnly,
            "#11523: {name} reaches a collector again (a flushing root lock?)"
        );
        leaf += usize::from(class(&t, name) == Class::Leaf);
    }
    assert_eq!(leaf, ISSUE_11523.len() - 6, "#11523 Leaf census changed");

    // The native shadow-stack helpers were removed (#12023): no export, no
    // leaf claim.
    for name in [
        "js_shadow_frame_enter",
        "js_shadow_frame_push",
        "js_shadow_frame_pop",
        "js_shadow_state_addr",
        "js_shadow_slot_bind",
        "js_shadow_slot_get",
        "js_shadow_slot_set",
    ] {
        assert!(!t.contains_key(name), "{name} is exported again");
    }

    // Calls that throw: user code, JS entry, coercion, the read entries
    // behind the probe (they run getters).
    for name in [
        "js_throw",
        "js_closure_call0",
        "js_native_call_value",
        "js_to_primitive",
        "js_number_coerce",
        "js_object_get_field_slow",
        "js_object_get_field_value",
        "user_function_not_in_any_archive",
    ] {
        assert!(throws(&t, name), "{name} must be throws=yes");
    }
    // Declared polls are `Reenters` (a collection can begin) yet cannot throw.
    for name in ["js_gc_safepoint", "js_gc_loop_safepoint", "js_gc_collect"] {
        assert_eq!(class(&t, name), Class::Reenters, "{name}");
        assert!(!throws(&t, name), "{name} is a poll and cannot throw");
    }

    // Helpers that allocate, run JS or throw are never a GC leaf.
    // `js_nanbox_string`'s null guard allocates an empty string; the TDZ box
    // getters throw a ReferenceError; the sloppy `this` reader boxes
    // primitives.
    for name in [
        "js_nanbox_string",
        "js_string_from_bytes",
        "js_array_alloc",
        "js_string_compare_value",
        "js_rel_lt",
        "js_rel_gt",
        "js_object_get_field_slow",
        "js_object_get_field_value",
        "js_object_get_field_nonptr",
        "js_this_coerce_sloppy",
        "js_box_get_bits",
        "js_box_get_bits_trusted",
        "js_box_get_bits_named",
        "js_box_get_bits_trusted_named",
        "js_value_length_f64",
        "js_value_length_property_f64",
        "js_value_length_property_key_f64",
        "js_array_get_f64",
    ] {
        assert_ne!(effect(&t, name), Effect::CannotCollect, "{name}");
    }
    // Polls and calls that can re-enter JS: never a leaf, never AllocOnly.
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
        "js_put_value_set",
    ] {
        assert_eq!(effect(&t, name), Effect::Unknown, "{name}");
    }

    // js_gc_register_global_root's entire body is that barrier plus a TLS
    // Vec::push; they cannot have different GC effects.
    assert_eq!(
        effect(&t, "js_gc_register_global_root"),
        effect(&t, "js_write_barrier_root_heap_word")
    );
}

/// The box/closure family's containment in the root-dominance checker's
/// `NONCOLLECTING` authority: every member the table proves `Leaf` must be
/// there, so codegen never leaf-marks a call the checker would call
/// collecting (the #7510 drift). Containment is required in the safe
/// direction only.
#[test]
#[ignore = "needs built archives: set PERRY_GC_EFFECTS_SIDECAR (see the module docs)"]
fn leaf_box_and_closure_helpers_are_in_the_checker_authority() {
    let t = table();
    let py_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/gc_root_dominance_check.py"
    );
    let py_src = std::fs::read_to_string(py_path).expect("read gc_root_dominance_check.py");
    let noncollecting = extract_python_set(&py_src, "NONCOLLECTING");
    assert!(
        noncollecting.len() > 50,
        "parsed only {} NONCOLLECTING names: the extraction broke",
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
    .filter(|n| class(&t, n) == Class::Leaf)
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

/// A symbol codegen defines itself (`OVERRIDES` in `gc_call_effects.rs`) must
/// not be exported by the archive, whose row it would shadow. (Codegen's
/// `nounwind` needs no such check: it is read from this table.)
#[test]
#[ignore = "needs built archives: set PERRY_GC_EFFECTS_SIDECAR (see the module docs)"]
fn codegen_defined_symbols_are_not_exported_by_the_archive() {
    let t = table();
    let codegen = concat!(env!("CARGO_MANIFEST_DIR"), "/../perry-codegen/src");
    let effects = std::fs::read_to_string(format!("{codegen}/gc_call_effects.rs"))
        .expect("read perry-codegen/src/gc_call_effects.rs");
    let overrides: Vec<&str> = extract_rust_str_slice(&effects, "OVERRIDES")
        .into_iter()
        .filter(|n| !n.contains(' '))
        .collect();
    assert!(
        !overrides.is_empty(),
        "parsed no OVERRIDES: the extraction broke"
    );
    for name in overrides {
        assert!(
            !t.contains_key(name),
            "the archive exports codegen-defined {name}"
        );
    }
}

/// The string literals inside a top-level `const NAME: &[...] = &[ ... ];`
/// (comments skipped).
fn extract_rust_str_slice<'a>(src: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        if !inside {
            let l = line.trim_start();
            let l = l.strip_prefix("pub(crate) ").unwrap_or(l);
            if l.starts_with(&format!("const {name}: ")) {
                inside = true;
            }
            continue;
        }
        if line.starts_with("];") {
            break;
        }
        let code = line.split("//").next().unwrap_or("");
        out.extend(quoted_literals(code));
    }
    out
}

/// The string literals of a top-level `NAME = {...}` python set.
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

/// Literal publication is part of allocation, with no JS callback boundary.
/// This pins the real linked archive, rather than only the unit-test world.
#[test]
#[ignore = "requires freshly built workspace archives and sidecar"]
fn literal_layout_birth_is_allocation_without_user_reentry() {
    let t = table();
    let name = "perry_alloc_literal_layout";
    assert!(t.get(name).is_some(), "the new ABI entry must be present");
    assert_eq!(effect(&t, name), Effect::AllocNoReentry);
    assert!(!throws(&t, name));
}
