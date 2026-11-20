//! Factory-return results must match the former path traversal, including
//! its two depth limits. The wide graph also pins a bound on semantic work.
use super::*;
use perry_hir::LogicalOp;
use std::collections::HashMap;

// Test-only semantic oracle, copied unchanged from the recursive resolver.
struct Recursive<'a>(&'a Factories);
impl Recursive<'_> {
    /// The class names a fresh instance `e` evaluates to can be of.
    fn instance_classes(
        &self,
        e: &Expr,
        cx: &FactoryCx<'_>,
        depth: u32,
        out: &mut BTreeSet<String>,
    ) {
        if depth > 6 {
            return;
        }
        match e {
            Expr::New { class_name, .. } => {
                out.insert(class_name.clone());
            }
            Expr::NewDynamic { callee, .. } => self.ctor_classes(callee, cx, depth + 1, out),
            Expr::Call { callee, .. } => {
                for f in self.0.callee_fns(callee, cx.fn_of_local) {
                    for r in self.0.returns.get(&f).into_iter().flatten() {
                        self.instance_classes(r, cx, depth + 1, out);
                    }
                }
            }
            // A factory returning its own local (`createBaseDeclaration`):
            // that local's initialising call.
            Expr::LocalGet(l) => {
                if let Some(init) = cx.init_expr.get(l) {
                    self.instance_classes(init, cx, depth + 1, out);
                }
            }
            _ => {}
        }
    }

    /// The class names a constructor value `e` evaluates to can be.
    fn ctor_classes(&self, e: &Expr, cx: &FactoryCx<'_>, depth: u32, out: &mut BTreeSet<String>) {
        if depth > 8 {
            return;
        }
        match e {
            Expr::ClassRef(n) => {
                out.insert(n.clone());
            }
            Expr::ClassExprFresh { template, .. } => {
                out.insert(template.clone());
            }
            Expr::LocalGet(l) => {
                if let Some(n) = cx.class_of_local.get(l) {
                    out.insert(n.clone());
                } else {
                    for v in self.0.assigns.get(l).into_iter().flatten() {
                        self.ctor_classes(v, cx, depth + 1, out);
                    }
                }
            }
            Expr::LocalSet(_, v) => self.ctor_classes(v, cx, depth + 1, out),
            Expr::Logical { left, right, .. } => {
                self.ctor_classes(left, cx, depth + 1, out);
                self.ctor_classes(right, cx, depth + 1, out);
            }
            Expr::Call { callee, .. } => {
                for f in self.0.callee_fns(callee, cx.fn_of_local) {
                    for r in self.0.returns.get(&f).into_iter().flatten() {
                        self.ctor_classes(r, cx, depth + 1, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn call(callee: Expr) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(callee),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    }
}

fn method(name: &str) -> Expr {
    call(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::Undefined),
        property: name.into(),
        byte_offset: 0,
    })
}

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

fn dynamic(callee: Expr) -> Expr {
    Expr::NewDynamic {
        byte_end: 0,
        callee: Box::new(callee),
        args: vec![],
        byte_offset: 0,
    }
}

fn logical(left: Expr, right: Expr) -> Expr {
    Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[test]
fn mixed_factory_graph_matches_recursive_results_at_every_depth() {
    let mut f = Factories::default();
    f.named.insert("make".into(), vec![1, 2, 1]);
    f.named_locals.insert("make".into(), vec![10, 11, 10]);
    f.returns.insert(1, vec![new("A"), call(Expr::FuncRef(3))]);
    f.returns
        .insert(2, vec![Expr::LocalGet(20), method("cycle")]);
    f.returns.insert(3, vec![dynamic(call(Expr::FuncRef(4)))]);
    f.returns.insert(
        4,
        vec![logical(Expr::LocalGet(30), Expr::ClassRef("B".into()))],
    );
    f.named.insert("cycle".into(), vec![2, 5]);
    f.returns.insert(5, vec![new("C"), Expr::LocalGet(21)]);
    f.assigns.insert(
        30,
        vec![
            Expr::LocalSet(31, Box::new(Expr::ClassRef("D".into()))),
            call(Expr::FuncRef(4)),
        ],
    );
    f.assigns
        .insert(31, vec![Expr::ClassRef("IgnoredWhenClassBound".into())]);
    let fn_of_local = [(10, 3), (11, 1)].into_iter().collect();
    let init_expr = [(20, new("E")), (21, call(Expr::LocalGet(10)))]
        .into_iter()
        .collect();
    let class_of_local = [(31, "F".into())].into_iter().collect();
    let cx = FactoryCx {
        fn_of_local: &fn_of_local,
        init_expr: &init_expr,
        class_of_local: &class_of_local,
    };
    let roots = [
        method("make"),
        call(Expr::LocalGet(10)),
        Expr::LocalGet(21),
        dynamic(Expr::LocalGet(30)),
        dynamic(Expr::LocalGet(31)),
        logical(Expr::ClassRef("Direct".into()), call(Expr::FuncRef(4))),
        new("DirectNew"),
        Expr::ClassRef("DirectClass".into()),
        Expr::Undefined,
    ];
    for (index, root) in roots.iter().enumerate() {
        for depth in 0..=10 {
            for mode in [FactoryMode::Instance, FactoryMode::Constructor] {
                let mut expected = BTreeSet::new();
                let mut actual = BTreeSet::new();
                match mode {
                    FactoryMode::Instance => {
                        Recursive(&f).instance_classes(root, &cx, depth, &mut expected)
                    }
                    FactoryMode::Constructor => {
                        Recursive(&f).ctor_classes(root, &cx, depth, &mut expected)
                    }
                }
                f.walk_classes(root, &cx, mode, depth, &mut actual);
                assert_eq!(
                    actual,
                    expected,
                    "root={index}, depth={depth}, constructor={}",
                    mode == FactoryMode::Constructor
                );
            }
        }
    }
}

#[test]
fn ambiguous_method_diamonds_are_expanded_by_graph_distance_not_path_count() {
    const WIDTH: u32 = 128;
    let mut f = Factories::default();
    f.named.insert("make".into(), (0..WIDTH).collect());
    for id in 0..WIDTH {
        f.returns.insert(id, vec![method("make"), new("Result")]);
    }
    let fn_of_local = FxHashMap::default();
    let init_expr = FxHashMap::default();
    let class_of_local = HashMap::new();
    let cx = FactoryCx {
        fn_of_local: &fn_of_local,
        init_expr: &init_expr,
        class_of_local: &class_of_local,
    };
    let mut classes = BTreeSet::new();
    let work = f.walk_classes(&method("make"), &cx, FactoryMode::Instance, 0, &mut classes);
    assert_eq!(classes, BTreeSet::from(["Result".into()]));
    // Six return layers, two expressions per function; the recursive walk
    // follows more than WIDTH^6 paths through the very same finite graph.
    assert!(
        work <= 1 + 6 * 2 * WIDTH as usize,
        "expanded {work} expressions"
    );
}

#[test]
fn constructor_transition_keeps_the_extra_two_levels() {
    let f = Factories::default();
    let fn_of_local = FxHashMap::default();
    let init_expr = FxHashMap::default();
    let class_of_local = HashMap::new();
    let cx = FactoryCx {
        fn_of_local: &fn_of_local,
        init_expr: &init_expr,
        class_of_local: &class_of_local,
    };
    let root = dynamic(logical(Expr::ClassRef("AtEight".into()), Expr::Undefined));
    let mut classes = BTreeSet::new();
    f.walk_classes(&root, &cx, FactoryMode::Instance, 6, &mut classes);
    assert_eq!(classes, BTreeSet::from(["AtEight".into()]));
    classes.clear();
    f.walk_classes(&root, &cx, FactoryMode::Instance, 7, &mut classes);
    assert!(classes.is_empty());
}
