//! Tests for `--emit-types` (#7685).
//!
//! The load-bearing ones are the **omission** tests. This module's whole claim
//! is "never emit a wrong type", and a test suite that only checks the happy
//! path cannot tell a working omission rule from a deleted one — so every
//! omission below is paired with a positive control proving the same input
//! *would* have emitted something if the rule were absent.
//!
//! [`emitting`] is that control throughout. It is a `Ptr<Shape>` row because
//! after the soundness audit that is the *only* analysis which licenses a
//! TypeScript type; an earlier version of these tests used a canonical-`I32`
//! row as the control, which stopped emitting when that arm was withdrawn.

use super::*;
use crate::opt_report::{Analysis, Entry, Outcome, Position, RegionKind};

/// A `Selected` local entry, the shape every test varies from.
fn local(name: &str, analysis: Analysis, rep: &str) -> Entry {
    Entry {
        module: "m.ts".to_string(),
        function: "f".to_string(),
        region: RegionKind::Function,
        position: Position::Local,
        name: name.to_string(),
        local_id: Some(1),
        analysis,
        outcome: Outcome::Selected,
        rep: rep.to_string(),
        rule: None,
        reason: None,
        tier: None,
        issue: None,
        loop_depth: 0,
        invoked_per_element: None,
        detail: None,
        byte_offset: None,
        site: None,
        alloc_context: None,
        alloc_ordinal: None,
        shape_class: None,
        shape_fields: None,
    }
}

/// The one row shape that DOES emit: a `Ptr<Shape>` local of a source class.
fn emitting(name: &str) -> Entry {
    let mut e = local(name, Analysis::PtrShape, "Ptr<Shape>");
    e.shape_class = Some("Point".to_string());
    e
}

fn types_of(entries: &[Entry]) -> Vec<(String, String)> {
    recover(entries)
        .0
        .into_iter()
        .map(|r| (r.name, r.ts_type))
        .collect()
}

// ── The one analysis that licenses a type ──────────────────────────────────

#[test]
fn a_ptr_shape_local_recovers_its_source_class_name() {
    assert_eq!(
        types_of(&[emitting("p")]),
        vec![("p".to_string(), "Point".to_string())]
    );
}

/// The four analyses that look like obvious mappings and are not.
///
/// Each is a *storage* proof rather than a value proof, and each was emitted by
/// the first version of this module. They are pinned as omissions so that
/// re-adding one turns this test red rather than quietly widening the output.
/// The reason per analysis is in `recovered_type`; the short version:
///
/// * `I32`/`U32` — the local's JS value can be `undefined` (non-dominating
///   writes to a `var` seed) or `bigint` (the bitwise arm ignores operand type).
/// * `Str` — derived from the annotation, and the representation explicitly
///   tolerates that annotation being false.
/// * `IntValued` — sound only because the value is never observed; an
///   annotation observes it.
/// * `Ptr<NumArray>` — `HolesOk` elements read back as `undefined`, which
///   `test-files/test_gap_repsel_p4a3_ptr_numarray.ts` pins as observable.
#[test]
fn storage_proofs_are_not_value_proofs_and_emit_nothing() {
    for (analysis, rep) in [
        (Analysis::CanonicalSlot, "I32"),
        (Analysis::CanonicalSlot, "U32"),
        (Analysis::CanonicalSlot, "Str"),
        (Analysis::IntValuedTa, "IntValued"),
        (Analysis::PtrNumArray, "Ptr<NumArray>"),
    ] {
        let e = local("x", analysis, rep);
        assert!(
            types_of(&[e]).is_empty(),
            "{analysis:?}/{rep} must not emit a type"
        );
    }
    // Control: the analysis that DOES license a type still emits, so the
    // omissions above are per-analysis and not a dead code path.
    assert_eq!(types_of(&[emitting("p")]).len(), 1);
}

/// A spec-ABI tuple is the most frequent argument-type tuple across a
/// function's call sites (`spec_abi::select_dominant_tuple`), with disagreeing
/// callers demoted to a guarded boxed entry. A function called four times with
/// numbers and once with a string still reports `i32,i32` — measured — so the
/// tuple is a majority, not a proof, and cannot license a parameter type.
#[test]
fn spec_abi_is_dropped_because_a_majority_of_call_sites_is_not_a_proof() {
    let e = local("n", Analysis::SpecAbi, "i32");
    assert!(types_of(&[e]).is_empty());
    assert_eq!(types_of(&[emitting("n")]).len(), 1);
}

/// An unrecognised representation must be an omission, never `any`.
#[test]
fn an_unrecognised_representation_is_omitted_rather_than_any() {
    let mut e = local("mystery", Analysis::PtrShape, "Ptr<SomethingNew>");
    e.shape_class = None;
    assert!(types_of(&[e]).is_empty());
    assert_eq!(types_of(&[emitting("mystery")]).len(), 1);
}

// ── Outcome and position filters ───────────────────────────────────────────

