//! A built-in that makes its result with a constructor value constructs
//! it: `Array.from` and `Array.of` construct their `this` (ECMA-262
//! 23.1.2.1, 23.1.2.3: IsConstructor(C) -> Construct(C)), the Array methods
//! that make a new array construct the receiver's species
//! (ArraySpeciesCreate, 10.4.2.3). Only a program constructor is constructed
//! this way: the realm's `%Array%` is the built-in's own array. `Array.from`
//! of a program object with no `@@iterator` reads it as an array-like
//! (23.1.2.1 step 5). CXCHK0 gap witnesses test_gap_7541,
//! test_gap_array_from_roots_across_mapfn, test_gap_packed_loop_cached_receiver,
//! test_gap_rest_bundle_and_map_fill, test_gap_array_holey_copy.

use super::*;

/// Whether `hit` is an active body in `src`.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

fn runs(name: &str, src: &str) {
    assert!(hit_runs(src), "{name}: hit inactive, Node runs it");
}

fn stays(name: &str, src: &str) {
    assert!(!hit_runs(src), "{name}: hit active, Node never runs it");
}

#[test]
fn array_from_and_of_construct_a_subclass_this() {
    runs(
        "from field",
        r#"
function hit() { return 1; }
class MyArr extends Array { x = hit(); }
MyArr.from([1]);
console.log("DONE");
"#,
    );
    runs(
        "from constructor",
        r#"
function hit() { return 1; }
class MyArr extends Array { constructor(...a: any[]) { super(...a); hit(); } }
const r = MyArr.from([1, 2], (v: number) => v + 1);
console.log(r.length);
"#,
    );
    runs(
        "of",
        r#"
function hit() { return 1; }
class MyArr extends Array { x = hit(); }
console.log(MyArr.of(1, 2).length);
"#,
    );
    runs(
        "from.call",
        r#"
function hit() { return 1; }
class Bag { constructor() { hit(); } }
const r: any = Array.from.call(Bag, [1]);
console.log(r[0]);
"#,
    );
}

#[test]
fn array_from_of_the_realm_array_constructs_no_program_class() {
    // Control: a subclass exists, but only %Array% is `this`.
    stays(
        "Array.from",
        r#"
function hit() { return 1; }
class MyArr extends Array { x = hit(); }
const k = MyArr;
console.log(Array.from([1]).length, Array.of(2).length, typeof k);
"#,
    );
}

#[test]
fn an_array_method_constructs_a_program_species() {
    runs(
        "map native",
        r#"
function hit() { return 1; }
class Other extends Array { constructor(n: any) { super(); hit(); } }
class MyArr extends Array { static get [Symbol.species]() { return Other; } }
const a: any = new MyArr();
a.push(1, 2);
console.log(a.map((x: any) => x).length);
"#,
    );
    runs(
        "filter typed",
        r#"
function hit() { return 1; }
class Other extends Array<number> { constructor(n: any) { super(); hit(); } }
class MyArr extends Array<number> { static get [Symbol.species]() { return Other; } }
const a = new MyArr();
a.push(1, 2);
console.log(a.filter((x) => x > 1).length);
"#,
    );
    runs(
        "subclass map",
        r#"
function hit() { return 1; }
class MyArr extends Array { x = hit(); }
const s = MyArr.from([1, 2, 3]) as any;
console.log(JSON.stringify(Array.from(s.map((v: number) => v + 1))));
"#,
    );
}

#[test]
fn an_array_method_of_a_plain_array_constructs_no_program_class() {
    // Control: a species class exists; the receiver is a plain array.
    stays(
        "map",
        r#"
function hit() { return 1; }
class Other extends Array { constructor(n: any) { super(); hit(); } }
const k = Other;
const a: any = [1, 2];
console.log(a.map((x: any) => x).length, [3].filter((x) => x > 1).length, typeof k);
"#,
    );
}

#[test]
fn array_from_reads_an_object_without_an_iterator_as_an_array_like() {
    runs(
        "getter",
        r#"
function hit() { return "boxed"; }
class Box { length = 3; get 1() { return hit(); } }
console.log(Array.from(new Box()));
"#,
    );
    runs(
        "mapped",
        r#"
function hit() { return "boxed"; }
class Box { length = 2; get 1() { return hit(); } }
console.log(Array.from(new Box(), (v) => v));
"#,
    );
    runs(
        "length getter",
        r#"
function hit() { return 2; }
class Box { get length() { return hit(); } }
console.log(Array.from(new Box()).length);
"#,
    );
}

#[test]
fn array_from_iterates_a_builtin_iterable() {
    // Control: a Set is iterated; its own index getter never runs.
    stays(
        "set",
        r#"
function hit() { return "boxed"; }
const s: any = new Set([1, 2]);
Object.defineProperty(s, "0", { get() { return hit(); }, configurable: true });
Object.defineProperty(s, "length", { value: 2 });
console.log(Array.from(s));
"#,
    );
    // An array the runtime made iterates as an Array: its index getter runs.
    runs(
        "keys array",
        r#"
function hit() { return "k"; }
const k: any = Object.keys({ a: 1 });
Object.defineProperty(k, "0", { get() { return hit(); }, configurable: true });
for (const x of k) console.log(x);
"#,
    );
}

/// `map` and `filter` of an array run their declarations: ArraySpeciesCreate
/// reads the receiver's `constructor`, and a program constructor there is
/// constructed (`species(this, ...)`, `Cons::NewProgram`).
#[test]
fn a_declared_species_method_constructs_the_receivers_constructor() {
    runs(
        "map",
        r#"
function hit() { return 1; }
class Other extends Array { constructor(n: any) { super(); hit(); } }
const a: any = [1, 2];
a.constructor = Other;
console.log(a.map((x: any) => x).length);
"#,
    );
    runs(
        "filter",
        r#"
function hit() { return 1; }
class Other extends Array { constructor(n: any) { super(); hit(); } }
const a: any = [1, 2];
a.constructor = Other;
console.log(a.filter((x: any) => x > 1).length);
"#,
    );
    stays(
        "constructor of another array",
        r#"
function hit() { return 1; }
class Other extends Array { constructor(n: any) { super(); hit(); } }
const a: any = [1, 2];
const b: any = [3];
b.constructor = Other;
console.log(a.map((x: any) => x).length, b.length);
"#,
    );
}
