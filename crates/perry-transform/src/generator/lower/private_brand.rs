//! Preserve a generator's lexical private-name environment across suspension.
use super::*;
use crate::closure_local_inline::{for_each_expr_in_stmt_mut, nested_stmt_lists};
use perry_hir::walker::walk_expr_children_mut;

fn visit_body(body: &mut [Stmt], f: &mut dyn FnMut(&mut Expr)) {
    for stmt in body {
        for_each_expr_in_stmt_mut(stmt, &mut |expr| visit_expr(expr, f));
        for nested in nested_stmt_lists(stmt) {
            visit_body(nested, f);
        }
    }
}

fn visit_expr(expr: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    walk_expr_children_mut(expr, &mut |child| visit_expr(child, f));
    if let Expr::Closure { body, .. } = expr {
        visit_body(body, f);
    }
    f(expr);
}

pub(super) fn uses_private_names(body: &mut [Stmt]) -> bool {
    let mut found = false;
    visit_body(body, &mut |expr| {
        found |= matches!(
            expr,
            Expr::PrivateGuard { .. } | Expr::PrivateBrandCheck { .. }
        );
    });
    found
}

/// Give every plain function that reads private names the class evaluation
/// it was created in, as V8 does: its private-name environment is the class
/// body's, fixed when the closure is created, not the receiver's or the
/// caller's. Two evaluations of one class body (`mk()` called twice) produce
/// closures with distinct brands, so `X.read(Y)` throws for a `#s` read in
/// a `function` created by X's static block.
///
/// The enclosing activation captures the current lexical brand
/// (`PrivateLexicalBrand`, a method's/static block's own evaluation) right
/// before the statement that creates the closure; the closure pushes it on
/// entry and pops it on every exit, exactly as generator continuations
/// restore theirs. Arrow functions are left alone: their `this` is lexical,
/// and the receiver-derived brand is already the right evaluation. Async and
/// generator functions get their environment from the generator lowering.
pub(crate) fn restore_in_class_closures(module: &mut Module, next_local_id: &mut u32) {
    // A closure that sits directly in an expression-only position (a class
    // field or global initializer) has no statement list to capture into,
    // and lowering copies such initializers into other bodies (fields into
    // the constructor). Every copy of one closure must keep one capture
    // layout, so those closures are never wrapped, wherever they appear.
    let mut blocked = std::collections::HashSet::new();
    {
        let mut block = |expr: &Expr| collect_direct_closures(expr, &mut blocked);
        for class in &module.classes {
            for field in class.fields.iter().chain(&class.static_fields) {
                field
                    .init
                    .iter()
                    .chain(&field.key_expr)
                    .for_each(&mut block);
            }
        }
        for global in &module.globals {
            global.init.iter().for_each(&mut block);
        }
    }
    let mut cx = Cx {
        next_local_id,
        blocked: &blocked,
    };
    for func in &mut module.functions {
        process_body(&mut func.body, true, &mut cx);
    }
    process_body(&mut module.init, true, &mut cx);
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            process_body(&mut ctor.body, true, &mut cx);
        }
        for method in class
            .methods
            .iter_mut()
            .chain(class.static_methods.iter_mut())
        {
            process_body(&mut method.body, true, &mut cx);
        }
        for (_, accessor) in class.getters.iter_mut().chain(class.setters.iter_mut()) {
            process_body(&mut accessor.body, true, &mut cx);
        }
        for member in &mut class.computed_members {
            process_body(&mut member.function.body, true, &mut cx);
        }
        for field in class
            .fields
            .iter_mut()
            .chain(class.static_fields.iter_mut())
        {
            for expr in field.init.iter_mut().chain(field.key_expr.iter_mut()) {
                process_expression_only(expr, &mut cx);
            }
        }
    }
    for global in &mut module.globals {
        if let Some(expr) = &mut global.init {
            process_expression_only(expr, &mut cx);
        }
    }
}

struct Cx<'a> {
    next_local_id: &'a mut u32,
    blocked: &'a std::collections::HashSet<u32>,
}

