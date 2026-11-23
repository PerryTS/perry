//! Named static methods of a class value on the web target (`class_set_static_method`).

use super::run_web_source;

// A named static method is a property of the class value, so a read that does not
// resolve to the class statically finds it: an alias, a subclass, and a class an arrow
// reads before its declaration (#14727). Expected output is Node's.
#[test]
fn a_class_value_carries_its_static_methods() {
    assert_eq!(
        run_web_source(
            r#"
function viaTag() { const read = () => C4.tag`x`; class C4 { static tag(s: TemplateStringsArray) { return "tagged"; } } return read(); }
function plainCall() { const read = () => C7.tag("x"); class C7 { static tag(s: any) { return "called " + s; } } return read(); }
class D { static twice(n: number) { return n * 2; } static v = 3; }
class E extends D {}
const K: any = D;
const S: any = E;
console.log("forward tag", viaTag());
console.log("forward call", plainCall());
console.log("alias", typeof K.twice, K.twice(4), K.v);
console.log("inherited", S.twice(5));
"#
        ),
        "forward tag tagged\nforward call called x\nalias function 8 3\ninherited 10"
    );
}

// An inherited static is found along the class chain, not copied onto the subclass
// value, so a nearer static field shadows an ancestor's method (#14727).
#[test]
fn an_inherited_static_method_is_not_an_own_property() {
    assert_eq!(
        run_web_source(
            r#"
class A { static m() { return "A.m"; } }
class B extends A { static m: any = 7; }
class C extends B {}
class D { static twice(n: number) { return n * 2; } }
class E extends D {}
const KC: any = C;
const KD: any = D;
const KE: any = E;
console.log("shadow", typeof KC.m, KC.m === 7);
console.log("own", Object.prototype.hasOwnProperty.call(KD, "twice"), Object.prototype.hasOwnProperty.call(KE, "twice"));
console.log("same", KE.twice === KD.twice);
(async () => console.log("async", KE.twice(6), typeof KC.m))();
"#
        ),
        "shadow number true\nown true false\nsame true\nasync 12 number"
    );
}

// A static method read through a class value runs with that value as `this`.
#[test]
fn a_static_method_called_through_a_subclass_has_it_as_this() {
    assert_eq!(
        run_web_source(
            r#"
class F { static who() { return this.name; } static make() { return new this(); } }
class G extends F {}
const KG: any = G;
console.log("this", G.who(), KG.who(), KG.make() instanceof G);
"#
        ),
        "this G G true"
    );
}

// Each evaluation of a class expression owns its static methods' captures.
#[test]
fn a_static_method_reads_its_own_evaluations_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(n: number) { return class { static read() { return n; } }; }
const P: any = make(1);
const Q: any = make(2);
console.log("eval", P.read(), Q.read());
"#
        ),
        "eval 1 2"
    );
}

// A static method value is a callback array methods can call.
#[test]
fn a_static_method_value_is_an_array_callback() {
    assert_eq!(
        run_web_source(
            r#"
class D { static twice(n: number) { return n * 2; } }
const K: any = D;
console.log("map", [1, 2].map(K.twice).join(","));
"#
        ),
        "map 2,4"
    );
}

// A deleted static method stays deleted, on its class and on a subclass.
#[test]
fn a_deleted_static_method_stays_deleted() {
    assert_eq!(
        run_web_source(
            r#"
class D { static twice(n: number) { return n * 2; } }
class E extends D {}
const KD: any = D;
const KE: any = E;
delete KD.twice;
console.log("deleted", typeof KD.twice, typeof KE.twice);
"#
        ),
        "deleted undefined undefined"
    );
}

// A static method named `__proto__` is an own property, not a prototype assignment.
#[test]
fn a_static_method_named_proto_is_an_own_property() {
    assert_eq!(
        run_web_source(
            r#"
class Z { static __proto__() { return 7; } }
const KZ: any = Z;
console.log("proto", Object.prototype.hasOwnProperty.call(KZ, "__proto__"));
"#
        ),
        "proto true"
    );
}

// `Reflect.apply` hands a static method the receiver it names.
#[test]
fn a_static_method_applied_to_an_object_has_it_as_this() {
    assert_eq!(
        run_web_source(
            r#"
class F { static who() { return this.name; } }
const KF: any = F;
console.log("reflect", Reflect.apply(KF.who, { name: "other" }, []));
"#
        ),
        "reflect other"
    );
}

// Only the method a class value is called on sees that value as `this`: a direct
// call it makes by class name sees the named class.
#[test]
fn a_direct_static_call_inside_an_inherited_call_sees_its_named_class() {
    assert_eq!(
        run_web_source(
            r#"
class F { static who() { return this.name; } static explicit() { return F.who(); } }
class G extends F {}
const KG: any = G;
console.log("nested", KG.who(), KG.explicit());
"#
        ),
        "nested G F"
    );
}

// An array callback that is a class expression's static method reads its own
// evaluation's captures.
#[test]
fn a_static_method_callback_reads_its_own_evaluations_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(n: number) { return class { static read(x: number) { return n + x; } }; }
const P: any = make(1);
const Q: any = make(2);
console.log("evalmap", [10].map(P.read).join(","), [10].map(Q.read).join(","), Q.read(10));
"#
        ),
        "evalmap 11 12 12"
    );
}

// A static block is not a property of its class value.
#[test]
fn a_static_block_is_not_a_static_method() {
    assert_eq!(
        run_web_source(
            r#"
class K { static v = 1; static { K.v = 2; } static m() { return 3; } }
const KK: any = K;
console.log("block", typeof KK.__perry_static_init_0, Object.prototype.hasOwnProperty.call(KK, "__perry_static_init_0"), KK.v);
"#
        ),
        "block undefined false 2"
    );
}
