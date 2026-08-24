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
//! This first increment deliberately accepts read-only declared-field uses.
//! Stores through an aliased parameter have additional frozen/sealed-object
//! semantics, and a bare use can publish the object to code the proof cannot
//! inspect.  Both therefore keep the generic body.

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

/// Nominate read-only parameters whose body uses declared class fields.
///
/// A declared `C` type or unique unannotated field signature only chooses the
/// expected class for the runtime guard emitted at every routed call site.
/// Classes absent from `classes`, optional/default/rest/`arguments`
/// parameters, async bodies, and any module carrying the conservative
/// shape-barrier latch stand down.
pub(crate) fn method_proven_shape_args(
    method: &Function,
    classes: &HashMap<String, &Class>,
    local_class_names: &HashSet<&str>,
    module_dispatch: &ModuleDispatchFacts,
) -> Option<ProvenShapeArgPlan> {
    if !super::ptr_shape::ptr_shape_locals_enabled()
        || module_dispatch.has_shape_barrier_sites()
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
        let mut use_check = ReadOnlyParamUse {
            param_id: param.id,
            field_reads: HashSet::new(),
            safe: true,
        };
        use_check.walk_stmts(&method.body);
        if !use_check.safe || use_check.field_reads.is_empty() {
            continue;
        }
        let class_name = match &param.ty {
            Type::Named(class_name) => {
                if !local_class_names.contains(class_name.as_str())
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
                let mut candidates = local_class_names.iter().filter(|class_name| {
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

/// The guarded clone's audited body cannot retain or reshape a matching
/// tracked argument, so that exact route preserves caller-side containment.
pub(super) fn route_preserves_argument_containment(
    module_dispatch: &ModuleDispatchFacts,
    candidates: &HashMap<u32, String>,
    roots: &HashMap<u32, u32>,
    owner_class: &str,
    method: &str,
    param_index: usize,
    arg: &Expr,
) -> bool {
    let Expr::LocalGet(id) = arg else {
        return false;
    };
    let Some(root) = roots.get(id) else {
        return false;
    };
    let Some(expected) = module_dispatch.argument_shape_class(owner_class, method, param_index)
    else {
        return false;
    };
    candidates.get(root).is_some_and(|got| got == expected)
}

struct ReadOnlyParamUse {
    param_id: u32,
    field_reads: HashSet<String>,
    safe: bool,
}

impl ReadOnlyParamUse {
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
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.walk_expr(condition);
                self.walk_stmts(body);
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
                if let Some(condition) = condition {
                    self.walk_expr(condition);
                }
                if let Some(update) = update {
                    self.walk_expr(update);
                }
                self.walk_stmts(body);
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
                self.field_reads.insert(property.clone());
            }
            // A direct store/update has frozen/sealed and setter semantics not
            // implied by an exact-shape entry guard.
            Expr::PropertySet { object, .. } | Expr::PropertyUpdate { object, .. } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.param_id) =>
            {
                self.safe = false;
            }
            Expr::LocalGet(id) if *id == self.param_id => self.safe = false,
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