/// Closures directly in `expr` (not inside another closure's body).
fn collect_direct_closures(expr: &Expr, out: &mut std::collections::HashSet<u32>) {
    if let Expr::Closure { func_id, .. } = expr {
        out.insert(*func_id);
        return;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| collect_direct_closures(child, out));
}

/// An expression-only position: closures in it stay as they are, but the
/// bodies they contain are processed like any other body.
fn process_expression_only(expr: &mut Expr, cx: &mut Cx) {
    if let Expr::Closure {
        body,
        is_arrow,
        captures_this,
        ..
    } = expr
    {
        let inner_this = !*is_arrow || *captures_this;
        process_body(body, inner_this, cx);
        return;
    }
    walk_expr_children_mut(expr, &mut |child| process_expression_only(child, cx));
}

/// `this_usable`: whether `Expr::This` may be evaluated in this body (an
/// arrow that does not already capture `this` must not start to).
fn process_body(body: &mut Vec<Stmt>, this_usable: bool, cx: &mut Cx) {
    let mut index = 0;
    while index < body.len() {
        for nested in nested_stmt_lists(&mut body[index]) {
            process_body(nested, this_usable, cx);
        }
        let mut captures = Vec::new();
        for_each_expr_in_stmt_mut(&mut body[index], &mut |expr| {
            wrap_closures(expr, this_usable, cx, &mut captures)
        });
        let added = captures.len();
        for (offset, stmt) in captures.into_iter().enumerate() {
            body.insert(index + offset, stmt);
        }
        index += added + 1;
    }
}

fn wrap_closures(expr: &mut Expr, this_usable: bool, cx: &mut Cx, captures_out: &mut Vec<Stmt>) {
    if let Expr::Closure {
        func_id,
        body,
        captures,
        captures_this,
        is_arrow,
        is_async,
        is_generator,
        ..
    } = expr
    {
        let inner_this = !*is_arrow || *captures_this;
        process_body(body, inner_this, cx);
        if *is_arrow
            || *is_async
            || *is_generator
            || cx.blocked.contains(func_id)
            || !uses_private_names(body)
        {
            return;
        }
        let brand_id = alloc_local(cx.next_local_id);
        captures_out.push(Stmt::Let {
            id: brand_id,
            name: "__private_brand".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PrivateLexicalBrand(Box::new(if this_usable {
                Expr::This
            } else {
                Expr::Undefined
            }))),
        });
        // Returns were already given their finally blocks inline by the
        // finally/return normalization; pop explicitly before each, and let
        // the finally arm cover throws and fallthrough.
        for stmt in body.iter_mut() {
            pop_before_return(stmt, cx.next_local_id);
        }
        let original = std::mem::take(body);
        *body = vec![
            Stmt::Expr(Expr::PrivateLexicalBrandPush(Box::new(Expr::LocalGet(
                brand_id,
            )))),
            Stmt::Try {
                body: original,
                catch: None,
                finally: Some(vec![Stmt::Expr(Expr::PrivateLexicalBrandPop)]),
            },
        ];
        captures.push(brand_id);
        return;
    }
    walk_expr_children_mut(expr, &mut |child| {
        wrap_closures(child, this_usable, cx, captures_out)
    });
}

