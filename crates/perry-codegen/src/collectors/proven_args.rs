//! Exact-shape facts carried into selected method parameters.
//!
//! A TypeScript parameter annotation can nominate a candidate, but is never
//! proof. For an unannotated JavaScript parameter, a unique local class whose
//! declared fields cover every direct read may nominate the candidate instead.
//! A call site must still prove the argument's exact runtime class and shape
//! before it may enter the clone described here; the ordinary method body
//! remains the fallback for every other value. The clone keeps the public
//! tagged ABI, so the parameter is stored in (and reloaded from) its ordinary
//! shadow-bound slot at every fixed-offset field access.
//!
//! Direct declared-field reads are accepted only while the parameter remains
//! contained. A later bare use may publish it, but then no later field read may
//! consume the entry proof and the caller must drop its post-call containment
//! fact. Stores/reassignment still keep the generic body because their
//! frozen/sealed and changing-binding semantics need stronger modeling.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Stmt};

use super::ptr_shape::{chain_admissible, chain_classes, chain_field_names, PtrShapeLocal};
use super::ModuleDispatchFacts;

/// One parameter proven by the call-site guard on entry to `$pshape_args`.
#[derive(Debug, Clone)]
pub struct ProvenShapeArg {
    /// Zero-based source/formal argument position (the method receiver is not
    /// counted).
    pub param_index: usize,
    pub param_id: u32,
    pub fact: PtrShapeLocal,
    /// True only when the method never publishes this argument. A false value
    /// still permits guarded direct reads before the first escape, but the
    /// caller must drop its broad post-call containment fact.
    pub preserves_containment: bool,
}

/// The single, non-combinatorial exact-shape argument clone for a method.
/// Every listed argument must pass before the call enters the clone.
#[derive(Debug, Clone)]
pub struct ProvenShapeArgPlan {
    pub args: Vec<ProvenShapeArg>,
}

/// Reserved generated symbol for the method's exact-shape argument clone.
pub(crate) fn pshape_args_method_name(public_name: &str) -> String {
    format!("{public_name}$pshape_args")
}

/// Nominate parameters whose contained prefix reads declared class fields.
///
/// A declared `C` type or unique unannotated field signature only chooses the
/// expected class for the runtime guard emitted at every routed call site.
/// Classes absent from `classes`, optional/default/rest/`arguments`
/// parameters, and async bodies stand down. Module-wide shape barriers do not
/// reject a clone: every route revalidates the live argument's exact class and
/// shape, and only a separate containment proof can make that route reachable.
pub(crate) fn method_proven_shape_args(
    method: &Function,
    classes: &HashMap<String, &Class>,
    visible_class_names: &HashSet<&str>,
) -> Option<ProvenShapeArgPlan> {
    if !super::ptr_shape::ptr_shape_locals_enabled()
        || method.is_async
        || method.is_generator
        || method.was_plain_async
        || !method.captures.is_empty()
    {
        return None;
    }

    let mut args = Vec::new();
    for (param_index, param) in method.params.iter().enumerate() {
        if param.default.is_some() || param.is_rest || param.arguments_object.is_some() {
            continue;
        }
        let mut use_check = PrefixContainedParamUse {
            param_id: param.id,
            field_reads: HashSet::new(),
            safe: true,
            escaped: false,
            read_in_region: false,
        };
        use_check.walk_stmts(&method.body);
        if !use_check.safe || use_check.field_reads.is_empty() {
            continue;
        }
        let class_name = match &param.ty {
            Type::Named(class_name) => {
                if !visible_class_names.contains(class_name.as_str())
                    || !class_fields_cover(classes, class_name, &use_check.field_reads)
                {
                    continue;
                }
                class_name.clone()
            }
            // An unannotated JS parameter lowers to `Any`. The field signature
            // is only a nomination mechanism: runtime guards still prove the
            // exact class and shape at every route.
            Type::Any => {
                let mut candidates = visible_class_names.iter().filter(|class_name| {
                    class_fields_cover(classes, class_name, &use_check.field_reads)
                });
                let Some(candidate) = candidates.next() else {
                    continue;
                };
                if candidates.next().is_some() {
                    continue;
                }
                (*candidate).to_string()
            }
            _ => continue,
        };
        if !chain_admissible(classes, &class_name) {
            continue;
        }
        args.push(ProvenShapeArg {
            param_index,
            param_id: param.id,
            fact: PtrShapeLocal {
                class_name,
                // An exact shape proves offsets, not the representation of a
                // caller-owned field value.
                numeric_fields: HashSet::new(),
                report_name: crate::opt_report::enabled().then(|| param.name.clone()),
            },
            preserves_containment: !use_check.escaped,
        });
    }

    (!args.is_empty()).then_some(ProvenShapeArgPlan { args })
}

