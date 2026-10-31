//! `Expr::clone` copies without recursion per nesting level
//! (`walker/expr_clone.rs`): a deep chain copies on a test thread's stack,
//! and every copy is the original.

use perry_diagnostics::SourceCache;
use perry_hir::walker::walk_expr_children_mut;
use perry_hir::{lower_module, BinaryOp, Expr};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "expr_clone.ts", &mut cache).expect("parses");
    lower_module(&parsed.module, "test", "expr_clone.ts").expect("lowers")
}

/// `o.k0 + o.k1 + … + o.k{n-1}`, left-deep.
fn chain(n: usize) -> Expr {
    let key = |i: usize| Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(0)),
        property: format!("k{i}"),
        byte_offset: 0,
        byte_end: 0,
    };
    let mut e = key(0);
    for i in 1..n {
        e = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(e),
            right: Box::new(key(i)),
        };
    }
    e
}

/// Drop `e` a node at a time (the derived drop recurses per level).
fn dismantle(e: Expr) {
    let mut todo = vec![e];
    while let Some(mut e) = todo.pop() {
        walk_expr_children_mut(&mut e, &mut |c| {
            todo.push(std::mem::replace(c, Expr::Undefined))
        });
    }
}

#[test]
fn a_deep_chain_copies_on_a_test_thread_stack() {
    // A derived clone took a frame of about 16 KB per level in a debug build:
    // 10,000 levels are 160 MB, far beyond the 2 MB of a test thread.
    let n = 10_000;
    let e = chain(n);
    let copy = e.clone();
    // The copy is the chain: its left spine, the key read at each level.
    let mut at = &copy;
    for i in (1..n).rev() {
        let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = at
        else {
            panic!("level {i}: {at:?}")
        };
        assert_eq!(
            format!("{right:?}"),
            format!("{:?}", chain(1)).replace("k0", &format!("k{i}"))
        );
        at = left;
    }
    assert_eq!(format!("{at:?}"), format!("{:?}", chain(1)));
    dismantle(copy);
    dismantle(e);
}

#[test]
fn a_copy_is_the_original() {
    // Every kind of child the walkers visit: boxed and listed operands,
    // call and spread arguments, array holes and spreads, object values,
    // parameter defaults, closure bodies (statements), class members,
    // templates, optional chains.
    let src = r#"
        function f(a: number, b = a + 1, ...rest: number[]) { return a + b + rest.length; }
        const xs = [1, , 3, ...[4, 5]];
        const o: any = { a: 1, ["b" + 1]: 2, ...{ c: 3 }, m(x: number) { return x * 2; } };
        const g = (x = () => o.a, y = xs[0]) => x() + y;
        class C {
          s = 1;
          static t = f(1);
          constructor(public p: number = 2) {}
          get q() { return this.p ?? this.s; }
          m(...a: number[]) { return Math.max(...a, this.s); }
        }
        const c = new C();
        const t = `x${c.q}y${o?.a?.b}z`;
        const r = f(...xs.map((v) => v ?? 0), g(), c.m(1, 2));
        let k = 0;
        for (const v of xs) { k += v ? v : -1; }
        console.log(typeof r === "number" && !(r instanceof C) ? t : [r, k], delete o.a);
    "#;
    let m = lower_src(src);
    let copy = m.clone();
    assert_eq!(format!("{copy:#?}"), format!("{m:#?}"));
}
