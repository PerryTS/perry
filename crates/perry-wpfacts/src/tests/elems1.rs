//! ELEMS1: the representation of a Matrix4-shaped `elements` Array
//! (`FACT FR rep`). three.js gives `Matrix4.elements = arr<boxed,0>`; each
//! E-fixture adds one statement shape of three.core.js to a minimal 2x2
//! matrix, each K-fixture is a KEEP (the rep must stay demoted).

use super::*;
use crate::FactsConfig;

const BASE: &str = r#"
class M {
  constructor(CTOR_ARGS) {
    this.elements = [1, 0, 0, 1];
    CTOR_BODY
  }
  mul(a, b) {
    const ae = a.elements, be = b.elements, te = this.elements;
    const a11 = ae[0], a12 = ae[1], a21 = ae[2], a22 = ae[3];
    const b11 = be[0], b12 = be[1], b21 = be[2], b22 = be[3];
    te[0] = a11 * b11 + a12 * b21;
    te[1] = a11 * b12 + a12 * b22;
    te[2] = a21 * b11 + a22 * b21;
    te[3] = a21 * b12 + a22 * b22;
    return this;
  }
  EXTRA
}
const x = new M();
const y = new M();
for (let i = 0; i < 10; i++) x.mul(x, y);
DRIVER
console.log(x.elements[0] + x.elements[3]);
"#;

const SET: &str = "set(a, b, c, d) { const te = this.elements; te[0] = a; te[1] = b; te[2] = c; te[3] = d; return this; }";
const FROM: &str = "fromArray(array, offset = 0) { for (let i = 0; i < 4; i++) { this.elements[i] = array[i + offset]; } return this; }";
const COPY: &str = "copy(m) { const te = this.elements, me = m.elements; te[0] = me[0]; te[1] = me[1]; te[2] = me[2]; te[3] = me[3]; return this; }";
const CLONE: &str = "clone() { return new M().fromArray(this.elements); }";
const TOARR: &str = "toArray(array = [], offset = 0) { const te = this.elements; array[offset] = te[0]; array[offset + 1] = te[1]; array[offset + 2] = te[2]; array[offset + 3] = te[3]; return array; }";

fn src(args: &str, body: &str, extra: &[&str], driver: &str) -> String {
    BASE.replace("CTOR_ARGS", args)
        .replace("CTOR_BODY", body)
        .replace("EXTRA", &extra.join("\n  "))
        .replace("DRIVER", driver)
}

