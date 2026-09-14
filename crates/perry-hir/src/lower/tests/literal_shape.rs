use super::*;

#[test]
fn literal_shape_proof_rejects_constructor_effects_and_nonliteral_classes() {
    let mut ctx = make_ctx();
    ctx.synthesize_anon_shape_class(&[("id".into(), Type::Number), ("name".into(), Type::String)]);
    let shape = ctx.pending_classes.pop().unwrap();
    assert!(shape.is_literal_shape(), "the actual lowering must qualify");
    let mut changed = shape.clone();
    changed
        .constructor
        .as_mut()
        .unwrap()
        .body
        .push(Stmt::Expr(Expr::Number(1.0)));
    assert!(!changed.is_literal_shape());
    let mut changed = shape.clone();
    changed.constructor.as_mut().unwrap().params[0].default = Some(Expr::Number(1.0));
    assert!(!changed.is_literal_shape());
    let mut changed = shape.clone();
    changed.fields[0].init = Some(Expr::Number(1.0));
    assert!(!changed.is_literal_shape());
    let mut changed = shape.clone();
    changed.extends_name = Some("Base".into());
    assert!(!changed.is_literal_shape());
    let mut changed = shape.clone();
    changed.name = "UserClass".into();
    assert!(!changed.is_literal_shape());
    let mut changed = shape;
    changed.constructor.as_mut().unwrap().body.swap(0, 1);
    assert!(
        !changed.is_literal_shape(),
        "field order is part of the proof"
    );
}