pub(super) fn restore_in_continuations(
    body: &mut [Stmt],
    brand_id: LocalId,
    next_local_id: &mut u32,
) {
    visit_body(body, &mut |expr| {
        if let Expr::Closure { captures, body, .. } = expr {
            if captures.contains(&brand_id) {
                // Generator lowering runs after the source finally/return
                // normalization pass. Explicitly clean up normal returns too;
                // the finally arm below handles exceptions and fallthrough.
                for stmt in body.iter_mut() {
                    pop_before_return(stmt, next_local_id);
                }
                let resume = std::mem::take(body);
                *body = vec![
                    Stmt::Expr(Expr::PrivateLexicalBrandPush(Box::new(Expr::LocalGet(
                        brand_id,
                    )))),
                    Stmt::Try {
                        body: resume,
                        catch: None,
                        finally: Some(vec![Stmt::Expr(Expr::PrivateLexicalBrandPop)]),
                    },
                ];
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closure(is_arrow: bool, body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id: 40,
            params: vec![],
            return_type: Type::Any,
            body,
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    fn private_read() -> Expr {
        Expr::PrivateGuard {
            class_name: "C".into(),
            class_id: 7,
            field_name: "#s".into(),
            kind: 0,
            op: 0,
            receiver_is_brand_owner: false,
            object: Box::new(Expr::This),
        }
    }

    /// `static { this.read = function (o) { return o.#s; }; }`: the
    /// function captures the evaluation's brand where it is created and
    /// restores it on entry, on return and through the finally arm; an arrow
    /// and a function without private names are untouched.
    #[test]
    fn plain_functions_in_class_bodies_capture_their_evaluation() {
        let mut body = vec![
            Stmt::Expr(closure(false, vec![Stmt::Return(Some(private_read()))])),
            Stmt::Expr(closure(true, vec![Stmt::Return(Some(private_read()))])),
            Stmt::Expr(closure(false, vec![Stmt::Return(Some(Expr::Undefined))])),
        ];
        let mut next = 100;
        let blocked = std::collections::HashSet::new();
        let mut cx = Cx {
            next_local_id: &mut next,
            blocked: &blocked,
        };
        process_body(&mut body, true, &mut cx);
        assert_eq!(
            body.len(),
            4,
            "one capture statement for the one plain private function"
        );
        let brand = match &body[0] {
            Stmt::Let {
                id,
                init: Some(Expr::PrivateLexicalBrand(receiver)),
                ..
            } => {
                assert!(matches!(**receiver, Expr::This));
                *id
            }
            other => panic!("expected the brand capture first, got {other:?}"),
        };
        let Stmt::Expr(Expr::Closure {
            body: wrapped,
            captures,
            ..
        }) = &body[1]
        else {
            panic!("closure moved");
        };
        assert!(captures.contains(&brand));
        assert!(matches!(
            &wrapped[0],
            Stmt::Expr(Expr::PrivateLexicalBrandPush(_))
        ));
        let Stmt::Try {
            body: inner,
            finally: Some(finally),
            ..
        } = &wrapped[1]
        else {
            panic!("missing finally");
        };
        assert!(matches!(
            finally.as_slice(),
            [Stmt::Expr(Expr::PrivateLexicalBrandPop)]
        ));
        assert_return_cleanup(&mut inner.clone());
        for untouched in &body[2..] {
            let Stmt::Expr(Expr::Closure { body, captures, .. }) = untouched else {
                panic!("expected closure");
            };
            assert!(captures.is_empty());
            assert!(matches!(body.as_slice(), [Stmt::Return(_)]));
        }
        // Inside an arrow that does not capture `this`, the capture must not
        // introduce a `this` read.
        let mut in_arrow = vec![Stmt::Expr(closure(
            true,
            vec![Stmt::Expr(closure(
                false,
                vec![Stmt::Return(Some(private_read()))],
            ))],
        ))];
        process_body(&mut in_arrow, true, &mut cx);
        let Stmt::Expr(Expr::Closure {
            body: arrow_body, ..
        }) = &in_arrow[0]
        else {
            panic!("arrow moved");
        };
        assert!(
            matches!(&arrow_body[0], Stmt::Let { init: Some(Expr::PrivateLexicalBrand(r)), .. }
            if matches!(**r, Expr::Undefined))
        );
    }

    /// A closure that is also a class-field initializer (copied into the
    /// constructor) keeps one capture layout: never wrapped in any copy.
    #[test]
    fn closures_in_expression_only_positions_are_never_wrapped() {
        let mut ctor_copy = vec![Stmt::Expr(closure(
            false,
            vec![Stmt::Return(Some(private_read()))],
        ))];
        let mut field_init = closure(false, vec![Stmt::Return(Some(private_read()))]);
        let mut blocked = std::collections::HashSet::new();
        collect_direct_closures(&field_init, &mut blocked);
        let mut next = 100;
        let mut cx = Cx {
            next_local_id: &mut next,
            blocked: &blocked,
        };
        process_body(&mut ctor_copy, true, &mut cx);
        process_expression_only(&mut field_init, &mut cx);
        assert_eq!(
            ctor_copy.len(),
            1,
            "no capture statement for a blocked closure"
        );
        for copy in [&ctor_copy[0], &Stmt::Expr(field_init)] {
            let Stmt::Expr(Expr::Closure { captures, body, .. }) = copy else {
                panic!()
            };
            assert!(captures.is_empty());
            assert!(matches!(body.as_slice(), [Stmt::Return(_)]));
        }
    }

    fn assert_return_cleanup(body: &mut Vec<Stmt>) {
        for index in 0..body.len() {
            if matches!(body[index], Stmt::Return(_)) {
                assert!(
                    index > 0
                        && matches!(&body[index - 1], Stmt::Expr(Expr::PrivateLexicalBrandPop)),
                    "normal returns must pop the environment, not bypass the finally"
                );
            }
            for nested in nested_stmt_lists(&mut body[index]) {
                assert_return_cleanup(nested);
            }
        }
    }

    #[test]
    fn private_generator_continuations_restore_captured_environment() {
        for is_async in [false, true] {
            let mut function = Function {
                id: 1,
                name: "gen".into(),
                type_params: vec![],
                params: vec![],
                return_type: Type::Any,
                body: vec![Stmt::Expr(Expr::Yield {
                    delegate: false,
                    value: Some(Box::new(Expr::PrivateGuard {
                        class_name: "C".into(),
                        class_id: 7,
                        field_name: "#v".into(),
                        kind: 0,
                        op: 0,
                        receiver_is_brand_owner: false,
                        object: Box::new(Expr::This),
                    })),
                })],
                is_strict: true,
                is_async,
                is_generator: true,
                is_exported: false,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            };
            let mut local = 10;
            let mut func_id = 10;
            transform_generator_function(&mut function, &mut local, &mut func_id);
            let brand = function
                .body
                .iter()
                .find_map(|stmt| match stmt {
                    Stmt::Let {
                        id,
                        init: Some(Expr::PrivateLexicalBrand(_)),
                        ..
                    } => Some(*id),
                    _ => None,
                })
                .expect("capture lexical brand before the generator body starts");
            let mut continuations = 0;
            visit_body(&mut function.body, &mut |expr| {
                if let Expr::Closure { captures, body, .. } = expr {
                    if captures.contains(&brand) {
                        continuations += 1;
                        assert_return_cleanup(body);
                        assert!(matches!(
                            body.first(),
                            Some(Stmt::Expr(Expr::PrivateLexicalBrandPush(_)))
                        ));
                        assert!(matches!(&body[1], Stmt::Try { finally: Some(finally), .. }
                            if matches!(finally.as_slice(), [Stmt::Expr(Expr::PrivateLexicalBrandPop)])));
                    }
                }
            });
            // next/return/throw share one resume closure. It must capture and
            // restore the brand, including return cleanup and the finally arm.
            assert_eq!(continuations, 1, "the shared resume must restore the brand");
        }
    }
}

fn pop_before_return(stmt: &mut Stmt, next_local_id: &mut u32) {
    if let Stmt::Return(value) = stmt {
        let mut sequence = Vec::new();
        let result = value.take().map(|value| {
            let id = alloc_local(next_local_id);
            sequence.push(Stmt::Let {
                id,
                name: "__gen_private_result".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(value),
            });
            Expr::LocalGet(id)
        });
        sequence.push(Stmt::Expr(Expr::PrivateLexicalBrandPop));
        sequence.push(Stmt::Return(result));
        *stmt = Stmt::If {
            condition: Expr::Bool(true),
            then_branch: sequence,
            else_branch: None,
        };
    } else if let Stmt::Labeled { body, .. } = stmt {
        pop_before_return(body, next_local_id);
    } else {
        for body in nested_stmt_lists(stmt) {
            for stmt in body {
                pop_before_return(stmt, next_local_id);
            }
        }
    }
}