fn class_fields_cover(
    classes: &HashMap<String, &Class>,
    class_name: &str,
    field_reads: &HashSet<String>,
) -> bool {
    if !chain_admissible(classes, class_name) {
        return false;
    }
    let fields = chain_field_names(&chain_classes(classes, class_name));
    !fields.is_empty() && field_reads.is_subset(&fields)
}

/// Check the caller-side provenance and alias terms for one guarded route.
///
/// A route may only be admitted when the clone preserves containment for the
/// parameter's whole lifetime. That is not a stylistic preference: the fact
/// map this feeds is keyed by local id and therefore flow-INSENSITIVE, so a
/// fact kept past a publishing call is consulted again at every later route
/// site for the same local — including sites that execute after the alias
/// exists. `PrefixContainedParamUse` proves a temporal property ("the reads
/// happen before the publication"), which a per-local map cannot express, so
/// the only sound reading of a publishing clone is that no caller-side
/// containment fact survives it at all.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_preserves_argument_containment(
    module_dispatch: &ModuleDispatchFacts,
    candidates: &HashMap<u32, String>,
    roots: &HashMap<u32, u32>,
    receiver_root: Option<u32>,
    owner_class: Option<&str>,
    method: &str,
    param_index: usize,
    arg: &Expr,
    call_args: &[Expr],
) -> bool {
    let Expr::LocalGet(id) = arg else {
        return false;
    };
    let Some(root) = roots.get(id) else {
        return false;
    };
    // The clone assumes that nothing reachable through `this` or another
    // formal can reshape a selected argument between its entry guard and a
    // fixed-offset read. Preserve containment only when this tracked object is
    // unique across every value supplied to the call.
    if receiver_root == Some(*root)
        || call_args.iter().enumerate().any(|(other_index, other)| {
            other_index != param_index
                && matches!(
                    other,
                    Expr::LocalGet(other_id) if roots.get(other_id) == Some(root)
                )
        })
    {
        return false;
    }
    let route = match owner_class {
        Some(owner) => module_dispatch.argument_shape_route(owner, method, param_index),
        None => module_dispatch.unique_argument_shape_class(method, param_index),
    };
    let Some((expected, preserves_containment)) = route else {
        return false;
    };
    preserves_containment && candidates.get(root).is_some_and(|got| got == expected)
}

/// Direct declared-field reads are safe until the first bare use publishes the
/// parameter. The tagged clone may continue generically after publication,
/// but no later field access may consume its entry shape proof.
struct PrefixContainedParamUse {
    param_id: u32,
    field_reads: HashSet<String>,
    safe: bool,
    escaped: bool,
    /// Whether the currently active repeated region performed any accepted
    /// direct field read. This is independent of `field_reads` cardinality:
    /// rereading a field already seen before the loop must still count.
    read_in_region: bool,
}