/// `M.elements`'s rep and the `# rest elem` probe lines.
fn rep(s: &str) -> (String, Vec<String>) {
    let pf = facts_with(
        s,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let t = tmpl(&pf, "M");
    let f = t
        .fields
        .iter()
        .find(|f| f.key == "elements")
        .unwrap_or_else(|| panic!("no field elements in {}", pf.dump_text()));
    let probe = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# rest elem") || l.starts_with("# arr_rep"))
        .cloned()
        .collect();
    (f.rep.name(), probe)
}

fn expect(s: &str, want: &str) {
    let (got, probe) = rep(s);
    assert_eq!(got, want, "M.elements rep; probe {probe:#?}");
}

const F4: &str = "arr<f64,4,dense,fixed>";

#[test]
fn elems1_e1_minimal_mul() {
    expect(&src("", "", &[], ""), F4);
}

#[test]
fn elems1_e2_ctor_args_guarded_set() {
    let s = src(
        "n11, n12, n21, n22",
        "if (n11 !== undefined) { this.set(n11, n12, n21, n22); }",
        &[SET],
        "const z = new M(Number(process.argv[2]), 1, 2, 3); x.mul(x, z);",
    );
    expect(&s, F4);
}

#[test]
fn elems1_e2b_set_numbers() {
    let s = src("", "", &[SET], "y.set(Number(process.argv[2]), 0, 0, 1);");
    expect(&s, F4);
}

#[test]
fn elems1_e3_from_array_loop() {
    let s = src(
        "",
        "",
        &[FROM],
        "const z = new M().fromArray([1, 2, 3, 4]); x.mul(x, z);",
    );
    expect(&s, F4);
}

#[test]
fn elems1_e4_copy() {
    expect(&src("", "", &[COPY], "x.copy(y);"), F4);
}

#[test]
fn elems1_e5_clone_from_own_elements() {
    let s = src("", "", &[FROM, CLONE], "const z = x.clone(); x.mul(x, z);");
    expect(&s, F4);
}

#[test]
fn elems1_e6_to_array() {
    expect(
        &src("", "", &[TOARR], "console.log(x.toArray().length);"),
        F4,
    );
}

#[test]
fn elems1_e7_three_ctor_no_args() {
    // three.js: every `new Matrix4()` has no argument; the guarded set.
    let s = src(
        "n11, n12, n21, n22",
        "if (n11 !== undefined) { this.set(n11, n12, n21, n22); }",
        &[SET],
        "y.set(1, 2, 3, Number(process.argv[2]));",
    );
    expect(&s, F4);
}

#[test]
fn elems1_k1_string_store_is_boxed() {
    let (got, p) = rep(&src("", "", &[], "x.elements[1] = process.argv[2];"));
    assert!(got.starts_with("arr<boxed"), "{got} {p:#?}");
}

#[test]
fn elems1_k2_push_not_fixed() {
    let (got, p) = rep(&src("", "", &[], "x.elements.push(1);"));
    assert!(
        got.starts_with("arr<f64,4,dense") && !got.contains("fixed"),
        "{got} {p:#?}"
    );
}

#[test]
fn elems1_k3_store_past_end_not_fixed() {
    // a[9] = 1 on length 4: length 10, holes 4..8, none below the floor 4.
    let (got, p) = rep(&src("", "", &[], "x.elements[9] = 1;"));
    assert_eq!(got, "arr<f64,4,dense>", "{p:#?}");
}

#[test]
fn elems1_k4_unknown_index_not_fixed() {
    let (got, p) = rep(&src(
        "",
        "",
        &[],
        "x.elements[Number(process.argv[2])] = 1;",
    ));
    assert!(!got.contains("fixed"), "{got} {p:#?}");
    // exact: an index store never shrinks: floor 4 stays (rule today: ⊤ -> 0)
    assert_eq!(got, "arr<f64,4,dense>", "{p:#?}");
}

const PIVOT: &str =
    "pivot(p) { const te = this.elements; te[3] += p.x - te[0] * p.x; return this; }";

#[test]
fn elems1_e8_compound_literal_index_store() {
    // three.js Object3D.updateMatrix: `te[ 12 ] += px - te[ 0 ] * px ...`.
    let s = src("", "", &[PIVOT], "x.pivot({ x: Number(process.argv[2]) });");
    expect(&s, F4);
}

#[test]
fn elems1_e9_three_shape_reproduces() {
    // E7 + E8 together: three.js gives arr<boxed,0>.
    let s = src(
        "n11, n12, n21, n22",
        "if (n11 !== undefined) { this.set(n11, n12, n21, n22); }",
        &[SET, PIVOT],
        "y.set(1, 2, 3, Number(process.argv[2])); x.pivot({ x: Number(process.argv[2]) });",
    );
    expect(&s, F4);
}

#[test]
fn elems1_e7b_guard_on_every_param() {
    // The guarded set when each forwarded parameter is the guarded one.
    let s = src(
        "n11",
        "if (n11 !== undefined) { this.set(n11, n11, n11, n11); }",
        &[SET],
        "",
    );
    expect(&s, F4);
}

#[test]
fn elems1_e10_console_shown_owner() {
    // three.js Object3D.add: `console.error( ..., object )` shows an
    // Object3D, whose `matrix.elements` is then shown (InspectShown).
    let s = src(
        "",
        "",
        &[],
        "class N { constructor() { this.matrix = new M(); } add(o) { if (o === this) { console.error(self, o); } return this; } }\nconst n = new N(); n.add(new N()); n.matrix.mul(n.matrix, y);",
    );
    expect(&s, F4);
}

/// The rep's element kind: `arr<KIND,...`.
fn elem_kind(s: &str) -> String {
    let (got, _) = rep(s);
    got.trim_start_matches("arr<")
        .split(',')
        .next()
        .unwrap_or_default()
        .to_string()
}

const PUT: &str = "function put(m, v) { if (v !== undefined) { m.elements[0] = v; } }";

#[test]
fn elems1_g1_guarded_branch_runs_with_args() {
    // KEEP (R2): one call passes arguments, so the guarded branch runs and
    // its stores (a string) apply.
    let s = src(
        "n11, n12, n21, n22",
        "if (n11 !== undefined) { this.set(n11, n12, n21, n22); }",
        &[SET],
        "const z = new M(process.argv[2], \"a\", \"b\", \"c\"); x.mul(x, z);",
    );
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_g2_guard_narrows_number_or_undefined() {
    // R2 + narrowing: the branch runs with the narrowed value (a Number).
    let s = src(
        "",
        "",
        &[],
        &format!("{PUT}\nput(y, Math.random() < 0.5 ? 1 : undefined);"),
    );
    expect(&s, F4);
}

#[test]
fn elems1_g2b_guard_narrows_string_or_undefined() {
    // KEEP: the narrowed value is a String: the store applies.
    let s = src(
        "",
        "",
        &[],
        &format!("{PUT}\nput(y, Math.random() < 0.5 ? \"s\" : undefined);"),
    );
    assert_eq!(elem_kind(&s), "boxed");
}

const INSP: &str = "const o = { [Symbol.for(\"nodejs.util.inspect.custom\")]() { y.elements[0] = \"s\"; return \"o\"; } };";

#[test]
fn elems1_g3_own_inspect_custom_runs() {
    // KEEP (R1): an own `[inspect.custom]` method of a shown object runs.
    let s = src("", "", &[], &format!("{INSP}\nconsole.log(o);"));
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_g3c_control_not_shown() {
    let s = src("", "", &[], &format!("{INSP}\nconsole.log(typeof o);"));
    assert_eq!(elem_kind(&s), "f64");
}

#[test]
fn elems1_g4_own_to_json_on_array_runs() {
    // KEEP (R1): a program-written `toJSON` on the array runs under JSON.
    let s = src("", "", &[], "y.elements.toJSON = function () { this[0] = \"s\"; return 1; };\nconsole.log(JSON.stringify(y.elements));");
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_g4b_own_to_string_on_array_runs() {
    // KEEP (R1): a program-written `toString` on the array runs under String().
    let s = src("", "", &[], "y.elements.toString = function () { this[0] = \"s\"; return \"m\"; };\nconsole.log(String(y.elements));");
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_e5d_from_array_of_a_field() {
    // e5's UNDEF without `clone` or the default `offset`: the loop read
    // `array[i]` (i in [0, 3]) of an `M.elements` array (RED, open).
    let s = src(
        "",
        "",
        &[FROM],
        "const z = new M().fromArray(x.elements); x.mul(x, z);",
    );
    expect(&s, F4);
}

// ELEM2: a computed-key store and read inside the bounds of a dense Array.
// The store `a[i] = v` under a Number key marked a hole at once, so a read
// of the same Array under a computed key read `undefined` from the hole
// (ECMA-262 10.4.2.1: a store below the length writes that element; only an
// index past the end leaves holes). I-fixtures are in bounds; KEEPs reach
// the end, take an unknown offset, shrink the Array or put an index key on
// Array.prototype.

#[test]
fn elems1_i1_loop_read_store_in_bounds() {
    // `a[i]` read and stored for i in [0, 3] on a length-4 Array, then
    // carried into M.elements: no hole, no `undefined`.
    let d = "const a = [1, 2, 3, 4]; for (let i = 0; i < 4; i++) { a[i] = a[i] + 1; x.elements[i] = a[i]; }";
    expect(&src("", "", &[], d), F4);
}

#[test]
fn elems1_i2_from_array_of_another_instance() {
    // three.js Matrix4.fromArray(m.elements) with the default offset, both
    // ways between two instances: each Array is read and stored under `i`.
    let s = src(
        "",
        "",
        &[FROM],
        "const w = new M(); w.fromArray(x.elements); x.fromArray(w.elements); x.mul(x, w);",
    );
    expect(&s, F4);
}

#[test]
fn elems1_i3_keep_unknown_offset() {
    // KEEP: `i + offset` with an unknown offset may be past the end.
    let s = src(
        "",
        "",
        &[FROM],
        "const z = new M().fromArray(y.elements, Number(process.argv[2])); x.mul(x, z);",
    );
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_i4_keep_one_past_the_end() {
    // KEEP: `i <= 4` reads index 4 of a length-4 Array: `undefined`.
    let s = src(
        "",
        "",
        &[],
        "for (let i = 0; i <= 4; i++) { x.elements[0] = y.elements[i]; }",
    );
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_i5_keep_shrunk_array() {
    // KEEP: a `length` store may shrink the Array: the length's lower bound
    // is no longer 4, so `i < 4` is no proof the read is in bounds.
    let d = "y.elements.length = Number(process.argv[2]); for (let i = 0; i < 4; i++) { x.elements[i] = y.elements[i]; }";
    assert_eq!(elem_kind(&src("", "", &[], d)), "boxed");
}

#[test]
fn elems1_i5b_grown_array_floor_holds() {
    // A `push` only grows the Array: the floor 4 is a true lower bound of
    // its length at every read, so the read in [0, 3] is in bounds (the
    // length is not fixed).
    let d = "y.elements.push(5); for (let i = 0; i < 4; i++) { x.elements[i] = y.elements[i]; }";
    assert_eq!(elem_kind(&src("", "", &[], d)), "f64");
}

#[test]
fn elems1_i6_keep_prototype_index_past_the_end() {
    // KEEP: past the end the read goes to Array.prototype, whose index key
    // the program wrote (10.1.8 OrdinaryGet): the String reaches M.elements.
    let d = "Array.prototype[5] = process.argv[2]; for (let i = 0; i < 6; i++) { x.elements[0] = y.elements[i]; }";
    assert_eq!(elem_kind(&src("", "", &[], d)), "boxed");
}

// ELEM3: three.core.js Matrix4.makeOrthographic / makePerspective: `let c;`
// without initializer (ECMA-262 14.3.1: `undefined` when the declaration is
// evaluated), assigned in every arm of an `if / else if / else` whose last
// arm throws (14.14: it never reaches the statement after the `if`), then
// stored. Only the arms' values reach the store. U-fixtures from the real
// shape; KEEPs where `undefined` (or the declaration's value) can reach it.

const ORTHO_DRIVER: &str = "x.ortho(Number(process.argv[2]));";

fn ortho(body: &str) -> String {
    let m = format!("ortho(k) {{ const te = this.elements; let c; {body} return this; }}");
    src("", "", &[&m], ORTHO_DRIVER)
}

#[test]
fn elems1_u1_let_assigned_in_every_arm_but_a_throwing_else() {
    let s = ortho("if (k === 1) { c = 1; } else if (k === 2) { c = 2; } else { throw new Error(\"x\"); } te[0] = c;");
    expect(&s, F4);
}

#[test]
fn elems1_u2_let_assigned_in_every_arm() {
    let s =
        ortho("if (k === 1) { c = 1; } else if (k === 2) { c = 2; } else { c = 3; } te[0] = c;");
    expect(&s, F4);
}

#[test]
fn elems1_u3_keep_no_final_else() {
    // KEEP: no final `else`: when k is neither, the store reads `undefined`.
    let s = ortho("if (k === 1) { c = 1; } else if (k === 2) { c = 2; } te[0] = c;");
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_u4_keep_closure_reads_before_the_assignment() {
    // KEEP: the closure runs before the `if` assigns `c`: it returns
    // `undefined`.
    let s =
        ortho("const f = () => c; te[1] = f(); if (k === 1) { c = 1; } else { c = 2; } te[0] = c;");
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_u5_keep_loop_read_before_the_first_assignment() {
    // KEEP: the first iteration reads `c` before the `if` assigns it.
    let s =
        ortho("for (let i = 0; i < k; i++) { te[0] = c; if (k === 1) { c = 1; } else { c = 2; } }");
    assert_eq!(elem_kind(&s), "boxed");
}

#[test]
fn elems1_u6_switch_with_throwing_default() {
    let s = ortho("switch (k) { case 1: c = 1; break; case 2: c = 2; break; default: throw new Error(\"x\"); } te[0] = c;");
    expect(&s, F4);
}

#[test]
fn elems1_u7_keep_switch_break_before_the_assignment() {
    // KEEP: `break` before `c = 1` leaves the switch with `c` undefined.
    let s = ortho("switch (k) { case 1: if (k > Number(process.argv[3])) break; c = 1; break; default: c = 2; } te[0] = c;");
    assert_eq!(elem_kind(&s), "boxed");
}

// ELEM4 H-fixtures: the rep of the three `mul` locals `ae`, `be`, `te`
// (`# arr_rep` lines; the codegen's rep tier keys on it). Only the `mul`
// locals and their aliases hold `elements` Arrays.

/// The `# arr_rep` reps of the fixture's locals.
fn local_reps(s: &str) -> Vec<String> {
    let pf = facts_with(
        s,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.dump_text()
        .lines()
        .filter(|l| l.starts_with("# arr_rep"))
        .filter_map(|l| l.split("rep=").nth(1).map(str::to_string))
        // An `elements` Array is stored into by `mul`: never immutable
        // (`,im`), so an `im` rep is another local of the fixture.
        .filter(|r| !r.ends_with(",im>"))
        .collect()
}

#[test]
fn elems1_h1_mul_locals_have_the_field_rep_under_aliasing() {
    // x.mul(x, y) (BASE) and y.mul(x, x), x.mul(y, x): `a`, `b`, `this`
    // the same object.
    let r = local_reps(&src("", "", &[], "y.mul(x, x); x.mul(y, x);"));
    assert!(r.iter().filter(|r| r.as_str() == F4).count() >= 3, "{r:#?}");
}

#[test]
fn elems1_h2_push_keeps_locals_not_fixed() {
    let r = local_reps(&src(
        "",
        "",
        &[],
        "if (process.argv[2] === \"1\") x.elements.push(1);",
    ));
    assert!(r.iter().all(|r| !r.contains("fixed")), "{r:#?}");
}

#[test]
fn elems1_h3_non_array_field_value_gives_no_local_rep() {
    let r = local_reps(&src(
        "",
        "",
        &[],
        "if (process.argv[2] === \"1\") x.elements = \"str\";",
    ));
    assert!(r.iter().all(|r| !r.starts_with("arr<")), "{r:#?}");
}

#[test]
fn elems1_h4_undefined_store_keeps_locals_boxed() {
    let r = local_reps(&src(
        "",
        "",
        &[],
        "x.elements[Number(process.argv[2]) & 3] = undefined;",
    ));
    assert!(r.iter().all(|r| !r.starts_with("arr<f64")), "{r:#?}");
}

#[test]
fn elems1_h5_escape_to_a_reading_callee_keeps_the_rep() {
    let look = "look(t) { if (process.argv[2] === \"9\") console.log(t[0]); }";
    let s = src("", "", &[look], "x.look(x.elements);");
    let r = local_reps(&s);
    assert!(r.iter().filter(|r| r.as_str() == F4).count() >= 3, "{r:#?}");
}

#[test]
fn elems1_h5s_escape_to_a_string_storing_callee_is_boxed() {
    let look = "look(t) { if (process.argv[2] === \"1\") t[1] = \"s\"; }";
    let s = src("", "", &[look], "x.look(x.elements);");
    let r = local_reps(&s);
    assert!(r.iter().all(|r| !r.starts_with("arr<f64")), "{r:#?}");
}
