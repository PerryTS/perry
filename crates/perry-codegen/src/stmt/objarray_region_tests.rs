//! Bounded object-element loads must enter the existing shape/Number region.
use super::*;

fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn fixture(mask: i64) -> Module {
    let mut m = method_calls_module(Expr::Integer(200_000), Vec::new(), false);
    let mut y = m.classes[0].fields[0].clone();
    y.name = "y".into();
    m.classes[0].fields.push(y);
    m.classes[0].constructor = Some(assigning_ctor(&m.classes[0].fields));
    m.init[0] = Stmt::Let {
        id: 1,
        name: "rows".into(),
        ty: Type::Array(Box::new(Type::Named("Counter".into()))),
        mutable: false,
        init: Some(Expr::Array(
            (0..8)
                .map(|i| Expr::New {
                    class_name: "Counter".into(),
                    args: vec![Expr::Integer(i), Expr::Integer(3)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                })
                .collect(),
            None,
        )),
    };
    let get = |property: &str| Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(10)),
        property: property.into(),
        byte_offset: 0,
    };
    let Stmt::For { body, .. } = &mut m.init[1] else {
        panic!("loop")
    };
    *body = vec![
        Stmt::Let {
            id: 10,
            name: "row".into(),
            ty: Type::Named("Counter".into()),
            mutable: false,
            init: Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(binary(
                    BinaryOp::BitAnd,
                    Expr::LocalGet(7),
                    Expr::Integer(mask),
                )),
            }),
        },
        Stmt::Expr(Expr::LocalSet(
            11,
            Box::new(binary(
                BinaryOp::Add,
                Expr::LocalGet(11),
                binary(BinaryOp::Mul, get("value"), get("y")),
            )),
        )),
    ];
    m.init.insert(
        1,
        Stmt::Let {
            id: 11,
            name: "sum".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(0)),
        },
    );
    m
}

fn ir(m: &Module) -> String {
    let births = crate::module_birth_shapes(m, ir_opts()).unwrap();
    let mut opts = ir_opts();
    opts.static_shape_ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    opts.verify_native_regions = true;
    String::from_utf8(compile_module(m, opts).unwrap()).unwrap()
}

/// Object model step 7 deleted the array/element loop regions: a bounded
/// loop over object elements lowers through checked element loads, and the
/// live numeric arm still multiplies.
#[test]
fn bounded_object_elements_use_checked_element_loads() {
    let ir = ir(&fixture(7));
    assert!(
        !ir.contains("rloop.version.split") && !ir.contains("rloop.element.join"),
        "no array/element region forms: {ir}"
    );
    assert!(
        ir.contains("@js_array_get") || ir.contains("bidx.header") || ir.contains("arr_get"),
        "{ir}"
    );
    assert!(
        ir.contains("fmul double"),
        "the live numeric arm must multiply: {ir}"
    );
}

#[test]
fn unbounded_signed_mask_keeps_checked_element_loads() {
    let ir = ir(&fixture(-1));
    assert!(
        !ir.contains("rloop.version.split"),
        "a negative mask does not prove a bounded window: {ir}"
    );
    assert!(
        ir.contains("@js_array_get") || ir.contains("bidx.header") || ir.contains("arr_get"),
        "{ir}"
    );
}

#[test]
fn boxed_bound_element_loop_keeps_checked_element_loads() {
    let mut m = fixture(7);
    let mut f = m.classes[0].constructor.clone().unwrap();
    f.id = 100;
    f.name = "kernel".into();
    f.params.truncate(1);
    f.params[0].id = 20;
    f.params[0].name = "n".into();
    f.return_type = Type::Number;
    f.body = std::mem::take(&mut m.init);
    let Stmt::For { condition, .. } = &mut f.body[2] else {
        panic!("loop")
    };
    let Some(Expr::Compare { right, .. }) = condition else {
        panic!("condition")
    };
    *right = Box::new(Expr::LocalGet(20));
    f.body.push(Stmt::Return(Some(Expr::LocalGet(11))));
    m.functions.push(f);
    let ir = ir(&m);
    // Step 7 deleted the element regions: no receiver tail, and the loop's
    // element reads stay checked.
    assert!(
        !ir.contains("rloop.element.join"),
        "no element region forms: {ir}"
    );
    assert!(
        ir.contains("@js_array_get") || ir.contains("bidx.header") || ir.contains("arr_get"),
        "{ir}"
    );
}

#[test]
fn declared_numeric_row_hint_still_requires_its_live_shape() {
    let mut m = fixture(7);
    let mut boxed = m.classes[0].clone();
    boxed.id = 102;
    boxed.name = "BoxedRow".into();
    for field in &mut boxed.fields {
        field.ty = Type::Any;
    }
    boxed.constructor = Some(assigning_ctor(&boxed.fields));
    boxed.constructor.as_mut().unwrap().id = 9001;
    m.classes.push(boxed);
    let Stmt::Let { ty, init, .. } = &mut m.init[0] else {
        panic!("rows")
    };
    *ty = Type::Array(Box::new(Type::Named("BoxedRow".into())));
    *init = Some(Expr::Array(
        (0..8)
            .map(|_| Expr::New {
                class_name: "BoxedRow".into(),
                args: vec![Expr::String("2".into()), Expr::String("3".into())],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            })
            .collect(),
        None,
    ));
    // The row declaration still nominates Counter's numeric lanes. Its
    // array actually carries BoxedRow's Any lanes. The numeric arm must
    // compare the live shape rather than trusting the declaration.
    let births = crate::module_birth_shapes(&m, ir_opts()).unwrap();
    let numeric = births.iter().find(|b| b.class_id == 101).unwrap();
    assert_eq!(
        numeric.shape.rep & 15,
        5,
        "the nominee must have two F64 lanes"
    );
    let ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let identity = ids[&numeric.shape];
    let ir = ir(&m);
    let expected = crate::layout_ordinals::static_word(identity).to_string();
    // Step 7 deleted the element regions; the row receivers' object regions
    // still compare the live shape rather than trusting the declaration.
    assert!(
        !ir.contains("rloop.element.join"),
        "no element region forms: {ir}"
    );
    let mut in_guard = false;
    let mut numeric_guard = false;
    for line in ir.lines() {
        if line.ends_with(':') {
            in_guard = line.starts_with("rloop.guard.static.");
        }
        numeric_guard |= in_guard
            && line.contains("icmp eq i32")
            && line.trim_end().ends_with(&format!(", {expected}"));
    }
    assert!(
        numeric_guard,
        "binding refinement must not replace the numeric supplier: {ir}"
    );
}
