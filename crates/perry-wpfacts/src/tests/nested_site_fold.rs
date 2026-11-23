//! Rule NESTED-SITE-RUN folds a recursion by function, not by token. A
//! function declared inside a closure maker has one token per closure
//! context (`TokKind::Func(f, ctx)`). When its recursive call is a plain
//! call through a variable that holds several closures of it (here `cur`,
//! assigned once per maker run; in three.js the `WebGLShadowMap`
//! `renderObject` recursion), a fold on the token alone lets the chain hold
//! each token once, so the runs are the permutations of the tokens (c08: an
//! endless fill, OOM). Folded on the function, each token runs once per
//! program site.

fn walk_runs(n: usize) -> usize {
    let sets: String = (0..n).map(|i| format!("cur = Maker({i}); ")).collect();
    let src = format!(
        "let cur: any = undefined;\n\
         function Maker(tag: number) {{\n\
           function walk(o: any): number {{ let s = tag; for (const c of o.kids) s += cur(c); return s; }}\n\
           return walk;\n\
         }}\n\
         {sets}\n\
         const root: any = JSON.parse(process.argv[2] ?? \"{{}}\");\n\
         console.log(cur(root));\n"
    );
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.sum
        .insts
        .iter()
        .filter(|i| g.m.funcs[i.f as usize].name.contains("walk"))
        .count()
}

/// The program site `cur(root)` runs each of the `n` closures of `walk`;
/// the recursive `cur(c)` reaches all `n` and folds onto that site: `n`
/// runs (a fold by token alone: 39 for n=3, 3165 for n=5).
#[test]
fn a_recursion_over_many_closures_of_one_function_folds_per_site() {
    for n in [3, 5] {
        let runs = walk_runs(n);
        eprintln!("NESTED-FOLD n={n} walk_runs={runs}");
        assert!(runs >= n, "n={n}: the site runs each closure (got {runs})");
        assert!(runs <= n * n, "n={n}: runs={runs} > {}", n * n);
    }
}