#[test]
fn a_denied_entry_contributes_nothing() {
    let mut e = emitting("x");
    e.outcome = Outcome::Denied;
    e.rep = "Boxed".to_string();
    assert!(types_of(&[e]).is_empty());
}

/// A proof codegen threw away is not evidence this module can act on: nothing
/// in the stream distinguishes "dropped because unreachable" from "dropped
/// because refuted".
#[test]
fn an_unconsumed_proof_is_not_emitted() {
    let mut e = emitting("x");
    e.outcome = Outcome::Unconsumed;
    assert!(types_of(&[e]).is_empty());
    let mut consumed = emitting("x");
    consumed.outcome = Outcome::Consumed;
    assert_eq!(types_of(&[consumed]).len(), 1, "Consumed must still emit");
}

#[test]
fn non_local_positions_are_not_emitted() {
    for position in [Position::Param, Position::Return, Position::AllocSite] {
        let mut e = emitting("x");
        e.position = position;
        assert!(
            types_of(&[e]).is_empty(),
            "position {position:?} must not emit"
        );
    }
}

#[test]
fn synthetic_binding_names_are_not_emitted() {
    for name in ["<local 7>", "(parameters + return)", "this"] {
        assert!(
            types_of(&[emitting(name)]).is_empty(),
            "{name} must not emit"
        );
    }
}

// ── Synthetic class names ──────────────────────────────────────────────────

/// A compiler-minted class name is not a TypeScript type. Without a field set
/// to turn into structure the binding is omitted — never emitted under a name
/// that would name nothing in the source.
///
/// The `$` cases are the ones an earlier prefix-only filter let through:
/// generic monomorphization rewrites the provenance `New` to `Box$num`, and a
/// scope collision renames a class to `Name$2`. Both are real class names
/// reachable here, and both would have produced TypeScript that does not
/// compile.
#[test]
fn compiler_minted_class_names_are_never_emitted() {
    for class in [
        "__AnonShape_1f2e",
        "__anon_class_7",
        "Box$num",
        "Point$2",
        "Target__inline_3_1",
    ] {
        let mut e = emitting("o");
        e.shape_class = Some(class.to_string());
        assert!(
            types_of(&[e.clone()]).is_empty(),
            "{class} must not be emitted as a type"
        );
        let ts = render_ts(&[e]);
        assert!(!ts.contains(class), "{class} leaked into:\n{ts}");
    }
    // Control: a source-level class name emits.
    assert_eq!(types_of(&[emitting("o")]).len(), 1);
}

// ── Structural interfaces ──────────────────────────────────────────────────

fn shape_entry(name: &str, fields: &[(&str, Option<&str>)]) -> Entry {
    let mut e = local(name, Analysis::PtrShape, "Ptr<Shape>");
    e.shape_class = Some("__AnonShape_aa".to_string());
    e.shape_fields = Some(
        fields
            .iter()
            .map(|(n, t)| ShapeField {
                name: n.to_string(),
                ts_type: t.map(str::to_string),
            })
            .collect(),
    );
    e
}

#[test]
fn an_anon_shape_becomes_a_structural_interface() {
    let e = shape_entry("rec", &[("x", Some("number")), ("tag", Some("string"))]);
    let ts = render_ts(&[e.clone()]);
    assert!(
        ts.contains("export interface PerryShape1 {\n  x: number;\n  tag: string;\n}"),
        "unexpected output:\n{ts}"
    );
    assert_eq!(
        types_of(&[e]),
        vec![("rec".to_string(), "PerryShape1".to_string())]
    );
}

/// A partial interface is a WRONG type, not a smaller one: `{ a: number }` for
/// an object that also has `b` makes `o.b` an error at every call site.
#[test]
fn a_shape_with_one_untyped_field_is_refused_whole() {
    let e = shape_entry("rec", &[("x", Some("number")), ("b", None)]);
    assert!(types_of(&[e.clone()]).is_empty());
    let ts = render_ts(&[e]);
    assert!(!ts.contains("interface"), "unexpected interface:\n{ts}");
    // Control: type that field and the same shape emits.
    let ok = shape_entry("rec", &[("x", Some("number")), ("b", Some("string"))]);
    assert_eq!(types_of(&[ok]).len(), 1);
}

#[test]
fn an_empty_field_set_is_not_an_empty_interface() {
    assert!(types_of(&[shape_entry("rec", &[])]).is_empty());
}

/// Two bindings with the same field set share one interface, and the numbering
/// is first-appearance order so the document is reproducible.
#[test]
fn identical_shapes_are_deduplicated_and_numbered_deterministically() {
    let a = shape_entry("a", &[("x", Some("number"))]);
    let mut b = shape_entry("b", &[("x", Some("number"))]);
    b.local_id = Some(2);
    let mut c = shape_entry("c", &[("y", Some("string"))]);
    c.local_id = Some(3);
    let (recovered, shapes) = recover(&[a, b, c]);
    assert_eq!(shapes.len(), 2, "identical field sets must share one shape");
    let named: Vec<_> = recovered.iter().map(|r| r.ts_type.as_str()).collect();
    assert_eq!(named, vec!["PerryShape1", "PerryShape1", "PerryShape2"]);
}

