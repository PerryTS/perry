//! The order of a unit cycle (`units::driver::dependency_order`): a cycle
//! whose only way back is a parameter type is broken there, never at a
//! subclass's `extends` edge (which made the subclass's unit see its parent
//! class as ⊤ and break it: tsc's Project -> ProjectService ->
//! ExternalProject -> Project).

use super::*;

// `b` is named first by `m`, so the order's DFS enters the cycle at `b`:
// b -> p (extends) -> s (the parameter type) -> b (`new Ext`).
const P: &str = r#"import { Service } from "./s";
export class Base {
    n: number;
    constructor(readonly svc: Service) { this.n = 1; }
    size() { return this.n + 1; }
}"#;

const S: &str = r#"import { Ext } from "./b";
export class Service {
    count = 0;
    create() { const e = new Ext(null); return e.size() + e.k; }
}"#;

const B: &str = r#"import { Base } from "./p";
export class Ext extends Base {
    k: number;
    constructor(svc: any) { super(svc); this.k = 2; }
}"#;

const M: &str = r#"import { Ext } from "./b";
import { Service } from "./s";
const s = new Service();
console.log(s.create(), new Ext(s).size());"#;

#[test]
fn a_cycle_through_a_parameter_type_is_broken_at_the_type_not_at_extends() {
    let mods = [("p", P), ("s", S), ("b", B), ("m", M)];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    let (pu, pw) = (proven_set(&u), proven_set(&w));
    let lost: Vec<_> = pw.difference(&pu).collect();
    let broken: Vec<_> = u
        .broken
        .iter()
        .filter(|d| !d.starts_with("override "))
        .collect();
    assert!(!pw.is_empty(), "{}", w.dump_text());
    assert!(lost.is_empty() && broken.is_empty(), "lost {lost:?} broken {broken:?}\n{}", u.dump_text());
}
