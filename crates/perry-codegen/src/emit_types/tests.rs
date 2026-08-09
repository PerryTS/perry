//! Tests for `--emit-types` (#7685).
//!
//! The load-bearing ones are the **omission** tests. This module's whole claim
//! is "never emit a wrong type", and a test suite that only checks the happy
//! path cannot tell a working omission rule from a deleted one — so every
//! omission below is paired with a positive control proving the same input
//! *would* have emitted something if the rule were absent.

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

fn types_of(entries: &[Entry]) -> Vec<(String, String)> {
    recover(entries)
        .0
        .into_iter()
        .map(|r| (r.name, r.ts_type))
        .collect()
}

// ── The mapping ────────────────────────────────────────────────────────────

#[test]
fn canonical_slots_map_to_number_and_string() {
    let entries = vec![
        local("i", Analysis::CanonicalSlot, "I32"),
        local("u", Analysis::CanonicalSlot, "U32"),
        local("s", Analysis::CanonicalSlot, "Str"),
    ];
    let mut got = types_of(&entries);
    got.sort();
    assert_eq!(
        got,
        vec![
            ("i".to_string(), "number".to_string()),
            ("s".to_string(), "string".to_string()),
            ("u".to_string(), "number".to_string()),
        ]
    );
}

#[test]
fn numarray_and_int_valued_map_to_number_forms() {
    let entries = vec![
        local("xs", Analysis::PtrNumArray, "Ptr<NumArray>"),
        local("acc", Analysis::IntValuedTa, "IntValued"),
    ];
    let mut got = types_of(&entries);
    got.sort();
    assert_eq!(
        got,
        vec![
            ("acc".to_string(), "number".to_string()),
            ("xs".to_string(), "number[]".to_string()),
        ]
    );
}

#[test]
fn a_ptr_shape_local_recovers_its_source_class_name() {
    let mut e = local("p", Analysis::PtrShape, "Ptr<Shape>");
    e.shape_class = Some("Point".to_string());
    assert_eq!(types_of(&[e]), vec![("p".to_string(), "Point".to_string())]);
}

/// An unrecognised representation must be an omission, never `any`.
///
/// `any` is a claim — it tells `tsc` the binding may be used as anything, which
/// is exactly what an unknown proof does not license.
#[test]
fn an_unrecognised_representation_is_omitted_rather_than_any() {
    let e = local("mystery", Analysis::CanonicalSlot, "F128");
    assert!(types_of(&[e]).is_empty());
    // Control: the same row with a KNOWN rep does emit, so the omission above
    // is the rep check and not a dead code path swallowing everything.
    let ok = local("mystery", Analysis::CanonicalSlot, "I32");
    assert_eq!(types_of(&[ok]).len(), 1);
}

// ── The omissions ──────────────────────────────────────────────────────────

/// Spec-ABI reps are derived from `param.ty`, which is populated from the
/// source annotation and by nothing else. Emitting them would echo the input
/// back and inflate any accuracy measurement to meaninglessness.
#[test]
fn spec_abi_is_dropped_because_it_restates_the_source_annotation() {
    let mut e = local("n", Analysis::SpecAbi, "i32");
    e.position = Position::Local;
    assert!(types_of(&[e]).is_empty());
    // Control: an identical row under an analysis that really does prove
    // something emits, so the drop is keyed on the analysis.
    let ok = local("n", Analysis::CanonicalSlot, "I32");
    assert_eq!(types_of(&[ok]).len(), 1);
}

#[test]
fn a_denied_entry_contributes_nothing() {
    let mut e = local("x", Analysis::CanonicalSlot, "Boxed");
    e.outcome = Outcome::Denied;
    assert!(types_of(&[e]).is_empty());
}

/// A proof codegen threw away is not evidence this module can act on: nothing
/// in the stream distinguishes "dropped because unreachable" from "dropped
/// because refuted".
#[test]
fn an_unconsumed_proof_is_not_emitted() {
    let mut e = local("x", Analysis::CanonicalSlot, "I32");
    e.outcome = Outcome::Unconsumed;
    assert!(types_of(&[e]).is_empty());
    let mut consumed = local("x", Analysis::CanonicalSlot, "I32");
    consumed.outcome = Outcome::Consumed;
    assert_eq!(types_of(&[consumed]).len(), 1, "Consumed must still emit");
}

