//! `--opt-report` (#6952) support for the `Ptr<Shape>` collector.
//!
//! `collectors/ptr_shape.rs` already knows exactly which rule denied each
//! candidate — it just `continue`s. This module supplies the vocabulary and
//! the two auxiliary walks (binding names, loop depths, unbound allocation
//! sites) that let those `continue` sites record `(value, reason)` instead of
//! dropping the information on the floor.
//!
//! **Everything here runs only when [`crate::opt_report::enabled`] is true.**
//! The collector's returned facts are untouched either way: recording happens
//! *next to* the `continue`, never instead of it.
//!
//! ## On the actionability tiers
//!
//! The tier is a judgment about who can act, and it is stated here rather
//! than inferred at render time so it is reviewable in one place:
//!
//! - **`Fixable`** — the source could reasonably be written another way
//!   (stop reassigning, don't capture in a closure, don't add ad-hoc
//!   properties). The RFC's own examples.
//! - **`CompilerLimitation`** — the source is idiomatic TypeScript that
//!   Perry cannot yet prove through. `return`, call arguments and container
//!   elements are all in this bucket: #7034 §1/§4 established that the
//!   proof needs to survive an escape, and that work is not written yet.
//!   Telling a developer to stop returning records would be bad advice.
//! - **`InherentlyPolymorphic`** — genuinely dynamic; correctly boxed.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Stmt};

use crate::opt_report::{self, Analysis, Denial, Position, Tier};

/// A named denial: the rule as the collector numbers it, a human expansion,
/// the actionability tier, and the tracking issue when the fix is ours.
#[derive(Debug, Clone, Copy)]
pub(super) struct ShapeDenial {
    pub rule: &'static str,
    pub reason: &'static str,
    pub tier: Tier,
    pub issue: Option<&'static str>,
}

const RULE1: &str = "rule 1 (provenance)";
const RULE2: &str = "rule 2 (containment)";
const RULE3: &str = "rule 3 (this-flow containment)";
const RULE4: &str = "rule 4 (dispatch stability)";
const RULE5: &str = "rule 5 (module-wide barrier)";
const ADMISSION: &str = "class admission";
const GATE: &str = "disabled by PERRY_PTR_SHAPE_LOCALS";

// ── Rule 1: provenance ─────────────────────────────────────────────────────

pub(super) const UNBOUND_ALLOC: ShapeDenial = ShapeDenial {
    rule: RULE1,
    reason: "allocated in expression position and never bound to a `let`/`const`, \
             so the shape proof has no local to anchor to. This is the \
             `.map(x => ({...}))` / `return { ... }` idiom.",
    tier: Tier::CompilerLimitation,
    issue: Some("#7034 §4 (return-shape facts)"),
};

pub(super) const LET_INIT_NOT_NEW: ShapeDenial = ShapeDenial {
    rule: RULE1,
    reason: "the binding is re-declared with an initializer that is not the \
             original allocation, so its dynamic class is not fixed.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const MULTIPLE_LET: ShapeDenial = ShapeDenial {
    rule: RULE1,
    reason: "bound by more than one `let`/`const` in this body; the proof \
             requires exactly one binding for the local's whole lifetime.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ALIAS_NOT_SINGLE_LET: ShapeDenial = ShapeDenial {
    rule: RULE1,
    reason: "an alias of this object is bound more than once, leaving a \
             second binding the proof does not cover.",
    tier: Tier::Fixable,
    issue: None,
};

// ── Class admission ────────────────────────────────────────────────────────

pub(super) const ADMIT_ACCESSOR: ShapeDenial = ShapeDenial {
    rule: ADMISSION,
    reason: "the class chain declares a getter or setter, so a field access \
             is a call, not a fixed-offset load.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ADMIT_COMPUTED: ShapeDenial = ShapeDenial {
    rule: ADMISSION,
    reason: "the class chain has a computed member or computed field key, so \
             its key set is not statically known.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ADMIT_DYNAMIC_BASE: ShapeDenial = ShapeDenial {
    rule: ADMISSION,
    reason: "the class chain extends a dynamic or lexically-shadowed \
             expression, so the parent shape is not statically walkable.",
    tier: Tier::CompilerLimitation,
    issue: None,
};