impl PrefixContainedParamUse {
    fn walk_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        if !self.safe {
            return;
        }
        match stmt {
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => self.walk_expr(expr),
            Stmt::Let {
                init: Some(expr), ..
            } => self.walk_expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition);
                self.walk_stmts(then_branch);
                if let Some(branch) = else_branch {
                    self.walk_stmts(branch);
                }
            }
            Stmt::While { condition, body } => {
                let outer = self.enter_repeated_region();
                self.walk_expr(condition);
                self.walk_stmts(body);
                self.finish_repeated_region(outer);
            }
            Stmt::DoWhile { condition, body } => {
                let outer = self.enter_repeated_region();
                self.walk_stmts(body);
                self.walk_expr(condition);
                self.finish_repeated_region(outer);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                let outer = self.enter_repeated_region();
                if let Some(condition) = condition {
                    self.walk_expr(condition);
                }
                if let Some(update) = update {
                    self.walk_expr(update);
                }
                self.walk_stmts(body);
                self.finish_repeated_region(outer);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_stmts(body);
                if let Some(catch) = catch {
                    self.walk_stmts(&catch.body);
                }
                if let Some(finally) = finally {
                    self.walk_stmts(finally);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.walk_expr(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.walk_expr(test);
                    }
                    self.walk_stmts(&case.body);
                }
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(body),
            Stmt::Return(None)
            | Stmt::Let { init: None, .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn enter_repeated_region(&mut self) -> (bool, bool) {
        let outer = (self.escaped, self.read_in_region);
        self.read_in_region = false;
        outer
    }

    fn finish_repeated_region(&mut self, outer: (bool, bool)) {
        let (escaped_before, read_before) = outer;
        // Even when every first-iteration read precedes publication, the next
        // iteration would perform that read after publication. Refuse the
        // whole-parameter overlay when a repeated region contains both.
        if !escaped_before && self.escaped && self.read_in_region {
            self.safe = false;
        }
        self.read_in_region |= read_before;
    }

    fn walk_expr(&mut self, expr: &Expr) {
        if !self.safe {
            return;
        }
        match expr {
            // This is the only position that consumes the proof.  Do not walk
            // the receiver child: its otherwise-bare LocalGet is licensed by
            // this declared-field operation.
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.param_id) => {
                if self.escaped {
                    self.safe = false;
                } else {
                    self.field_reads.insert(property.clone());
                    self.read_in_region = true;
                }
            }
            // A direct store/update has frozen/sealed and setter semantics not
            // implied by an exact-shape entry guard.
            Expr::PropertySet { object, .. } | Expr::PropertyUpdate { object, .. } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.param_id) =>
            {
                self.safe = false;
            }
            // A bare use publishes the object. Reads already performed remain
            // valid, but the entry proof cannot license any later field read.
            Expr::LocalGet(id) if *id == self.param_id => self.escaped = true,
            Expr::LocalSet(id, _) if *id == self.param_id => self.safe = false,
            Expr::Closure { body, .. } => {
                perry_hir::walker::walk_expr_children(expr, &mut |child| self.walk_expr(child));
                self.walk_stmts(body);
            }
            _ => perry_hir::walker::walk_expr_children(expr, &mut |child| self.walk_expr(child)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_read(param_id: u32) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(param_id)),
            property: "id".to_string(),
            byte_offset: 0,
        }
    }

    #[test]
    fn repeated_reread_before_publication_rejects_entry_shape_proof() {
        let param_id = 7;
        let mut use_check = PrefixContainedParamUse {
            param_id,
            field_reads: HashSet::new(),
            safe: true,
            escaped: false,
            read_in_region: false,
        };
        use_check.walk_stmts(&[
            Stmt::Expr(field_read(param_id)),
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![
                    // `id` is already in the HashSet before this loop. The
                    // repeated-region proof must count this occurrence, not a
                    // set-size delta, because the next iteration reads after
                    // the publication below.
                    Stmt::Expr(field_read(param_id)),
                    Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::LocalGet(99)),
                        args: vec![Expr::LocalGet(param_id)],
                        type_args: Vec::new(),
                        byte_offset: 0,
                    }),
                ],
            },
        ]);
        assert!(!use_check.safe);
    }

    /// `$pshape_args` is an internal direct-call capability. Keep every place
    /// that can spell its suffix visible here so a future vtable/indirect-call
    /// registration fails the same kind of reachability ratchet as the
    /// proven-`this` family.
    #[test]
    fn pshape_argument_symbol_reachability() {
        let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed: [&str; 10] = [
            "collectors/proven_args.rs",                   // naming + this test
            "collectors/proven_this.rs",                   // exact-suffix reachability split
            "collectors/ptr_shape.rs",                     // containment route contract
            "collectors/scalar_method_dispatch.rs",        // emitted-route metadata
            "codegen/argument_shape_clone_tests.rs",       // IR/report ratchets
            "codegen/method.rs",                           // clone emission
            "codegen/opts.rs",                             // cross-module context
            "expr/mod.rs",                                 // clone parameter proof overlay
            "lower_call/method_override.rs",               // guarded direct routing
            "lower_call/property_get/dynamic_dispatch.rs", // Ptr<Shape> receiver routing
        ];
        let mut offenders = Vec::new();

        fn visit(
            dir: &std::path::Path,
            root: &std::path::Path,
            allowed: &[&str],
            out: &mut Vec<String>,
        ) {
            for entry in std::fs::read_dir(dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    visit(&path, root, allowed, out);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if allowed.contains(&rel.as_str()) {
                    continue;
                }
                if std::fs::read_to_string(&path)
                    .expect("read source file")
                    .contains("$pshape_args")
                {
                    out.push(rel);
                }
            }
        }

        visit(&src_root, &src_root, &allowed, &mut offenders);
        assert!(
            offenders.is_empty(),
            "argument-shape clone symbol fragments found outside the direct-call allowlist: \
             {offenders:?}"
        );
    }
}
