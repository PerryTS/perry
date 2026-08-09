//! `--emit-types` (#7685, **EXPERIMENTAL**): write the representations Perry
//! *proved* back out as TypeScript.
//!
//! `--opt-report` (#6952) surfaces the negative half of representation
//! selection — which values could not be typed, and why. The positive half is
//! computed and then discarded. This module is a second **consumer** of the
//! same [`Entry`] stream: it keeps the wins and renders them as types.
//!
//! Nothing here runs an analysis. It reads what the collectors already
//! recorded, so its coverage is exactly the proof rate `--opt-report` measures
//! and nothing this module does can widen it.
//!
//! ## The headline result: a representation is not a type
//!
//! Perry has five representation analyses. **One of them licenses a TypeScript
//! type.** That is the finding, and it is why this stays experimental:
//!
//! | analysis | emitted? | why |
//! |---|---|---|
//! | `PtrShape` | **yes** | a genuine value proof — exact dynamic class, provenance + containment |
//! | `CanonicalSlot` (`I32`/`U32`) | no | storage proof; the value can still be `undefined` or `bigint` |
//! | `CanonicalSlot` (`Str`) | no | annotation-derived, and *designed* to tolerate a false annotation |
//! | `IntValuedTa` | no | sound only because the value is never observed; annotating it publishes it |
//! | `PtrNumArray` | no | admits holes that read back `undefined`; also only echoes an annotation |
//! | `SpecAbi` | no | reads `param.ty`, which only an annotation populates |
//!
//! The per-arm reasoning, with the source each claim comes from, is in
//! [`recovered_type`]. The pattern is consistent: these representations were
//! chosen to be *observationally equivalent* to the boxed form, which is a
//! weaker property than "the value has this type" — and in three cases the
//! equivalence holds precisely because the value is never observed in a
//! context that could tell the difference. Publishing an annotation is exactly
//! such an observation.
//!
//! ## The one rule
//!
//! **Never emit a wrong type.** A missing annotation costs nothing; a wrong one
//! poisons a downstream `tsc`. Every mapping is an omission unless the
//! representation *proves* the TypeScript type, and the three places where the
//! proof is conditional are omissions rather than guesses:
//!
//! - **A majority is not a proof.** [`Analysis::SpecAbi`] is the trap here, and
//!   it is worth stating precisely because the obvious reading is wrong. A
//!   specialized entry is *not* derived from the parameter's annotation: it
//!   comes from `codegen/spec_abi.rs::select_dominant_tuple`, which counts the
//!   argument-type tuples at the **call sites** and picks the most frequent
//!   one, demoting the rest to `Boxed` behind a guarded entry. So it fires on
//!   completely unannotated JavaScript — measured — and it fires even when a
//!   caller disagrees: a function called four times with numbers and once with
//!   a string still reports `i32,i32`. Emitting `a: number` there would be
//!   flatly wrong, because a real caller passes a string. Spec-ABI entries are
//!   therefore dropped. See [`recovered_type`].
//! - **A binding two entries disagree about** is dropped entirely rather than
//!   resolved by a precedence rule. A function can be lowered more than once (a
//!   boxed entry plus a typed clone) and the two lowerings can select different
//!   representations; picking a winner would be picking which of two proofs to
//!   believe. See [`recover`].
//! - **A compiler-minted class name is not a TypeScript type.** A `Ptr<Shape>`
//!   local whose provenance class is `__AnonShape_*`, `__anon_class_*`, or a
//!   `$`-mangled monomorphization/collision rename has no source-level name to
//!   emit. It becomes a structural interface when the field set is known, and
//!   is omitted when it is not — never emitted under the minted name. See
//!   [`is_synthetic_class`], whose `$` case is the one a prefix-only filter
//!   misses.
//!
//! ## What it cannot do, stated rather than worked around
//!
//! HIR carries binding *names* through lowering but not source *spans*
//! (`opt_report`'s own module doc records this; `Entry::byte_offset` is
//! populated only for `Expr::New`). So this module cannot rewrite a source file
//! to insert `: T` after `let x` — there is no offset to insert at. The
//! locals it recovers are therefore reported, not applied, and the TypeScript
//! it writes is a sidecar rather than a patch.