pub(super) const ADMIT_NATIVE_BASE: ShapeDenial = ShapeDenial {
    rule: ADMISSION,
    reason: "the class chain extends a native, unmodeled, or built-in Error \
             base, which installs its surface as own properties at run time.",
    tier: Tier::CompilerLimitation,
    issue: None,
};

pub(super) const ADMIT_UNRESOLVED: ShapeDenial = ShapeDenial {
    rule: ADMISSION,
    reason: "the allocated class is not resolvable in this module's class \
             table (imported, re-exported, or synthesised).",
    tier: Tier::CompilerLimitation,
    issue: None,
};

// ── Rule 2: containment ────────────────────────────────────────────────────

pub(super) const ESC_REASSIGNED: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "the binding is reassigned, so it does not hold one object of one \
             class for its whole lifetime.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_CLOSURE_CAPTURE: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "captured by a closure; the capture machinery creates an alias \
             whose lifetime the proof cannot bound.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_CALL_ARGUMENT: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "passed as a call argument. There is no mechanism yet by which a \
             shape fact at a call site becomes a fact about the callee's \
             parameter, so any argument position disqualifies.",
    tier: Tier::CompilerLimitation,
    issue: Some("#7034 §1 (argument positions via clone-and-route)"),
};

pub(super) const ESC_RETURN: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "returned from this function. Return positions do not carry a \
             shape fact yet, so returning a record forfeits its proof.",
    tier: Tier::CompilerLimitation,
    issue: Some("#7034 §4 (return-shape facts)"),
};

pub(super) const ESC_ELEMENT: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "stored into a container whose own uses the proof cannot bound. \
             `A.push(x)` is contained when `A` is an element-shape-proven \
             local array (#7034 §3: single empty-literal `Let`, only `push` \
             writes of one class, only `.length` and in-bounds `A[i]` reads, \
             `return A` aside); object properties and every other container \
             slot still carry no shape fact.",
    tier: Tier::CompilerLimitation,
    issue: Some("#7034 §3/§5 P4-P5 (elements and fields)"),
};

pub(super) const ESC_ELEMENT_GROUP: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "it shares an element-shape-proven array with a value that failed \
             containment. One member adding a property reshapes the objects \
             every other member reads guard-free, so an element group is \
             all-or-nothing — fix the sibling's escape and this value is \
             promoted with it.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_THROWN: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "thrown; the value escapes on an exceptional edge the proof does \
             not follow.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_BARE_REFERENCE: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "referenced as a bare value somewhere other than a declared-field \
             access or a vetted method call, which creates an alias the escape \
             walk cannot bound.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_UNDECLARED_PROPERTY: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "a property that is not a declared field of the class chain is \
             read or written on it, which would transition its shape.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_UNRESOLVED_METHOD: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "a method that does not resolve on the declared class chain is \
             called on it.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_DELETE: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "`delete` is applied to one of its properties, which changes its \
             shape at run time.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_FREEZE: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "`Object.freeze`/`seal`/`preventExtensions` is applied to it, \
             which rewrites its descriptors.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const ESC_CONTAINER_MUTATOR: ShapeDenial = ShapeDenial {
    rule: RULE2,
    reason: "used as the receiver of a container mutator (array push/pop/\
             splice, Set.add, …), which the proof treats as an escape.",
    tier: Tier::Fixable,
    issue: None,
};

// ── Rules 3-5 ──────────────────────────────────────────────────────────────

pub(super) const THIS_ESCAPE: ShapeDenial = ShapeDenial {
    rule: RULE3,
    reason: "the constructor chain, a field initializer, or a called method \
             leaks `this` as a value (stores, passes, or captures it), \
             creating an alias the escape walk cannot see.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const METHOD_THIS_ESCAPE: ShapeDenial = ShapeDenial {
    rule: RULE3,
    reason: "a method called on this local is not `this`-flow safe — its body \
             uses `this` as a value rather than only accessing declared \
             fields and calling vetted methods.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const UNSTABLE_PROTOTYPE: ShapeDenial = ShapeDenial {
    rule: RULE4,
    reason: "the module writes through a `.prototype` object, so method \
             dispatch on this class is not statically stable.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const FIELD_METHOD_AMBIGUITY: ShapeDenial = ShapeDenial {
    rule: RULE4,
    reason: "a called name is both a declared field and a method on the chain, \
             which is ambiguous under own-property shadowing.",
    tier: Tier::Fixable,
    issue: None,
};