// ── Disagreement ───────────────────────────────────────────────────────────

/// Note: this guard cannot fire on a real compile — `take_entries` de-duplicates
/// upstream on a key that omits `rep` and `shape_class`, so the second row is
/// destroyed before this module sees it. See `recover`'s doc comment. The tests
/// drive `recover` directly, which is the only place the behaviour is
/// observable.
#[test]
fn a_binding_two_entries_disagree_about_is_dropped() {
    let a = emitting("x");
    let mut b = emitting("x");
    b.shape_class = Some("Other".to_string());
    assert!(types_of(&[a.clone(), b.clone()]).is_empty());
    assert!(
        types_of(&[b, a]).is_empty(),
        "order must not decide a conflict"
    );
}

#[test]
fn agreeing_duplicate_entries_collapse_to_one_row() {
    let a = emitting("x");
    let mut b = emitting("x");
    b.outcome = Outcome::Consumed;
    assert_eq!(
        types_of(&[a, b]),
        vec![("x".to_string(), "Point".to_string())]
    );
}

/// A third row cannot revive a binding an earlier conflict poisoned.
#[test]
fn a_poisoned_binding_stays_poisoned() {
    let a = emitting("x");
    let mut b = emitting("x");
    b.shape_class = Some("Other".to_string());
    let c = emitting("x");
    assert!(types_of(&[a, b, c]).is_empty());
}

// ── HIR type mapping ───────────────────────────────────────────────────────

#[test]
fn hir_types_map_only_where_they_prove_a_typescript_type() {
    use perry_hir::types::Type;
    assert_eq!(
        ts_type_for_hir_type(&Type::Number).as_deref(),
        Some("number")
    );
    assert_eq!(
        ts_type_for_hir_type(&Type::Int32).as_deref(),
        Some("number")
    );
    assert_eq!(
        ts_type_for_hir_type(&Type::String).as_deref(),
        Some("string")
    );
    assert_eq!(
        ts_type_for_hir_type(&Type::Boolean).as_deref(),
        Some("boolean")
    );
    assert_eq!(
        ts_type_for_hir_type(&Type::Array(Box::new(Type::Number))).as_deref(),
        Some("number[]")
    );
    assert_eq!(
        ts_type_for_hir_type(&Type::Named("Point".into())).as_deref(),
        Some("Point")
    );
    // The omissions.
    assert_eq!(ts_type_for_hir_type(&Type::Any), None);
    assert_eq!(ts_type_for_hir_type(&Type::Unknown), None);
    assert_eq!(
        ts_type_for_hir_type(&Type::Union(vec![Type::Number, Type::String])),
        None
    );
    // A compiler-minted class is not a nameable type even inside a field —
    // including the `$`-mangled monomorphization form.
    assert_eq!(
        ts_type_for_hir_type(&Type::Named("__AnonShape_1".into())),
        None
    );
    assert_eq!(ts_type_for_hir_type(&Type::Named("Box$num".into())), None);
    // An array of an unmappable element is unmappable, not `any[]`.
    assert_eq!(
        ts_type_for_hir_type(&Type::Array(Box::new(Type::Any))),
        None
    );
}

// ── Rendering ──────────────────────────────────────────────────────────────

/// An empty result must say so. "No recoverable types" and "the tool did not
/// run" produce very different follow-up actions and must not look alike.
#[test]
fn an_empty_program_renders_an_explicit_statement_not_a_blank_file() {
    let ts = render_ts(&[]);
    assert!(ts.contains("No binding in this program had a recoverable type."));
    assert!(ts.contains("EXPERIMENTAL"));
}

#[test]
fn the_header_states_the_coverage_caveat_and_the_parameter_limitation() {
    let ts = render_ts(&[emitting("p")]);
    assert!(ts.contains("EXPERIMENTAL"));
    assert!(
        ts.contains("Parameter and return types are NOT recovered"),
        "the artifact must carry its own limitation"
    );
    assert!(ts.contains("absent from this file is NOT untyped"));
    assert!(
        ts.contains("Only ONE of Perry's five representation analyses"),
        "the artifact must state how narrow it is"
    );
}

#[test]
fn json_and_ts_agree_on_what_was_recovered() {
    let entries = vec![emitting("p"), shape_entry("rec", &[("x", Some("number"))])];
    let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
    assert_eq!(json["summary"]["recovered_bindings"], 2);
    assert_eq!(json["summary"]["recovered_shapes"], 1);
    assert_eq!(json["experimental"], true);
    let ts = render_ts(&entries);
    assert!(ts.contains("PerryShape1"));
    assert!(ts.contains("p: Point;"));
}