use crate::opt_report::{Analysis, Entry, Outcome, Position};
use perry_hir::types::Type;
use std::collections::BTreeMap;

/// Class-name forms the compiler mints itself, which therefore name nothing a
/// `.ts` file could refer to. Emitting one would produce TypeScript that does
/// not compile, so each is either turned into structure or omitted.
///
/// The list is the audited set, not a guess — an earlier version carried only
/// the first entry plus a `__EmptySite_` that **matches nothing in the tree**
/// (empty literals also mint `__AnonShape_<hash>`), while three real families
/// leaked straight through:
///
/// * `__AnonShape_<16 hex>` — object literals, incl. `{}`
///   (`perry-hir/src/lower/context.rs`, a content-addressed FNV-1a of the
///   shape key, not a counter).
/// * `__anon_class_<id>` — `new (class {})()`
///   (`perry-hir/src/lower/expr_new/non_ident.rs`).
/// * `__inline_` / `__anon_dup_` — transform-minted specializations
///   (`perry-transform/src/inline/factory_specialize.rs`).
const SYNTHETIC_CLASS_PREFIXES: [&str; 4] =
    ["__AnonShape_", "__anon_class_", "__inline_", "__anon_dup_"];

/// Is this class name compiler-minted rather than source-level?
///
/// The `$` test is the load-bearing one and is deliberately a *substring*
/// check, not a prefix: generic monomorphization rewrites the `New` site's
/// class to `Box$num` (`perry-hir/src/monomorph/mangle.rs`) and a scope
/// collision renames a class to `Name$2`. Both are real class names reachable
/// at a `Ptr<Shape>` provenance site, and both would emit TypeScript naming a
/// type that does not exist. `$` is already this repo's reserved
/// generated-suffix namespace (`collectors/proven_this.rs`), so no source
/// identifier can contain one.
fn is_synthetic_class(name: &str) -> bool {
    name.contains('$')
        || SYNTHETIC_CLASS_PREFIXES.iter().any(|p| name.starts_with(p))
        || name.contains("__inline_")
}

/// A binding name the collectors invented because the source had none. Such a
/// row identifies no source-level binding, so there is nothing to annotate.
fn is_synthetic_binding(name: &str) -> bool {
    name.starts_with('<') || name.starts_with('(') || name == "this"
}

/// Map an HIR type to the TypeScript type it *proves*, or `None` when it proves
/// nothing a `tsc` run would accept as sound.
///
/// Deliberately total and deliberately narrow. Every variant that could widen
/// at runtime, or that names something this module cannot resolve to a
/// TypeScript declaration, returns `None`:
///
/// - [`Type::Any`] / [`Type::Unknown`] prove nothing by construction.
/// - [`Type::Named`] is emitted only when it is not one of the synthesized
///   shape classes; the caller is responsible for having a declaration in
///   scope for it (it is a source-level class, so the source has one).
/// - [`Type::Object`], [`Type::Function`], [`Type::Union`], [`Type::Generic`],
///   [`Type::TypeVar`] and [`Type::Tuple`] are omitted: rendering them
///   faithfully needs structure this module does not carry, and rendering them
///   approximately would break the one rule.
/// - [`Type::Void`] is `undefined`, not `void` — this maps *value* positions,
///   and `void` is not a value type.
pub fn ts_type_for_hir_type(ty: &Type) -> Option<String> {
    Some(match ty {
        Type::Number | Type::Int32 => "number".to_string(),
        Type::String | Type::StringLiteral(_) => "string".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::BigInt => "bigint".to_string(),
        Type::Symbol => "symbol".to_string(),
        Type::Null => "null".to_string(),
        Type::Void => "undefined".to_string(),
        Type::Array(inner) => format!("{}[]", ts_type_for_hir_type(inner)?),
        Type::Named(name) if !is_synthetic_class(name) => name.clone(),
        _ => return None,
    })
}