pub(super) const MODULE_BARRIER: ShapeDenial = ShapeDenial {
    rule: RULE5,
    reason: "the module contains a shape barrier (`Object.defineProperty`, \
             `delete`, `setPrototypeOf`/`__proto__` write, `new Proxy`, or a \
             mutating `Reflect.*`). The first-increment rule disables ALL \
             Ptr<Shape> promotion in the module regardless of the barrier's \
             target.",
    tier: Tier::CompilerLimitation,
    issue: Some("#7034 §2 (barrier narrowing)"),
};

pub(super) const GATE_DISABLED: ShapeDenial = ShapeDenial {
    rule: GATE,
    reason: "shape-proven pointer locals are switched off for this build \
             (`PERRY_PTR_SHAPE_LOCALS=0`).",
    tier: Tier::CompilerLimitation,
    issue: None,
};

// ── Recording ──────────────────────────────────────────────────────────────

thread_local! {
    /// #7034 §4: the return-shape module pre-pass re-runs the WHOLE Phase 3b
    /// proof over each candidate producer's body, before codegen has entered
    /// any region. Those runs are a proof query, not a compilation decision:
    /// left unsuppressed they would double-count every win and attribute
    /// every denial to no region at all. The real per-region pass reports the
    /// same bodies later, so nothing is lost.
    static SUPPRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// True while a [`SuppressScope`] is alive on this thread.
pub(super) fn suppressed() -> bool {
    SUPPRESS.with(|s| s.get())
}

/// RAII: silence `--opt-report` / `PERRY_REPSEL_DEBUG` recording for the
/// duration of a speculative proof query. Restores the previous state, so
/// nesting is safe.
pub(super) struct SuppressScope(bool);

impl SuppressScope {
    pub(super) fn new() -> Self {
        SuppressScope(SUPPRESS.with(|s| s.replace(true)))
    }
}

impl Drop for SuppressScope {
    fn drop(&mut self) {
        SUPPRESS.with(|s| s.set(self.0));
    }
}

/// Record one denied `Ptr<Shape>` candidate.
pub(super) fn deny_local(
    id: u32,
    names: &HashMap<u32, String>,
    depths: &HashMap<u32, u32>,
    class_name: Option<&str>,
    d: ShapeDenial,
) {
    // Guard here, not just inside `opt_report::deny`: the arguments below
    // allocate, and they are evaluated before the callee can early-return.
    if !opt_report::enabled() || suppressed() {
        return;
    }
    let fallback = format!("<local {id}>");
    let name = names.get(&id).map(String::as_str).unwrap_or(&fallback);
    opt_report::deny(Denial {
        position: Position::Local,
        name,
        local_id: Some(id),
        analysis: Analysis::PtrShape,
        rule: d.rule,
        reason: d.reason,
        tier: d.tier,
        issue: d.issue,
        loop_depth: depths.get(&id).copied().unwrap_or(0),
        detail: class_name.map(|c| format!("candidate class: {c}")),
        byte_offset: None,
    });
}

/// Record an allocation site that never became a candidate (rule 1).
pub(super) fn deny_alloc_site(site: &NewSite) {
    if !opt_report::enabled() || suppressed() {
        return;
    }
    opt_report::deny(Denial {
        position: Position::AllocSite,
        name: &site.display,
        local_id: None,
        analysis: Analysis::PtrShape,
        rule: UNBOUND_ALLOC.rule,
        reason: UNBOUND_ALLOC.reason,
        tier: UNBOUND_ALLOC.tier,
        issue: UNBOUND_ALLOC.issue,
        loop_depth: site.loop_depth,
        detail: Some(format!("allocation position: {}", site.context)),
        byte_offset: (site.byte_offset != 0).then_some(site.byte_offset),
    });
}

// ── Auxiliary walks (only run under the report flag) ───────────────────────

/// `LocalId -> source binding name`, for every `Stmt::Let` in the region.
/// HIR keeps names through lowering; positions do not survive, which is why
/// the report is by function + name rather than `file:line`.
pub(super) fn local_names(stmts: &[Stmt]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    walk_lets(stmts, 0, &mut |id, name, _depth| {
        out.insert(id, name.to_string());
    });
    out
}

/// `LocalId -> loop-nesting depth of its declaration`. A hotness *proxy*: it
/// cannot see that a closure body with depth 0 runs once per element, which
/// is why [`crate::opt_report::Entry::invoked_per_element`] is a separate
/// column rather than folded in here.
pub(super) fn loop_depths(stmts: &[Stmt]) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    walk_lets(stmts, 0, &mut |id, _name, depth| {
        out.insert(id, depth);
    });
    out
}

