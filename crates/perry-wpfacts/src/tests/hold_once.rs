//! The realm hold is one fact of the whole program, not a moment of the
//! solve: once any unknown code exists it holds the realm (ECMA-262 9.3)
//! for every statement, the ones the solve visited first included. A solve
//! that flips `realm_held` midway keeps what its rules made before the
//! flip, so its result depends on the visiting order. The whole-program
//! solve that meets its first hold starts again with the realm held from
//! its seed (`Model::restart_on_hold`): the least fixpoint of the held rules.
//! Test: the same program with the unknown call first or last gives the
//! same model and the same verdicts.
use super::*;

fn program(first: &str, last: &str) -> String {
    format!(
        r#"
class Box {{ v: any; constructor(v: any) {{ this.v = v; }} get(): any {{ return this.v; }} }}
var k: any = process.argv[2];
function go(xs: any, mm: any) {{ const g: any = (globalThis as any)[k]; g(xs, mm); }}
{first}
function wrap(x: any): any {{ return new Box(x); }}
function deep(n: number, x: any): any {{ return n > 0 ? deep(n - 1, wrap(x)) : x; }}
var items: any[] = [];
for (let i = 0; i < 4; i++) items.push(deep(3, {{ i }}));
var m = new Map<any, any>();
items.forEach((it) => m.set(it, it.get()));
m.forEach((v, kk) => {{ const e = new Error(String(v)); items.push(e); }});
const p = Promise.resolve(items).then((xs) => xs);
const w = new WeakMap<any, any>();
for (const it of items) w.set(it, wrap(it));
console.log(w.get(items[0]));
{last}
"#
    )
}

fn shape(pf: &ProgramFacts) -> (usize, Vec<String>) {
    let mut v: Vec<String> = pf
        .sites
        .iter()
        .map(|s| format!("{} {:?}", s.key, s.verdict))
        .collect();
    v.sort();
    (pf.stats.nodes, v)
}

#[test]
fn hold_first_or_last_same_model() {
    let early = facts(&program("go(items, m);", ""));
    let late = facts(&program("", "go(items, m);"));
    assert_eq!(shape(&early), shape(&late));
}

/// Control: the hold still happens (the unknown callee stays refused).
#[test]
fn late_hold_still_holds() {
    let pf = facts(&program("", "go(items, m);"));
    assert!(matches!(site(&pf, "()").verdict, Verdict::No(_)));
}
