//! COST A: past the work bound the solve widens (`Model::past_work_bound`,
//! `Model::add_tokens`) and finishes: facts no later arrival touched stay,
//! the rest are unknown. Before, the program got no facts at all.
use super::*;

/// A cheap precise part (`a.v`) and an expensive product after it: N
/// receivers times N methods (CXCTX: 2N^2+N instances).
fn program(n: usize) -> String {
    let mut s = String::from(
        "class A { constructor(public v: number) {} }\n\
         const a = new A(1);\n\
         export const cheap = a.v;\n\
         class Box { constructor(public x: number) {} }\n",
    );
    for i in 0..n {
        s += &format!("function f{i}(this: Box) {{ return {{ y: this.x }}; }}\n");
    }
    let fs: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let bs: Vec<String> = (0..n).map(|i| format!("new Box({i})")).collect();
    s += &format!(
        "const fs = [{}];\nconst bs = [{}];\n\
         for (const b of bs) {{ for (const f of fs) {{ const o: any = b; o.f = f; export_y(o.f().y); }} }}\n\
         function export_y(y: number) {{ return y; }}\n",
        fs.join(", "),
        bs.join(", ")
    );
    s
}

fn run(src: &str, bound: u64) -> ProgramFacts {
    let mut cfg = FactsConfig::default();
    cfg.max_work = Some(bound);
    facts_with(src, cfg)
}

fn verdicts(pf: &ProgramFacts) -> Vec<(u32, String, String)> {
    pf.sites
        .iter()
        .map(|s| (s.byte_offset, s.key.clone(), format!("{:?}", s.verdict)))
        .collect()
}

fn proven(v: &str) -> bool {
    !v.starts_with("No")
}

/// The cheap site stays proven past the bound; the product is not claimed.
#[test]
fn past_the_bound_the_settled_part_keeps_its_facts() {
    let src = program(8);
    let full = run(&src, 50_000_000);
    assert!(!full.stats.widened && !full.stats.aborted);
    let at = src.find("a.v").unwrap() as u32;
    let cheap = |pf: &ProgramFacts| {
        verdicts(pf)
            .into_iter()
            .filter(|(o, k, _)| *o >= at && k == "v")
            .min_by_key(|(o, _, _)| *o)
            .unwrap_or_else(|| panic!("site a.v in\n{}", pf.dump_text()))
            .2
    };
    assert!(proven(&cheap(&full)), "{}", cheap(&full));
    for d in [2u64, 4, 8, 16] {
        let c = run(&src, full.stats.work / d);
        eprintln!("bound {} work {} instances {} (full {}) widened {} aborted {} cheap {}",
            full.stats.work / d, c.stats.work, c.stats.instances, full.stats.instances, c.stats.widened, c.stats.aborted,
            proven(&cheap(&c)));
    }
    let bound = full.stats.work / 4;
    let cut = run(&src, bound);
    assert!(cut.stats.widened && !cut.stats.aborted, "work {}", cut.stats.work);
    assert_eq!(cheap(&cut), cheap(&full));
    // Past the bound no new calling context: fewer runs than the product.
    assert!(cut.stats.instances < full.stats.instances, "{} runs", cut.stats.instances);
}

/// At every bound a widened solve claims nothing the full solve does not:
/// each proven verdict equals the unbounded one. A late arrival into the
/// precise receiver (`a2` gets a `B` after the product) is the control.
#[test]
fn widened_verdicts_are_a_subset_of_the_full_solve() {
    for src in [
        program(4),
        program(4) + "class B { constructor(public v: number, public w: number) {} }\n\
                      let a2: any = new A(2);\nexport const late = a2.v;\n\
                      for (const b of bs) { a2 = new B(3, 4); }\n",
    ] {
        let full = run(&src, 50_000_000);
        assert!(!full.stats.widened);
        let fv = verdicts(&full);
        let mut widened = 0;
        let mut b = 1u64;
        while b < full.stats.work {
            let cut = run(&src, b);
            if cut.stats.aborted {
                assert!(cut.sites.iter().all(|s| matches!(s.verdict, Verdict::No(_))));
            } else {
                widened += cut.stats.widened as u32;
                let cv = verdicts(&cut);
                for (o, k, v) in &cv {
                    if proven(v) {
                        let f = fv.iter().find(|(fo, fk, _)| fo == o && fk == k);
                        assert_eq!(f.map(|f| &f.2), Some(v), "bound {b}: site {k}@{o} claims {v}");
                    }
                }
            }
            b = b * 5 / 4 + 1;
        }
        assert!(widened > 0);
    }
}

/// The widened solve is bounded too: past 1000 times the bound it stops
/// and claims nothing, and says why.
#[test]
fn widened_solve_stops_at_its_own_bound_and_claims_nothing() {
    let src = program(6);
    let full = run(&src, 0);
    assert!(full.stats.work > 1000, "fixture too cheap: work {}", full.stats.work);
    let stop = run(&src, 1);
    assert!(stop.stats.widened && stop.stats.widened_stop && stop.stats.aborted, "work {}", stop.stats.work);
    assert!(stop.sites.iter().all(|s| matches!(s.verdict, Verdict::No(_))));
}
