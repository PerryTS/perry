//! Member-shape helpers for `class_decl`: computed-key naming, the
//! accessor-name survey, and the ECMA-262 last-wins accessor record.
//! Split out of `class_decl.rs` for the 2000-line file cap.

use super::*;

/// The property key a class method names without evaluation, if any.
fn static_method_key(method: &ast::ClassMethod) -> Option<String> {
    match &method.key {
        ast::PropName::Ident(i) => Some(i.sym.to_string()),
        ast::PropName::Str(s) => s.value.as_str().map(str::to_string),
        ast::PropName::Num(n) => Some(crate::lower::number_to_js_key(n.value)),
        ast::PropName::Computed(c) => crate::lower_decl::helpers::literal_computed_key(&c.expr),
        _ => None,
    }
}

/// Does `method` (a literal computed key) redefine a key an earlier member of
/// the same placement already defined? A redefinition replaces the earlier
/// property and keeps its position (ClassDefinitionEvaluation), which the
/// computed registration models. A named method that redefines a named method
/// is handled by [`define_named_method`].
fn literal_key_redefines(
    earlier: &[ast::ClassMember],
    method: &ast::ClassMethod,
    key: &str,
) -> bool {
    earlier.iter().any(|m| match m {
        ast::ClassMember::Method(e) => {
            e.is_static == method.is_static
                && e.function.body.is_some()
                // A getter and a setter of one key are two halves of one
                // property, not a redefinition.
                && (e.kind == method.kind
                    || e.kind == ast::MethodKind::Method
                    || method.kind == ast::MethodKind::Method)
                && static_method_key(e).as_deref() == Some(key)
        }
        _ => false,
    })
}

/// Define method `func` on a named-member list. A redefinition (`b() {}
/// b() {}`) replaces the earlier method in place and keeps its order token
/// (the earlier function id): ClassDefinitionEvaluation keeps the property's
/// first position and gives it the last definition.
pub(super) fn define_named_method(list: &mut Vec<Function>, func: Function) {
    match list.iter_mut().find(|m| m.name == func.name) {
        Some(earlier) => {
            *earlier = Function {
                id: earlier.id,
                ..func
            }
        }
        None => list.push(func),
    }
}

pub(super) fn generic_computed_member_key<'a>(
    _ctx: &LoweringContext,
    method: &'a ast::ClassMethod,
    earlier: &[ast::ClassMember],
) -> Option<&'a ast::ComputedPropName> {
    let ast::PropName::Computed(computed) = &method.key else {
        return None;
    };
    // Single source of truth — see `is_special_lowered_well_known`. #9226
    // hand-copied a subset here and silently dropped four symbols.
    if crate::lower_decl::helpers::is_special_lowered_well_known(method)
        || crate::lower_decl::helpers::literal_computed_key(&computed.expr)
            .is_some_and(|key| !literal_key_redefines(earlier, method, &key))
    {
        return None;
    }
    Some(computed)
}

pub(super) fn computed_member_name(
    kind: ast::MethodKind,
    computed: &ast::ComputedPropName,
) -> String {
    let base = match kind {
        ast::MethodKind::Method => "__computed_method",
        ast::MethodKind::Getter => "__computed_getter",
        ast::MethodKind::Setter => "__computed_setter",
    };
    format!("{}_{}_{}", base, computed.span.lo.0, computed.span.hi.0)
}

