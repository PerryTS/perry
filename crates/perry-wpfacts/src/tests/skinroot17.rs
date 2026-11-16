//! SKINROOT17: `valueOf` called on a typed array. `%TypedArray%.prototype`
//! and a kind's own prototype do not own `valueOf` (ECMA-262 23.2.3, 23.2.7:
//! the realm's declarations list their members), so [[Get]] continues to
//! `Object.prototype.valueOf` (20.1.3.7: return ToObject(this value)): it
//! returns the receiver and runs no program code. Today the typed-array
//! family is opaque ("every name is a member"), the call is an unknown
//! method, unknown code holds the realm and every intrinsic escapes; on
//! three.js r8 that is the first realm hold (work 1.83 M), which hands the
//! GLTF JSON tokens to unknown code (`escaped-native-fields`, 1.93 M).
//! RED (r1, the maker of r8 call 865): ToPrimitive of an object whose chain
//! holds a typed-array prototype holds the realm. KEEP: k1 a `valueOf` the
//! program writes onto `%TypedArray%.prototype` is called.
use super::*;

const HEAD: &str = "class M { go() { return 1; } }\nclass R { y = 1; }\nconst j: any = JSON.parse(process.argv[2]);\nconst r: any = new R();\nconst t: any = new Float32Array(3);\n";

fn run(src: &str) -> ProgramFacts {
    facts(&format!("{HEAD}{src}"))
}

/// The realm is held when `console.log(o)` may call unknown code with `o`,
/// so `o.m()` is not the program body (`held_realm_flow`).
fn m_mono(src: &str) -> bool {
    let pf = run(&format!("const o = {{ m() {{ return 1; }} }};\n{src}\nconsole.log(o);\no.m();"));
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "m" && s.is_call).collect();
    assert!(!v.is_empty(), "no call site of m");
    v.iter().all(|s| {
        let x = format!("{:?}", s.verdict);
        x == "T0Mono" || x.starts_with("T0Mono(")
    })
}

// three.js r8 call 865: ToPrimitive (7.1.1, OrdinaryToPrimitive 7.1.1.1) of
// an object whose chain holds a typed-array prototype looks `valueOf` up
// there (an implicit call made by no program function), then `toString`
// (%TypedArray%.prototype.toString is %Array.prototype.toString%: declared).
const R1: &[&str] = &[
    "const p: any = Object.create(Float32Array.prototype);\nconst x: any = {};\nx[p] = 1;",
    "const p: any = Object.create(Object.getPrototypeOf(Float32Array.prototype));\nconsole.log(\"\" + p);",
];

#[test]
fn r1_typed_array_value_of_is_object_prototype_value_of() {
    for src in R1 {
        assert!(m_mono(src), "RED: ToPrimitive of a typed-array-prototype object holds the realm: {src}");
    }
}

const K1: &str = "Object.getPrototypeOf(Float32Array.prototype).valueOf = function () { return new M(); };\nconst P: any = Object.getPrototypeOf(Float32Array.prototype);\nP.valueOf().go();\nt.valueOf().go();";

#[test]
fn keep_k1_value_of_written_on_typed_array_prototype_runs() {
    let pf = run(K1);
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "go" && s.is_call).collect();
    assert!(v.iter().any(|s| s.top || s.receivers.iter().any(|r| r.ends_with(":M"))), "{}", pf.dump_text());
}