/// One field of a recovered structural shape.
///
/// Carried on the `Ptr<Shape>` selection entry rather than re-derived here:
/// the class chain that declares these fields lives in the collector
/// (`collectors/ptr_shape.rs`), and this module never sees HIR.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ShapeField {
    pub name: String,
    /// The TypeScript type, when the declared HIR type proved one. `None`
    /// renders as an omitted field rather than `any` — see [`Shape::render`].
    pub ts_type: Option<String>,
}

/// The declared field set of a proven class chain, as this module consumes it.
///
/// Report-only; never consulted by codegen, and called only when
/// `opt_report::enabled()`. It lives here rather than in the collector so that
/// the whole field-to-TypeScript mapping has exactly one home.
///
/// Two field kinds are recorded name-only, with no type, rather than
/// approximated — and because [`Shape::is_emittable`] requires a type for
/// *every* field, either one refuses the whole interface rather than silently
/// narrowing it:
///
/// * a **computed key** (`key_expr: Some(..)`), whose `name` is a synthetic
///   placeholder for HIR identity and not the runtime property name at all;
/// * a **private** field, which is not part of an object's structural type.
fn declared_shape_fields(chain: &[&perry_hir::Class]) -> Vec<ShapeField> {
    let mut out = Vec::new();
    for class in chain {
        for field in &class.fields {
            let ts_type = if field.key_expr.is_some() || field.is_private {
                None
            } else {
                ts_type_for_hir_type(&field.ty)
            };
            out.push(ShapeField {
                name: field.name.clone(),
                ts_type,
            });
        }
    }
    out
}

/// Build the `Ptr<Shape>` payload the `--opt-report` selection carries.
///
/// The collector calls this instead of assembling the struct itself, so the
/// class name and the field mapping are produced in one place and the collector
/// keeps no `--emit-types` state of its own.
pub(crate) fn selected_shape(
    class_name: &str,
    chain: &[&perry_hir::Class],
) -> crate::opt_report::SelectedShape {
    crate::opt_report::SelectedShape {
        class_name: class_name.to_string(),
        fields: declared_shape_fields(chain),
    }
}

/// A structural shape recovered from a `Ptr<Shape>` selection whose provenance
/// class is compiler-synthesized.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shape {
    fields: Vec<ShapeField>,
}

impl Shape {
    /// A shape is emittable only when **every** field resolved to a type.
    ///
    /// A partial interface is a wrong type, not a smaller one: `{ a: number }`
    /// for an object that also has `b` is a claim `tsc` will act on — it makes
    /// `o.b` an error at every call site. Omitting the whole interface costs a
    /// missing annotation; emitting a partial one costs a false diagnostic.
    fn is_emittable(&self) -> bool {
        !self.fields.is_empty() && self.fields.iter().all(|f| f.ts_type.is_some())
    }

    fn render(&self) -> String {
        let body = self
            .fields
            .iter()
            .filter_map(|f| f.ts_type.as_ref().map(|t| format!("  {}: {};", f.name, t)))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{{\n{body}\n}}")
    }
}

/// One recovered binding: a source-level name and the TypeScript type Perry
/// proved for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Recovered {
    pub module: String,
    pub function: String,
    pub name: String,
    pub position: &'static str,
    /// The TypeScript type. For a structural shape this is the generated
    /// interface name, whose declaration is in the same document.
    pub ts_type: String,
    /// Which representation analysis proved it, so a consumer can weigh the
    /// evidence rather than trusting a bare type string.
    pub analysis: &'static str,
    /// The representation the analysis selected, verbatim.
    pub rep: String,
}