#[test]
fn non_local_positions_are_not_emitted() {
    for position in [Position::Param, Position::Return, Position::AllocSite] {
        let mut e = local("x", Analysis::CanonicalSlot, "I32");
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
        let e = local(name, Analysis::CanonicalSlot, "I32");
        assert!(types_of(&[e]).is_empty(), "{name} must not emit");
    }
}

/// A synthesized class name is not a TypeScript type. Without a field set to
/// turn into structure, the binding is omitted — never emitted as
/// `__AnonShape_1f2e`, which would name nothing in the source.
#[test]
fn a_synthetic_shape_class_is_never_emitted_under_its_synthetic_name() {
    let mut e = local("o", Analysis::PtrShape, "Ptr<Shape>");
    e.shape_class = Some("__AnonShape_1f2e".to_string());
    assert!(types_of(&[e.clone()]).is_empty());
    assert!(!render_ts(&[e]).contains("__AnonShape"));
    // Control: the same row with a SOURCE class name emits that name.
    let mut real = local("o", Analysis::PtrShape, "Ptr<Shape>");
    real.shape_class = Some("Point".to_string());
    assert_eq!(types_of(&[real]).len(), 1);
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
    let e = shape_entry("rec", &[]);
    assert!(types_of(&[e]).is_empty());
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

/// A function lowered twice (a boxed entry plus a typed clone) contributes two
/// rows per binding. If they disagree there is no principled winner, so the
/// binding is dropped rather than resolved by declaration order.
#[test]
fn a_binding_two_entries_disagree_about_is_dropped() {
    let a = local("x", Analysis::CanonicalSlot, "I32");
    let b = local("x", Analysis::CanonicalSlot, "Str");
    assert!(types_of(&[a.clone(), b.clone()]).is_empty());
    assert!(
        types_of(&[b, a]).is_empty(),
        "order must not decide a conflict"
    );
}

#[test]
fn agreeing_duplicate_entries_collapse_to_one_row() {
    let a = local("x", Analysis::CanonicalSlot, "I32");
    let mut b = local("x", Analysis::CanonicalSlot, "I32");
    b.outcome = Outcome::Consumed;
    assert_eq!(
        types_of(&[a, b]),
        vec![("x".to_string(), "number".to_string())]
    );
}

/// A third row cannot revive a binding an earlier conflict poisoned.
#[test]
fn a_poisoned_binding_stays_poisoned() {
    let a = local("x", Analysis::CanonicalSlot, "I32");
    let b = local("x", Analysis::CanonicalSlot, "Str");
    let c = local("x", Analysis::CanonicalSlot, "I32");
    assert!(types_of(&[a, b, c]).is_empty());
}

// ── HIR type mapping ───────────────────────────────────────────────────────

#[test]
fn hir_types_map_only_where_they_prove_a_typescript_type() {
    use perry_hir::types::Type;
    assert_eq!(ts_type_for_hir_type(&Type::Number).as_deref(), Some("number"));
    assert_eq!(ts_type_for_hir_type(&Type::Int32).as_deref(), Some("number"));
    assert_eq!(ts_type_for_hir_type(&Type::String).as_deref(), Some("string"));
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
    // A synthesized shape class is not a nameable type even inside a field.
    assert_eq!(
        ts_type_for_hir_type(&Type::Named("__AnonShape_1".into())),
        None
    );
    // An array of an unmappable element is unmappable, not `any[]`.
    assert_eq!(ts_type_for_hir_type(&Type::Array(Box::new(Type::Any))), None);
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
    let ts = render_ts(&[local("i", Analysis::CanonicalSlot, "I32")]);
    assert!(ts.contains("EXPERIMENTAL"));
    assert!(
        ts.contains("Parameter and return types are NOT recovered"),
        "the artifact must carry its own limitation"
    );
    assert!(ts.contains("absent from this file is NOT untyped"));
}

#[test]
fn json_and_ts_agree_on_what_was_recovered() {
    let entries = vec![
        local("i", Analysis::CanonicalSlot, "I32"),
        shape_entry("rec", &[("x", Some("number"))]),
    ];
    let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
    assert_eq!(json["summary"]["recovered_bindings"], 2);
    assert_eq!(json["summary"]["recovered_shapes"], 1);
    assert_eq!(json["experimental"], true);
    let ts = render_ts(&entries);
    assert!(ts.contains("PerryShape1"));
    assert!(ts.contains("i: number;"));
}
