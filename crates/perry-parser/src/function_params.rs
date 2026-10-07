//! The full syntactic parameter list exposed by the previous SWC AST.

use std::borrow::Cow;
use swc_ecma_ast::{BindingIdent, Function, Ident, Param, Pat, TsThisParam};

fn this_pattern(param: TsThisParam) -> Pat {
    Pat::Ident(BindingIdent {
        id: Ident::new_no_ctxt("this".into(), param.this_span),
        type_ann: param.type_ann,
    })
}

/// SWC now separates the type-only `this` annotation from ordinary params.
/// Consumers of syntactic positions and signature metadata still need the
/// same sequence, including that marker. Runtime HIR arity excludes it.
pub fn function_parameter_patterns(function: &Function) -> impl Iterator<Item = Cow<'_, Pat>> {
    function
        .this_param
        .iter()
        .map(|param| Cow::Owned(this_pattern((**param).clone())))
        .chain(
            function
                .params
                .iter()
                .map(|param| Cow::Borrowed(&param.pat)),
        )
}

/// Moving form for the interpreter's existing owned parameter-pattern list.
pub fn into_function_parameter_patterns(
    this_param: Option<Box<TsThisParam>>,
    params: Vec<Param>,
) -> impl Iterator<Item = Pat> {
    this_param
        .into_iter()
        .map(|param| this_pattern(*param))
        .chain(params.into_iter().map(|param| param.pat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_ecma_ast::{Decl, ModuleItem, Stmt};

    #[test]
    fn preserves_this_marker_positions_annotations_and_spans() {
        let module = crate::parse_typescript(
            "function callback(this: string, value: number) {}",
            "callback.ts",
        )
        .unwrap();
        let ModuleItem::Stmt(Stmt::Decl(Decl::Fn(decl))) = &module.body[0] else {
            panic!("expected function");
        };
        let patterns: Vec<_> = function_parameter_patterns(&decl.function).collect();
        assert_eq!(patterns.len(), 2);
        let Pat::Ident(marker) = patterns[0].as_ref() else {
            panic!("expected this marker");
        };
        assert_eq!(marker.id.sym.as_ref(), "this");
        assert_eq!(
            marker.id.span,
            decl.function.this_param.as_ref().unwrap().this_span
        );
        assert!(marker.type_ann.is_some());
        assert!(matches!(patterns[1], Cow::Borrowed(_)));
        let Pat::Ident(value) = patterns[1].as_ref() else {
            panic!("expected value parameter");
        };
        assert_eq!(value.id.sym.as_ref(), "value");
        assert!(value.type_ann.is_some());
        let owned = (*decl.function).clone();
        let moved: Vec<_> =
            into_function_parameter_patterns(owned.this_param, owned.params).collect();
        assert_eq!(
            moved,
            patterns
                .into_iter()
                .map(Cow::into_owned)
                .collect::<Vec<_>>()
        );
    }
}
