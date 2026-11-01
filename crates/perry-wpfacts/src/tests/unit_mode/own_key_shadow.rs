//! DECISIONS 234 per unit: on a published class's receiver marker (another
//! unit calls the method), a definite own key of the class shadows the
//! builtin prototypes a store into ⊤ may have written, for instances of
//! the class and of every subclass. (A subclass of another unit reports
//! the keys its instances hold, inherited ones included, as `override k
//! of`: the marker's own field is then another unit's, and does not
//! shadow: DECISIONS 231(d) is not built.) A subclass of another unit that may
//! delete or redefine a key (census `override * of`), another unit's
//! delete on an instance (census report) and an accessor through ⊤ in any
//! unit revoke it.

use super::*;

/// `h.svc = 1` may write `Object.prototype.svc` (node: `hostFn` returns
/// `Object.prototype`); a computed key would write `total` there too.
const P: &str = r#"const h: any = (globalThis as any).hostFn();
h.svc = 1;
export class Svc { size = 3; }
export class P {
    svc: Svc;
    constructor(s: Svc) { this.svc = s; }
    total() { return this.svc.size; }
}"#;

/// Is p.ts's `this.svc.size` proven, per unit and in the whole program?
fn size_proven(mods: &[(&str, &str)]) -> (bool, bool) {
    let at = |pf: &ProgramFacts| {
        proven_set(pf)
            .iter()
            .any(|s| s.starts_with("p.ts@") && s.ends_with(".size"))
    };
    (at(&units_of(mods)), at(&whole_of(mods)))
}

#[test]
fn a_definite_key_shadows_on_the_receiver_marker() {
    let m = r#"import { P, Svc } from "./p";
console.log(new P(new Svc()).total());"#;
    assert_eq!(size_proven(&[("p", P), ("m", m)]), (true, true));
}

#[test]
fn a_subclass_that_deletes_the_key_revokes_it() {
    let b = r#"import { P } from "./p";
export class Q extends P { constructor(s: any) { super(s); delete (this as any).svc; } }"#;
    let m = r#"import { Q } from "./b";
import { Svc } from "./p";
console.log(new Q(new Svc()).total());"#;
    assert_eq!(size_proven(&[("p", P), ("b", b), ("m", m)]), (false, false));
}

#[test]
fn another_units_delete_revokes_the_key() {
    let m = r#"import { P, Svc } from "./p";
const p: any = new P(new Svc());
delete p.svc;
console.log(p.total());"#;
    assert_eq!(size_proven(&[("p", P), ("m", m)]), (false, false));
}

/// `kill` deletes a key of its parameter: per unit, another unit's
/// instance of P that reached it is ⊤ in p.ts, so the delete revokes the key
/// on P's receiver marker there (`SharedWrite::Hard(⊤, E7)`).
#[test]
fn another_units_delete_through_a_parameter_revokes_the_key() {
    let k = r#"export function kill(o: any, key: string) { delete o[key]; }"#;
    let m = r#"import { P, Svc } from "./p";
import { kill } from "./k";
declare const name: string;
const p = new P(new Svc());
kill(p, name);
console.log(p.total());"#;
    assert_eq!(size_proven(&[("p", P), ("k", k), ("m", m)]), (false, false));
}

#[test]
fn another_units_accessor_through_top_revokes_the_key() {
    let m = r#"import { P, Svc } from "./p";
const g: any = (globalThis as any).hostFn();
Object.defineProperty(g, "svc", { get() { return 1; }, set(v: any) {} });
console.log(new P(new Svc()).total());"#;
    assert_eq!(size_proven(&[("p", P), ("m", m)]), (false, false));
}