/// The TypeScript type an entry proves for its binding, or `None` to omit.
///
/// `None` is the default for everything not listed. A representation this
/// function does not recognise must not become `any` — an unrecognised
/// representation is an unknown proof, and `any` is a claim.
fn recovered_type(entry: &Entry) -> Option<TypeOrShape> {
    // Only wins. `Denied` is the negative half `--opt-report` already renders,
    // and `Unconsumed` means codegen dropped the proof — the proof itself still
    // held, but this module has no way to distinguish "dropped because
    // unreachable" from "dropped because refuted", so it stands down.
    if !matches!(entry.outcome, Outcome::Selected | Outcome::Consumed) {
        return None;
    }
    // v1 recovers locals. `Param`/`Return`/`Field` rows exist in the schema but
    // the only analysis that populates `Param` today is spec-ABI, which is
    // dropped below, and the `this` receiver rows name no source binding.
    if entry.position != Position::Local {
        return None;
    }
    if is_synthetic_binding(&entry.name) {
        return None;
    }
    match entry.analysis {
        // ── The four analyses that do NOT license a TypeScript type ──────────
        //
        // Each of these looks like an obvious mapping and is not one. They are
        // listed explicitly, with the reason, so that re-adding one requires
        // arguing with the reason rather than noticing an absent arm.
        //
        // A MAJORITY VOTE over call sites, not a proof about the parameter.
        // `spec_abi::select_dominant_tuple` counts the argument-type tuples at
        // every call site and keeps the most frequent, demoting disagreeing
        // params to `Boxed` behind a guarded entry. Measured: a function called
        // four times with numbers and once with a string still reports
        // `i32,i32`, so `a: number` would be wrong for a caller that exists.
        // (It is also NOT annotation-derived, which is the natural guess — it
        // fires on wholly unannotated JavaScript.)
        Analysis::SpecAbi => None,
        // `I32`/`U32` are STORAGE proofs, not value proofs, and the local's JS
        // value can leave `number` three ways: the scaffolding seed admits
        // `var x;` with later non-dominating writes (JS reads `undefined`,
        // Perry reads the entry-block `store i32 0`); `int_valued_ta` members
        // are merged straight into `integer_locals` and carry §3's hazard; and
        // the bitwise arm treats every `Binary` as int32-producing regardless
        // of operand type, so `a & b` on BigInts is a `bigint`
        // (`not_bigint_locals` is computed but is not a term in the admission
        // conjunction). `Str` is worse than unproven — it is annotation-derived
        // (`refined_ty` is the declared type verbatim when it is not `Any`, and
        // nothing checks the initializer), and the representation is
        // *designed* to tolerate the annotation being false: "a type-annotation
        // lie degrades to today's behavior" (`expr/slot_rep.rs`). That is the
        // same defect the `SpecAbi` arm above is rejected for.
        //
        // The `Entry` stream carries only `rep: "I32"`, not which admitting set
        // licensed it, so the sound subsets (`loop_bounded_i32_locals`,
        // `unsigned_i32_locals`) cannot be separated out here. Recovering them
        // needs the collector to record provenance.
        Analysis::CanonicalSlot => None,
        // Self-refuting. `collectors/int_valued_ta_locals.rs`'s own module doc:
        // an OOB / negative / fractional typed-array read "yields `undefined`
        // …, NOT an integer", and the representation is sound *only* because
        // rule (2) forbids every context in which `undefined` and an integer
        // are distinguishable. Writing the annotation publishes a value whose
        // safety depends on it never being observed.
        Analysis::IntValuedTa => None,
        // `NumArrayDensity::HolesOk` is the PRIMARY provenance (`new Array(n)`),
        // and its slots read back as `undefined`. The repo pins the observable
        // itself: `test-files/test_gap_repsel_p4a3_ptr_numarray.ts` asserts
        // `console.log(c[0], c[1], c[3])` prints `undefined 2 undefined` for a
        // promoted local, so the true type is `(number | undefined)[]`.
        // Gating on `Dense` would not rescue it — the `IndexSet` arm never
        // compares the index against the length, so `const a = []; a[3] = 1;`
        // is `Dense` while the runtime hole-fills 0..2. And admission already
        // REQUIRES the declared type be `number[]`, so this arm could only ever
        // echo an annotation back.
        Analysis::PtrNumArray => None,
        // ── The one that survives ────────────────────────────────────────────
        Analysis::PtrShape => {
            let class = entry.shape_class.as_deref()?;
            if is_synthetic_class(class) {
                // No source-level name. Recoverable only as structure.
                let fields = entry.shape_fields.clone()?;
                let shape = Shape { fields };
                shape.is_emittable().then_some(TypeOrShape::Shape(shape))
            } else {
                Some(TypeOrShape::Ts(class.to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeOrShape {
    Ts(String),
    Shape(Shape),
}

/// Identity of a source-level binding across the several entries that can
/// describe it (a selection plus one row per consumption site).
type BindingKey = (String, String, String, Option<u32>);

/// Reduce the entry stream to one type per binding, dropping every binding the
/// stream disagrees about. A function lowered twice (a boxed entry plus a typed
/// clone) contributes two rows per binding, and if the two lowerings selected
/// different representations there is no principled winner.
///
/// **This guard cannot currently fire on a real compile, and saying so is the
/// point.** `opt_report::take_entries` de-duplicates *before* any consumer sees
/// the stream, and `Entry::dedup_key` omits `local_id`, `rep`, `shape_class`
/// and `shape_fields` — so two `Selected` rows for one name in one function
/// with **different classes** collapse to whichever arrived first, and the
/// disagreement is destroyed upstream rather than detected here. Two distinct
/// bindings that share a name (`{const r = new A();} {const r = new B();}`)
/// collapse the same way, so a rendered row can name an ambiguous binding.
///
/// The guard is kept because it is the correct behaviour for the stream this
/// module is handed, and because the unit tests exercise it directly. But it is
/// a guard whose subject never arrives — CLAUDE.md's fourth way a gate cannot
/// fail — and closing it means widening `dedup_key`, which is `--opt-report`'s
/// contract and not this prototype's to change.
fn recover(entries: &[Entry]) -> (Vec<Recovered>, Vec<Shape>) {
    let mut by_binding: BTreeMap<BindingKey, Option<(TypeOrShape, &Entry)>> = BTreeMap::new();
    for entry in entries {
        let Some(ty) = recovered_type(entry) else {
            continue;
        };
        let key = (
            entry.module.clone(),
            entry.function.clone(),
            entry.name.clone(),
            entry.local_id,
        );
        match by_binding.get_mut(&key) {
            // Already poisoned by an earlier disagreement — stays poisoned.
            Some(None) => {}
            Some(slot @ Some(_)) => {
                let agrees = slot.as_ref().is_some_and(|(seen, _)| *seen == ty);
                if !agrees {
                    *slot = None;
                }
            }
            None => {
                by_binding.insert(key, Some((ty, entry)));
            }
        }
    }

    // Structural shapes are de-duplicated by their field list and named in
    // first-appearance order, so the document is deterministic: the same input
    // always produces the same interface numbering.
    let mut shapes: Vec<Shape> = Vec::new();
    let mut out = Vec::new();
    for (_, slot) in by_binding {
        let Some((ty, entry)) = slot else { continue };
        let ts_type = match ty {
            TypeOrShape::Ts(t) => t,
            TypeOrShape::Shape(shape) => {
                let index = match shapes.iter().position(|s| *s == shape) {
                    Some(i) => i,
                    None => {
                        shapes.push(shape);
                        shapes.len() - 1
                    }
                };
                shape_name(index)
            }
        };
        out.push(Recovered {
            module: entry.module.clone(),
            function: entry.function.clone(),
            name: entry.name.clone(),
            position: entry.position.as_str(),
            ts_type,
            analysis: entry.analysis.as_str(),
            rep: entry.rep.clone(),
        });
    }
    (out, shapes)
}

fn shape_name(index: usize) -> String {
    format!("PerryShape{}", index + 1)
}

/// The header every emitted document carries.
///
/// It states the flag is experimental and states the coverage caveat in the
/// artifact itself, because the artifact outlives the terminal session that
/// produced it and a reader who finds it in a repo has no other way to know
/// that "absent" means "not proven" rather than "proven absent".
const HEADER: &str = "\
// Generated by `perry --emit-types` — EXPERIMENTAL (#7685). Do not edit.
//
// These types are recovered from Perry's representation-selection analysis:
// they are the types the compiler PROVED in order to pick an unboxed
// representation, not the output of a type inferencer. Coverage is therefore
// exactly Perry's proof rate, which is low on code that was not written for it
// (`--opt-report` reports the other half: what could not be proven, and why).
//
// A binding absent from this file is NOT untyped — it is unproven. Nothing here
// is a guess: where the proof was conditional, the binding was omitted.
//
// Parameter and return types are NOT recovered, so this file contains no
// function signatures. Perry's specialized-ABI analysis picks the most frequent
// argument-type tuple across a function's call sites and guards the rest — a
// majority, not a proof — so it cannot license a parameter annotation.
//
// Only ONE of Perry's five representation analyses licenses a TypeScript type:
// the Ptr<Shape> object proof. The numeric and string slot representations are
// storage decisions that survive a false annotation, and the array proof admits
// holes that read back as undefined — none of them is a value proof, so none is
// emitted. See crates/perry-codegen/src/emit_types.rs for the case-by-case
// reasoning.
";

/// Render the recovered types as TypeScript.
///
/// Structural shapes become real `export interface` declarations — valid,
/// checkable TypeScript. Locals cannot: TypeScript has no syntax for declaring
/// the type of another file's function-local, and HIR carries no span to
/// rewrite the source with. They are therefore rendered as a per-function
/// comment block, which is the honest form rather than invalid syntax.
pub fn render_ts(entries: &[Entry]) -> String {
    let (recovered, shapes) = recover(entries);
    let mut out = String::from(HEADER);

    if recovered.is_empty() && shapes.is_empty() {
        out.push_str("\n// No binding in this program had a recoverable type.\n");
        return out;
    }

    if !shapes.is_empty() {
        out.push_str(
            "\n// ── Recovered structural shapes ──────────────────────────────────────────\n",
        );
        out.push_str("// Object literals whose field set Perry proved closed and immutable.\n\n");
        for (i, shape) in shapes.iter().enumerate() {
            out.push_str(&format!(
                "export interface {} {}\n\n",
                shape_name(i),
                shape.render()
            ));
        }
    }

    if !recovered.is_empty() {
        out.push_str(
            "// ── Recovered local bindings ─────────────────────────────────────────────\n",
        );
        out.push_str(
            "// Comments, not declarations: TypeScript cannot declare another file's\n\
             // function-local, and HIR carries no source span to rewrite in place.\n",
        );
        let mut by_scope: BTreeMap<(&str, &str), Vec<&Recovered>> = BTreeMap::new();
        for r in &recovered {
            by_scope
                .entry((r.module.as_str(), r.function.as_str()))
                .or_default()
                .push(r);
        }
        for ((module, function), rows) in by_scope {
            out.push_str(&format!("\n// {module} — {function}\n"));
            for r in rows {
                out.push_str(&format!(
                    "//   {}: {};   // {} [{}]\n",
                    r.name, r.ts_type, r.analysis, r.rep
                ));
            }
        }
    }
    out
}

/// The machine-readable form, for measurement and tooling.
///
/// The accuracy harness consumes this rather than parsing the TypeScript, so
/// the number it reports is a number about the mapping and not about a regex.
/// Both forms come from [`recover`], so they cannot drift.
pub fn render_json(entries: &[Entry]) -> String {
    let (recovered, shapes) = recover(entries);
    let shapes_json: Vec<_> = shapes
        .iter()
        .enumerate()
        .map(|(i, s)| serde_json::json!({ "name": shape_name(i), "fields": s.fields }))
        .collect();
    let doc = serde_json::json!({
        "schema_version": 1,
        "experimental": true,
        "summary": {
            "recovered_bindings": recovered.len(),
            "recovered_shapes": shapes.len(),
        },
        "shapes": shapes_json,
        "bindings": recovered,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

#[cfg(test)]
mod tests;
