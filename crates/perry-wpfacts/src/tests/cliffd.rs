//! CLIFFD: one builtin handed to code the solver does not see must not take
//! out the realm. (a) `performance` as the receiver of JSON's implicit
//! `toJSON` call runs the declaration of `Performance.prototype.toJSON`
//! (perf_hooks/prototypes.rs, performance_methods.rs `to_json`: a fresh
//! record, no program code, no store on a handed value), so its members stay
//! the realm's. (b) A builtin object handed to unknown code keeps its
//! intrinsic members (owner premise 2026-11-19);
//! a program store onto it is still seen. (c) Every [[Prototype]] chain the
//! realm declares ends (ECMA-262 10.1.2.1 step 8: no cycles), so the
//! solver's chain walks over it end.
use super::*;

const HELD: &str = "declare function sink(x: any): void;\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

const OBJ: &str = "const o = { m() { return 1; } };\n";

/// The verdicts of the call sites of `m`, in source order.
fn m_calls(src: &str) -> Vec<String> {
    let pf = one(src);
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "m" && s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("CLIFFD {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of m");
    v
}

/// `o.m()` after `pre` and `(performance as any).now(o)` is the program body:
/// `now` is the realm's (it reads no argument), so `o` reached no other code.
fn now_keeps_o(pre: &str) -> bool {
    let v = m_calls(&format!(
        "{OBJ}{pre}\n(performance as any).now(o);\nconst r = o.m();\nconsole.log(r);\n"
    ));
    v.iter().any(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// (a) `JSON.stringify(performance)` calls `performance.toJSON()` (25.5.2.2)
/// through its function value: the declaration runs, `performance` does not
/// escape, its `now` stays the realm's.
#[test]
fn performance_to_json_keeps_the_realm() {
    assert!(now_keeps_o(""), "twin without toJSON lost the claim");
    assert!(
        now_keeps_o("console.log(JSON.stringify(performance));"),
        "toJSON protocol call took out performance.now"
    );
}

/// (a) The explicit call of the builtin through its function value runs
/// the same declaration.
#[test]
fn performance_to_json_value_call_keeps_the_realm() {
    assert!(now_keeps_o(
        "const f = (performance as any).toJSON;\nconsole.log(f.call(performance));"
    ));
}

/// (b) Handed to unknown code, `performance` keeps its intrinsic `now`
/// (premise 2026-11-19).
#[test]
fn handed_builtin_keeps_intrinsic_members() {
    let held = now_keeps_o(&format!("{HELD}sink(performance);"));
    assert!(held, "a handed builtin lost its intrinsic now");
}

/// (b, keep) A program store onto the handed builtin is still seen: the
/// program's `now` replaces `o.m`, in both arms.
#[test]
fn program_store_onto_handed_builtin_is_kept() {
    assert!(!now_keeps_o(&format!(
        "{HELD}(performance as any).now = (x: any) => {{ x.m = () => 2; }};\nsink(performance);"
    )));
}

/// (c) Every declared [[Prototype]] chain ends: none names itself or loops
/// (2026-11-23: `node:cluster.settings` / `.workers` extended themselves and
/// HOSTVAL's chain walks spun in `cjs_require`).
#[test]
fn realm_prototype_chains_end() {
    let realm = crate::native_effects::realm();
    let by: std::collections::HashMap<&str, Option<&str>> =
        realm.iter().map(|e| (e.name, e.proto)).collect();
    let mut cycles = Vec::new();
    for e in realm {
        let mut seen = vec![e.name];
        let mut at = e.proto;
        while let Some(p) = at {
            if seen.contains(&p) {
                cycles.push(format!("{} -> {p}", seen.join(" -> ")));
                break;
            }
            seen.push(p);
            at = by.get(p).copied().flatten();
        }
    }
    assert!(cycles.is_empty(), "cyclic realm [[Prototype]] chains: {cycles:?}");
}
