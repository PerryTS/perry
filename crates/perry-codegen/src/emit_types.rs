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
//! ## The one rule
//!
//! **Never emit a wrong type.** A missing annotation costs nothing; a wrong one
//! poisons a downstream `tsc`. Every mapping below is an omission unless the
//! representation *proves* the TypeScript type, and the three places where the
//! proof is conditional are omissions rather than guesses:
//!
//! - **A representation whose proof is a restatement of the source annotation**
//!   is not recovered information. [`Analysis::SpecAbi`] is exactly that:
//!   `codegen/typed_abi.rs::typed_param_rep_for_type` reads `param.ty`, which
//!   is populated from the TypeScript annotation and by nothing else — no
//!   inference writes it (the only assignments in `perry-hir` *widen* it to
//!   `Any`, `lower/shared_mutable_capture.rs:363`). Echoing it back would
//!   inflate any round-trip accuracy number to meaninglessness while
//!   recovering nothing, so spec-ABI entries are dropped. See
//!   [`recovered_type`].
//! - **A binding two entries disagree about** is dropped entirely rather than
//!   resolved by a precedence rule. A function can be lowered more than once (a
//!   boxed entry plus a typed clone) and the two lowerings can select different
//!   representations; picking a winner would be picking which of two proofs to
//!   believe. See [`recover`].
//! - **A synthetic class name is not a TypeScript type.** A `Ptr<Shape>` local
//!   whose provenance class is a compiler-synthesized `__AnonShape_*` /
//!   `__EmptySite_*` shape has no source-level name to emit. It becomes a
//!   structural interface when the field set is known, and is omitted when it
//!   is not — never emitted under its synthetic name.
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

/// Prefixes the HIR lowering uses for classes it synthesizes for object
/// literals, which therefore have no source-level name a `.ts` file could
/// refer to (`perry-hir/src/lower/expr_object.rs`).
const SYNTHETIC_CLASS_PREFIXES: [&str; 2] = ["__AnonShape_", "__EmptySite_"];

fn is_synthetic_class(name: &str) -> bool {
    SYNTHETIC_CLASS_PREFIXES
        .iter()
        .any(|p| name.starts_with(p))
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
            .filter_map(|f| {
                f.ts_type
                    .as_ref()
                    .map(|t| format!("  {}: {};", f.name, t))
            })
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
        // A restatement of the source annotation, not recovered information.
        // See the module doc.
        Analysis::SpecAbi => None,
        Analysis::CanonicalSlot => match entry.rep.as_str() {
            // `SlotRep`'s `Debug` spelling (`expr/slot_rep.rs`). `U32` is a
            // number in TypeScript exactly as `I32` is — the distinction is a
            // storage one.
            "I32" | "U32" => Some(TypeOrShape::Ts("number".to_string())),
            "Str" => Some(TypeOrShape::Ts("string".to_string())),
            _ => None,
        },
        Analysis::IntValuedTa => match entry.rep.as_str() {
            "IntValued" => Some(TypeOrShape::Ts("number".to_string())),
            _ => None,
        },
        Analysis::PtrNumArray => match entry.rep.as_str() {
            "Ptr<NumArray>" => Some(TypeOrShape::Ts("number[]".to_string())),
            _ => None,
        },
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
/// stream disagrees about.
///
/// The disagreement case is not hypothetical bookkeeping: a function lowered
/// twice (a boxed entry plus a typed clone) contributes two rows per binding,
/// and `--opt-report`'s own `dedup_key` keeps them apart on purpose. If the two
/// lowerings selected different representations, there is no principled winner,
/// so the binding is omitted.
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
// Parameter and return types are NOT recovered. Perry's specialized-ABI
// analysis derives them from the source annotation, so on unannotated
// JavaScript it proves nothing and this file will contain no signatures.
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
        out.push_str("\n// ── Recovered structural shapes ──────────────────────────────────────────\n");
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
        out.push_str("// ── Recovered local bindings ─────────────────────────────────────────────\n");
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
        .map(|(i, s)| {
            serde_json::json!({ "name": shape_name(i), "fields": s.fields })
        })
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
