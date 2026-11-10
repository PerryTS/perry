//! A computed read under an unknown key on a holder whose chain has a
//! member table (an array, a primitive, a builtin object, a builtin
//! function, a builtin prototype, a builtin namespace) reads the member
//! under every name of the table the key may spell, as the named read finds
//! it (the exact natives), and what a store into ⊤ wrote there; never the
//! generic builtin function, whose unknown call escapes its receiver
//! (DECL0 census group b: the root of the tssrc and skin ⊤ cores).
//! Controls: the holder handed to unknown code, a store of an unknown
//! function into the chain under an unknown key, and a prototype the
//! program gave the holder (it is read, so its method's effect is seen).

use super::box_computed_read::{mints, run, SETUP_ARGV};

/// Holder, the generic mint reason its computed read had on main.
const HOLDERS: &[(&str, &str)] = &[
    (
        "const o: any = Array.prototype;",
        "computed key on a described chain",
    ),
    ("const o: any = [1, 2];", "array computed key"),
    ("const o: any = \"ab\" + k;", "primitive computed key"),
    (
        "const o: any = new Map([[k, 1]]);",
        "builtin object computed key",
    ),
    (
        "const o: any = [].map;",
        "native proto / builtin fn computed key",
    ),
    ("const o: any = Object;", "builtin namespace static"),
];

fn body(holder: &str) -> String {
    format!("{holder}\nconst f = o[k];\nconst h: any = {{ box }};\nh.m = f;\nh.m(\"_\");\nconsole.log(box.witness);")
}

#[test]
fn a_computed_read_on_a_described_holder_mints_no_builtin_function() {
    for (holder, why) in HOLDERS {
        let src = format!("{SETUP_ARGV}{}", body(holder));
        assert_eq!(mints(&src, why), 0, "{why}\n{src}");
    }
}

#[test]
fn a_computed_read_on_a_described_holder_keeps_the_receiver() {
    for (holder, _) in HOLDERS {
        let (ok, d) = run(SETUP_ARGV, &body(holder));
        assert!(ok, "{holder}\n{d}");
    }
}

/// Unknown code given the holder may store any function on it.
#[test]
fn a_holder_handed_to_unknown_code_stays_unknown() {
    for holder in [
        "const o: any = [1, 2];",
        "const o: any = [].map;",
        "const o: any = Object;",
    ] {
        let b = format!(
            "{holder}\nnew Function(process.argv[3])(o);\n{}",
            &body("")[1..]
        );
        let (ok, d) = run(SETUP_ARGV, &b);
        assert!(!ok, "{holder}\n{d}");
    }
}

/// A function of unknown code stored into the chain under an unknown key
/// is what the computed read may find.
#[test]
fn an_unknown_store_into_the_chain_stays_unknown() {
    // String, Map and Function prototypes are final under DEC 250 (an
    // unknown-key store there is refused at run time); the array's is not.
    for (holder, proto) in [("const o: any = [1, 2];", "Array.prototype")] {
        let b = format!(
            "{holder}\n(({proto}) as any)[process.argv[4]] = new Function(process.argv[3]);\n{}",
            &body("")[1..]
        );
        let (ok, d) = run(SETUP_ARGV, &b);
        assert!(!ok, "{holder}\n{d}");
    }
}

/// A prototype the program gave an array is on its chain: the computed read
/// finds its method, whose call hands the witness's holder to `ext`.
#[test]
fn a_given_prototype_is_read() {
    let b = "const o: any = [1, 2];\nObject.setPrototypeOf(o, { m(x: any) { ext(box); } });\nconst f = o[k];\nconst h: any = { };\nh.m = f;\nh.m(\"_\");\nconsole.log(box.witness);";
    let (ok, d) = run(SETUP_ARGV, b);
    assert!(!ok, "{d}");
}

/// The member names are read: `[1][k]` may be `forEach` or `map`, whose
/// call through `call` runs the callback on the array's element `ext`
/// (without the name reads the value is empty and the witness falsely
/// proven).
#[test]
fn the_members_under_the_key_are_read() {
    let b = "const o: any = [1, 2];\nconst f = o[k];\nf.call([ext], (g: any) => g(box));\nconsole.log(box.witness);";
    let (ok, d) = run(SETUP_ARGV, b);
    assert!(!ok, "{d}");
}