fn walk_lets(stmts: &[Stmt], depth: u32, f: &mut impl FnMut(u32, &str, u32)) {
    for s in stmts {
        match s {
            Stmt::Let { id, name, .. } => f(*id, name, depth),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_lets(then_branch, depth, f);
                if let Some(eb) = else_branch {
                    walk_lets(eb, depth, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                walk_lets(body, depth + 1, f);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    walk_lets(std::slice::from_ref(init.as_ref()), depth + 1, f);
                }
                walk_lets(body, depth + 1, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_lets(body, depth, f);
                if let Some(c) = catch {
                    walk_lets(&c.body, depth, f);
                }
                if let Some(fin) = finally {
                    walk_lets(fin, depth, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_lets(&c.body, depth, f);
                }
            }
            Stmt::Labeled { body, .. } => walk_lets(std::slice::from_ref(body.as_ref()), depth, f),
            _ => {}
        }
    }
}

/// An object allocation that is not the initializer of a `Stmt::Let`.
pub(super) struct NewSite {
    /// `new Row(...)` / `{ key, value }` — what the developer wrote.
    pub display: String,
    /// Where it sits: `return`, `call argument`, `array element`, …
    pub context: &'static str,
    pub loop_depth: u32,
    /// Byte offset of the `new` expression in its module's source. `Expr::New`
    /// is the one HIR node that already carries a source position (#5253,
    /// captured for constructor TypeErrors), so allocation sites — which have
    /// no binding name to report — can still be located. Ordinary locals have
    /// no span; see the module doc.
    pub byte_offset: u32,
}

/// Allocation sites in this region that rule 1 can never see, because they
/// are never bound to a local. **Does not descend into closure bodies** —
/// those are lowered as their own regions and reported under their own
/// function name.
pub(super) fn unbound_new_sites(stmts: &[Stmt]) -> Vec<NewSite> {
    let mut out = Vec::new();
    scan_stmts(stmts, 0, "statement", &mut out);
    out
}

fn scan_stmts(stmts: &[Stmt], depth: u32, ctx: &'static str, out: &mut Vec<NewSite>) {
    for s in stmts {
        match s {
            // The Let init IS the provenance site rule 1 accepts; skip it and
            // scan only its arguments.
            Stmt::Let {
                init: Some(Expr::New { args, .. }),
                ..
            } => {
                for a in args {
                    scan_expr(a, depth, "constructor argument", out);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    scan_expr(e, depth, "initializer", out);
                }
            }
            Stmt::Expr(e) => scan_expr(e, depth, ctx, out),
            Stmt::Throw(e) => scan_expr(e, depth, "throw", out),
            Stmt::Return(Some(e)) => scan_expr(e, depth, "return", out),
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                scan_expr(condition, depth, "condition", out);
                scan_stmts(then_branch, depth, ctx, out);
                if let Some(eb) = else_branch {
                    scan_stmts(eb, depth, ctx, out);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                scan_expr(condition, depth + 1, "condition", out);
                scan_stmts(body, depth + 1, ctx, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    scan_stmts(std::slice::from_ref(init.as_ref()), depth + 1, ctx, out);
                }
                if let Some(c) = condition {
                    scan_expr(c, depth + 1, "condition", out);
                }
                if let Some(u) = update {
                    scan_expr(u, depth + 1, "loop update", out);
                }
                scan_stmts(body, depth + 1, ctx, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_stmts(body, depth, ctx, out);
                if let Some(c) = catch {
                    scan_stmts(&c.body, depth, ctx, out);
                }
                if let Some(f) = finally {
                    scan_stmts(f, depth, ctx, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                scan_expr(discriminant, depth, "switch discriminant", out);
                for c in cases {
                    if let Some(t) = &c.test {
                        scan_expr(t, depth, "case test", out);
                    }
                    scan_stmts(&c.body, depth, ctx, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                scan_stmts(std::slice::from_ref(body.as_ref()), depth, ctx, out)
            }
            _ => {}
        }
    }
}

fn scan_expr(e: &Expr, depth: u32, ctx: &'static str, out: &mut Vec<NewSite>) {
    match e {
        // A closure body is its own lowering region; it reports separately.
        Expr::Closure { .. } => {}
        Expr::New {
            class_name,
            args,
            byte_offset,
            ..
        } => {
            out.push(NewSite {
                display: display_class(class_name),
                context: ctx,
                loop_depth: depth,
                byte_offset: *byte_offset,
            });
            for a in args {
                scan_expr(a, depth, "constructor argument", out);
            }
        }
        Expr::Call { callee, args, .. } => {
            scan_expr(callee, depth, ctx, out);
            for a in args {
                scan_expr(a, depth, "call argument", out);
            }
        }
        Expr::Array(items) => {
            for i in items {
                scan_expr(i, depth, "array element", out);
            }
        }
        _ => perry_hir::walker::walk_expr_children(e, &mut |c| scan_expr(c, depth, ctx, out)),
    }
}

/// Anonymous object literals lower to `Expr::New { class_name:
/// "__AnonShape_…" }`; render them as the object literals the developer
/// actually wrote rather than leaking the synthetic class name.
fn display_class(class_name: &str) -> String {
    if class_name.starts_with("__AnonShape") {
        String::from("object literal { ... }")
    } else {
        format!("new {class_name}(...)")
    }
}

/// Why a class chain failed admission, or `None` when it is admissible.
/// Single source of truth for both the gate and the report — the collector's
/// `chain_admissible` is `cause(...).is_none()`.
pub(super) fn admission_cause(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    chain: &[&Class],
) -> Option<ShapeDenial> {
    if chain.is_empty() {
        return Some(if classes.contains_key(class_name) {
            ADMIT_DYNAMIC_BASE
        } else {
            ADMIT_UNRESOLVED
        });
    }
    for class in chain {
        if !class.getters.is_empty() || !class.setters.is_empty() {
            return Some(ADMIT_ACCESSOR);
        }
        if !class.computed_members.is_empty() || class.fields.iter().any(|f| f.key_expr.is_some()) {
            return Some(ADMIT_COMPUTED);
        }
        if class.extends_expr.is_some()
            || class.heritage_lexically_shadowed
            || (class.extends.is_some() && class.extends_name.is_none())
        {
            return Some(ADMIT_DYNAMIC_BASE);
        }
        if class.native_extends.is_some() {
            return Some(ADMIT_NATIVE_BASE);
        }
    }
    let class = chain[0];
    if super::this_as_value::class_chain_extends_builtin_error(class, classes)
        || super::this_as_value::class_chain_has_unmodeled_base(class, classes)
    {
        return Some(ADMIT_NATIVE_BASE);
    }
    None
}

/// Enumerate the `Stmt::Let`-bound allocation candidates in a region without
/// running the proof — used on the early-bail paths (gate off, module
/// barrier) so the report can still name what *would* have been considered.
pub(super) fn candidate_seeds(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    super::find_new_candidates(stmts, boxed_vars, module_globals, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anon_shape_classes_render_as_object_literals() {
        assert_eq!(display_class("__AnonShape_3"), "object literal { ... }");
        assert_eq!(display_class("Row"), "new Row(...)");
    }

    #[test]
    fn every_compiler_limitation_that_has_a_fix_names_its_tracking_issue() {
        // The CompilerLimitation tier is meant to double as a roadmap. The
        // escape-position denials are the ones #7034 scoped, so they must
        // carry an issue; the rest may legitimately have none.
        for d in [ESC_CALL_ARGUMENT, ESC_RETURN, ESC_ELEMENT, UNBOUND_ALLOC] {
            assert_eq!(d.tier, Tier::CompilerLimitation, "{}", d.rule);
            assert!(d.issue.is_some(), "{} must name a tracking issue", d.rule);
        }
    }

    #[test]
    fn fixable_denials_do_not_claim_a_compiler_issue() {
        for d in [
            ESC_REASSIGNED,
            ESC_CLOSURE_CAPTURE,
            ESC_DELETE,
            MULTIPLE_LET,
        ] {
            assert_eq!(d.tier, Tier::Fixable, "{}", d.rule);
            assert!(d.issue.is_none(), "{} should not name an issue", d.rule);
        }
    }
}
