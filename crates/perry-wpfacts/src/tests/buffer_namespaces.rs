//! `%TypedArray%`, each concrete typed-array constructor, `DataView`,
//! `ArrayBuffer` and `SharedArrayBuffer` are distinct constructors with
//! distinct prototypes (ECMA-262 23.2, 25.1-25.3): a concrete typed array's
//! [[Prototype]] is `%TypedArray%`, and an instance reads the members its
//! own prototype chain holds, as the runtime realm declares them.
use super::*;

/// The receivers of the `size` read on `x`, a `Svc` or the value `v`.
fn receivers_of(prelude: &str, v: &str) -> Vec<String> {
    let src = format!(
        "class Svc {{ size = 3; }}\n\
         {prelude}\n\
         const v: any = {v};\n\
         const x: any = process.argv.length > 9 ? new Svc() : v;\n\
         console.log(x.size);\n"
    );
    site(&facts(&src), "size").receivers.clone()
}

/// The `size` read on `x` (a `Svc` or `v`): its receivers, and whether one
/// is TOP (an opaque builtin member a program store may have replaced).
fn size_site(prelude: &str, v: &str) -> (Vec<String>, bool) {
    let src = format!(
        "class Svc {{ size = 3; }}\n\
         {prelude}\n\
         const v: any = {v};\n\
         const x: any = process.argv.length > 9 ? new Svc() : v;\n\
         console.log(x.size);\n"
    );
    let pf = facts(&src);
    let s = site(&pf, "size");
    (s.receivers.clone(), s.top)
}

const DV: &str = "(new DataView(new ArrayBuffer(8)) as any)";
const AB: &str = "(new ArrayBuffer(8) as any)";
const SAB: &str = "(new SharedArrayBuffer(8) as any)";

/// Red on main: one namespace stood for `%TypedArray%`, every concrete
/// typed array and `DataView`, whose [[Prototype]] was `Function.prototype`
/// (so a member stored on `%TypedArray%.prototype` through
/// `Object.getPrototypeOf(Uint8Array)` was not read by an `Int8Array`: the
/// fxq/q5 false proof), and one for `ArrayBuffer` and `SharedArrayBuffer`.
#[test]
fn each_typed_array_constructor_is_its_own_with_typed_array_as_its_prototype() {
    let made = "class Made { size = 1; }";
    let u8 = "builtin:TypedArrayKind(\"Uint8Array\")";
    let cases = [
        ("", "Uint8Array".to_string(), u8),
        ("", "Object.getPrototypeOf(Uint8Array)".to_string(), "builtin:TypedArray"),
        ("", "Object.getPrototypeOf(Float16Array)".to_string(), "builtin:TypedArray"),
        ("", "(new Uint8Array(2) as any).constructor".to_string(), u8),
        ("", format!("{DV}.constructor"), "builtin:DataView"),
        ("", format!("{AB}.constructor"), "builtin:ArrayBuffer"),
        ("", format!("{SAB}.constructor"), "builtin:SharedArrayBuffer"),
    ];
    let mut wrong = Vec::new();
    for (prelude, v, want) in cases {
        let r = receivers_of(&format!("{made} {prelude}"), &v);
        if !r.iter().any(|t| t == want) {
            wrong.push(format!("{v}: want {want}, got {r:?}"));
        }
    }
    // A member the program stores on `%TypedArray%.prototype`, reached
    // through `Object.getPrototypeOf(Uint8Array)`, is read by an
    // `Int8Array` (fxq/q5): the `size` read sees `Made`, or the opaque
    // member (TOP) that stands for it. Without the store the member is
    // absent (no TOP): the control.
    let prelude =
        "const TA: any = Object.getPrototypeOf(Uint8Array); TA.prototype.make = () => new Made();";
    let v = "(new Int8Array(2) as any).make()";
    let (r, top) = size_site(&format!("{made} {prelude}"), v);
    if !top && !r.iter().any(|t| t == "t.ts:Made") {
        wrong.push(format!("{v} after the store: want t.ts:Made or TOP, got {r:?}"));
    }
    let (r, top) = size_site(made, v);
    if top {
        wrong.push(format!("{v} without a store: want no TOP, got {r:?}"));
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    // `Uint8Array` is not `%TypedArray%`, `SharedArrayBuffer` not `ArrayBuffer`.
    assert!(!receivers_of("", "Uint8Array").iter().any(|t| t == "builtin:TypedArray"));
    let sab = receivers_of("", &format!("{SAB}.constructor"));
    assert!(!sab.iter().any(|t| t == "builtin:ArrayBuffer"), "{sab:?}");
}

/// Red on main: every buffer and DataView instance read
/// `%TypedArray%.prototype` (`ab.slice` was the typed-array `slice`, a
/// DataView had `subarray`), and the realm declared no `getFloat16` /
/// `setFloat16` on `DataView.prototype` and no `grow` on
/// `SharedArrayBuffer.prototype`.
#[test]
fn buffer_and_data_view_instances_read_their_own_prototype_members() {
    let cases = [
        (format!("{DV}.getInt8"), "DataView.prototype.getInt8"),
        (format!("{DV}.getFloat16"), "DataView.prototype.getFloat16"),
        (format!("{DV}.setFloat16"), "DataView.prototype.setFloat16"),
        (format!("{AB}.slice"), "ArrayBuffer.prototype.slice"),
        (format!("{AB}.resize"), "ArrayBuffer.prototype.resize"),
        (format!("{SAB}.slice"), "SharedArrayBuffer.prototype.slice"),
        (format!("{SAB}.grow"), "SharedArrayBuffer.prototype.grow"),
    ];
    let mut wrong = Vec::new();
    for (v, want) in &cases {
        let r = receivers_of("", v);
        if !r.iter().any(|t| t.contains(want)) {
            wrong.push(format!("{v}: want {want}, got {r:?}"));
        }
    }
    // Absent in node and in the runtime: only the `Svc` receiver remains.
    for v in [
        format!("{DV}.subarray"),
        format!("{DV}[Symbol.iterator]"),
        format!("{SAB}.resize"),
        format!("{AB}.grow"),
    ] {
        let r = receivers_of("", &v);
        if r.iter().any(|t| t != "t.ts:Svc") {
            wrong.push(format!("{v}: want only t.ts:Svc, got {r:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