pub(super) fn runtime_instance_accessor_names(
    members: &[ast::ClassMember],
) -> crate::ClassAccessorNames {
    let mut accessor_names = crate::ClassAccessorNames::default();

    for member in members {
        match member {
            ast::ClassMember::Method(m)
                if !m.is_static
                    && m.function.body.is_some()
                    && matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
            {
                let key = match &m.key {
                    ast::PropName::Ident(i) => i.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    ast::PropName::Num(n) => crate::lower::number_to_js_key(n.value),
                    // #5592: a computed accessor key (`get [expr]()` /
                    // `set [expr](v)`) isn't statically known. Mark the class so
                    // `obj.prototype.<x> = v` writes route through the generic
                    // setter-invoking path rather than a name-keyed prototype
                    // monkey-patch.
                    ast::PropName::Computed(_) => {
                        accessor_names.has_computed = true;
                        continue;
                    }
                    _ => continue,
                };
                match m.kind {
                    ast::MethodKind::Getter => {
                        accessor_names.insert_getter(key);
                    }
                    ast::MethodKind::Setter => {
                        accessor_names.insert_setter(key);
                    }
                    _ => {}
                }
            }
            ast::ClassMember::PrivateMethod(m)
                if !m.is_static
                    && m.function.body.is_some()
                    && matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
            {
                let key = format!("#{}", m.key.name);
                match m.kind {
                    ast::MethodKind::Getter => {
                        accessor_names.insert_getter(key);
                    }
                    ast::MethodKind::Setter => {
                        accessor_names.insert_setter(key);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    accessor_names
}

pub(super) fn lower_generic_computed_class_member(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    computed: &ast::ComputedPropName,
    source_order: usize,
) -> Result<ClassComputedMember> {
    // `[inspect.custom]` names the registered symbol
    // `Symbol.for("nodejs.util.inspect.custom")` (node's own definition), so
    // it is a known key like any `Symbol.for("lit")`.
    let key_expr = if crate::lower_decl::helpers::is_inspect_custom_key(ctx, &computed.expr) {
        Expr::SymbolFor(Box::new(Expr::String(
            "nodejs.util.inspect.custom".to_string(),
        )))
    } else {
        lower_expr(ctx, &computed.expr)?
    };
    let function_name = computed_member_name(method.kind, computed);
    let (kind, function) = match method.kind {
        ast::MethodKind::Method => (
            ClassComputedMemberKind::Method,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_class_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Getter => (
            ClassComputedMemberKind::Getter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_getter_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Setter => (
            ClassComputedMemberKind::Setter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_setter_method_with_name(ctx, method, function_name)
            })?,
        ),
    };
    Ok(ClassComputedMember {
        key_expr,
        function,
        is_static: method.is_static,
        kind,
        source_order,
    })
}

pub(super) fn noncomputed_member_registration_name(
    kind: ast::MethodKind,
    method: &ast::ClassMethod,
) -> String {
    let base = match kind {
        ast::MethodKind::Method => "__computed_method_named",
        ast::MethodKind::Getter => "__computed_getter_named",
        ast::MethodKind::Setter => "__computed_setter_named",
    };
    format!("{}_{}_{}", base, method.span.lo.0, method.span.hi.0)
}

/// #9413: retain a class's original source text keyed by ClassId so
/// `Function.prototype.toString` can reconstruct it, mirroring
/// `capture_function_source` (#4101) for functions. SWC anchors
/// `ast::Class::span` at the `class` keyword (decorators sit outside it) and
/// closes it at the class body's `}`, so the slice is exactly the class source
/// node's `[[SourceText]]`. A no-op when no module source is installed (unit
/// tests / `check`), and idempotent — last write wins, matching the name
/// registry.
pub(crate) fn capture_class_source(
    ctx: &mut LoweringContext,
    class_id: crate::ClassId,
    class: &ast::Class,
) {
    if let Some(src) = crate::ir::current_module_source_range(class.span.lo.0, class.span.hi.0) {
        ctx.class_source_text.insert(class_id, src);
    }
}

/// Record one class accessor, honouring ECMA-262's "a later definition of the
/// same key replaces the earlier one".
///
/// `ClassDecl::getters` / `::setters` are consumed with `iter().find(...)`, so
/// the FIRST entry with a given name wins at lookup time. Appending
/// unconditionally therefore keeps a *shadowed* accessor alive and silently
/// drops the one the program actually defines last:
///
/// ```js
/// class Spring3 {
///   get z() { return this.a.z; }   // damping — shadowed
///   get z() { return this.c.x; }   // displacement — must win
/// }
/// ```
///
/// Perry returned `this.a.z` here while every other engine returns
/// `this.c.x`. In Claude-of-Duty that handed the viewmodel rig a spring's
/// DAMPING COEFFICIENT (0.46) where it wanted a Z displacement, pushing the
/// weapon 0.88 m behind the camera, where it clipped and drew nothing.
///
/// Static and instance accessors are distinct properties (one lives on the
/// constructor, one on the prototype) and may legally share a name, so the
/// replacement is keyed on `(name, is_static)` rather than the name alone.
pub(super) fn record_class_accessor(
    list: &mut Vec<(String, Function)>,
    statics: &mut Vec<bool>,
    name: String,
    func: Function,
    is_static: bool,
) {
    let existing = list
        .iter()
        .enumerate()
        .find_map(|(i, (n, _))| (n == &name && statics[i] == is_static).then_some(i));
    match existing {
        Some(i) => list[i] = (name, func),
        None => {
            list.push((name, func));
            statics.push(is_static);
        }
    }
}
