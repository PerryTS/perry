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

/// The computed read is the named reads of its table: where every member
/// the key may spell keeps a foreign receiver as its named read does (an
/// array's, a primitive's chain), so does the computed read. The other
/// holders' tables hold a member whose NAMED read, called as a method of
/// the witness's holder, already escapes it (`Map.prototype.get`,
/// `Function.prototype.call`, `Object.create`; node: each throws a
/// TypeError on that receiver or ignores it): the computed read sees the
/// same (`the_computed_read_is_the_named_reads`).
#[test]
fn a_computed_read_on_a_described_holder_keeps_the_receiver() {
    for (holder, _) in &HOLDERS[..3] {
        let (ok, d) = run(SETUP_ARGV, &body(holder));
        assert!(ok, "{holder}\n{d}");
    }
}

/// Controls of the above: a member of the table whose named read escapes
/// the receiver makes the computed read escape it, and the computed read
/// escapes it for no other reason (no generic builtin function:
/// `a_computed_read_on_a_described_holder_mints_no_builtin_function`).
#[test]
fn the_computed_read_is_the_named_reads() {
    for (holder, named) in [
        (HOLDERS[3].0, "o.get"),
        (HOLDERS[4].0, "o.call"),
        (HOLDERS[5].0, "o.create"),
    ] {
        let named_body = body(holder).replace("o[k]", named);
        let (by_name, d) = run(SETUP_ARGV, &named_body);
        assert!(!by_name, "{named}\n{d}");
        let (computed, d) = run(SETUP_ARGV, &body(holder));
        assert!(!computed, "{holder}\n{d}");
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

thread_local! {
    /// The shared chain nodes of the last solve: (family, member values).
    static CHAIN_NODES: std::cell::RefCell<Vec<(String, Vec<String>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The chain-any nodes of the last solve (`Model::chain_any_nodes`), with
/// the values each holds, by name.
pub(crate) fn record(m: &crate::model::Model) {
    let mut v: Vec<(String, Vec<String>)> = m
        .chain_any_nodes
        .iter()
        .map(|(&(fam, _, _), &n)| {
            let mut toks = Vec::new();
            m.nodes[n as usize].pts.sorted_into(&mut toks);
            (
                format!("{fam:?}"),
                toks.iter().map(|&t| m.tok_name(t)).collect(),
            )
        })
        .collect();
    v.sort();
    CHAIN_NODES.with_borrow_mut(|s| *s = v);
}

/// COST1 D1: an unknown-key read of two builtin functions (`[].push`,
/// `Math.max`) reads ONE node of `Function.prototype`'s chain under that
/// key, filled once with the members by name (the exact natives), and
/// mint no generic builtin function. (Called as a method of the witness's
/// holder, `Function.prototype.call` escapes it as its named read does:
/// `the_computed_read_is_the_named_reads`.)
#[test]
fn builtin_functions_under_one_key_read_one_chain_node() {
    let body = "const p: any = [].push;\nconst x: any = Math.max;\nconst f = p[k];\nconst g = x[k];\nconst h: any = { box };\nh.m = f;\nh.m(\"_\");\nh.n = g;\nh.n(\"_\");\nconsole.log(box.witness);";
    let _ = run(SETUP_ARGV, body);
    let nodes = CHAIN_NODES.with_borrow(|s| s.clone());
    let fun: Vec<_> = nodes.iter().filter(|(f, _)| f == "Function").collect();
    assert_eq!(fun.len(), 1, "one Function chain node per key: {nodes:?}");
    let names = fun[0].1.join(" ");
    for m in ["call", "apply", "bind"] {
        assert!(names.contains(m), "member {m} by name: {names}");
    }
    let src = format!("{SETUP_ARGV}{body}");
    assert_eq!(mints(&src, "computed key on a described chain"), 0, "{src}");
}
