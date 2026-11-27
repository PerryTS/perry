//! WBOUND (2026-11-25, owner yes 11-25): the solve's work bound
//! (`FactsConfig::max_work`, `Model::past_work_bound`) holds wherever work is
//! charged, also inside one instance fill and one rule replay.
use super::*;

fn run(src: &str, max_work: Option<u64>) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let cfg = FactsConfig {
        max_work,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn fixture(variant: u32, n: usize) -> String {
    let mut s = String::new();
    let reads: Vec<String> = (0..n).map(|i| format!("o.k{i}")).collect();
    let calls: Vec<String> = (0..n).map(|i| format!("o.m{i}()")).collect();
    let lits: Vec<String> = (0..n).map(|i| format!("{{k{i}: {i}}}")).collect();
    let ctors: Vec<String> = (0..n).map(|i| format!("new C{i}()")).collect();
    for i in 0..n {
        let ms: Vec<String> = (0..n)
            .map(|j| format!("m{j}() {{ return {i}; }}"))
            .collect();
        if variant == 1 {
            s += &format!("class C{i} {{ {} }}\n", ms.join(" "));
        }
    }
    match variant {
        0 => s += &format!("function f(o: any) {{ return {}; }}\n", reads.join(" + ")),
        _ => s += &format!("function f(o: any) {{ return {}; }}\n", calls.join(" + ")),
    }
    let elems = if variant == 0 {
        lits.join(", ")
    } else {
        ctors.join(", ")
    };
    s += &format!("const xs: any[] = [{elems}];\n");
    // The callee arrives late: after the elements, through a chain.
    s += "let g: any = f;\n";
    for _ in 0..40 {
        s += "g = [g][0];\n";
    }
    s += "let t = 0; for (const x of xs) t += g(x);\nconsole.log(t);\n";
    s
}

/// RED before WBOUND: the solve looked at the bound only between worklist
/// pops and between fills, so one fill (each rule replays on what its node
/// holds) or one callee replay ran on past it: the stop came at 1.1x-2.4x
/// `WIDENED_WORK` times the bound (v=0 bound 21: 31085 for 21000; bound 1:
/// 2395 for 1000). Now the stop comes at the first charge past it, and the
/// stopped solve claims nothing (sound); below the stop the solve widens and
/// runs to its fixpoint as before.
#[test]
fn wbound_holds_inside_one_fill_and_replay() {
    // v=0: member reads of a parameter that holds n literals (rule replays
    // in the fill); v=1: method calls on n class instances (ThisTo /
    // callee replays).
    for (v, n) in [(0u32, 60usize), (1, 30)] {
        let src = fixture(v, n);
        let full = run(&src, None);
        assert!(
            !full.stats.aborted && !full.stats.widened,
            "v={v}: no bound, no widening"
        );
        assert!(
            full.stats.work > 30_000,
            "v={v}: the fixture does enough work"
        );
        for bound in [1, full.stats.work / 2000] {
            let pf = run(&src, Some(bound));
            assert!(
                pf.stats.aborted && pf.stats.widened_stop,
                "v={v} b={bound}: the stop"
            );
            let stop = bound * 1000;
            assert!(
                pf.stats.work <= stop + 100,
                "v={v} b={bound}: stopped at {} for a stop at {stop}",
                pf.stats.work
            );
            assert!(
                !pf.body_reach.complete,
                "v={v} b={bound}: a stopped solve claims nothing"
            );
        }
        // Past the bound but below the stop: widened, runs to its fixpoint.
        let pf = run(&src, Some(full.stats.work / 10));
        assert!(
            pf.stats.widened && !pf.stats.aborted,
            "v={v}: widened, not stopped"
        );
        assert!(pf.body_reach.complete, "v={v}: a widened solve finishes");
    }
}
