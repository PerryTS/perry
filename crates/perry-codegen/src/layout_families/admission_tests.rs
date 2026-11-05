//! Admission of keyless families and of derived constructors that run code
//! before `super(...)`.
use super::*;

fn new(class: &str) -> Expr {
    Expr::New {
        byte_end: 0,
        class_name: class.into(),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn set(object: Expr, property: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(object),
        property: property.into(),
        value: Box::new(value),
        byte_offset: 0,
        byte_end: 0,
    })
}

#[test]
fn an_instance_stored_into_another_object_is_a_filled_record() {
    let mut m = HirModule::new("m");
    m.init = vec![
        // `links[id] ?? (links[id] = new Links())`
        Stmt::Expr(Expr::Logical {
            op: perry_hir::LogicalOp::Coalesce,
            left: Box::new(Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::LocalGet(2)),
            }),
            right: Box::new(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::LocalGet(2)),
                value: Box::new(new("Links")),
                byte_offset: 0,
                byte_end: 0,
            }),
        }),
        set(Expr::LocalGet(3), "links", new("Linked")),
        // A constructor's own part, an argument, a thrown value: not filled.
        set(Expr::This, "strategy", new("Strategy")),
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::LocalGet(4)),
            args: vec![new("Argument")],
            type_args: vec![],
            byte_offset: 0,
        }),
        Stmt::Throw(new("Thrown")),
    ];
    let filled = filled_records(&[&m]);
    assert_eq!(
        filled,
        BTreeSet::from(["Linked".to_string(), "Links".to_string()])
    );
}

#[test]
fn a_statement_before_super_never_names_the_receiver_or_returns() {
    let guard = Stmt::If {
        condition: Expr::LocalGet(1),
        then_branch: vec![Stmt::Throw(new("Error"))],
        else_branch: None,
    };
    assert!(before_super_free(&guard));
    assert!(!receiver_free(&guard));
    let store = set(Expr::LocalGet(1), "x", Expr::This);
    assert!(!before_super_free(&store));
    let early = Stmt::If {
        condition: Expr::LocalGet(1),
        then_branch: vec![Stmt::Return(Some(Expr::Object(vec![])))],
        else_branch: None,
    };
    assert!(!before_super_free(&early));
}
