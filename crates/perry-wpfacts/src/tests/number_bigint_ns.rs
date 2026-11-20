//! `Number` and `BigInt` are two constructors with two prototypes
//! (ECMA-262 21.1, 21.2): a number primitive reads `Number.prototype`'s
//! members, a bigint primitive `BigInt.prototype`'s, and each one's
//! `constructor` is its own constructor.
use super::*;

/// The receivers of the `size` read on `x`, a `Svc` or the value `v`.
fn receivers_of(v: &str) -> Vec<String> {
    let src = format!(
        "class Svc {{ size = 3; }}\n\
         const v: any = {v};\n\
         const x: any = process.argv.length > 9 ? new Svc() : v;\n\
         console.log(x.size);\n"
    );
    site(&facts(&src), "size").receivers.clone()
}

/// Red on main: one namespace stood for both, whose prototype was
/// `BigInt.prototype`, so a number's `toString` was `BigInt.prototype`'s,
/// a number's `toFixed` and every bigint member the shared builtin-function
/// token, and a bigint's `constructor` the `Number` namespace.
#[test]
fn a_number_and_a_bigint_read_their_own_prototype() {
    let cases = [
        ("(5).toString", "builtin-fn:Number.prototype.toString"),
        ("(5).toFixed", "builtin-fn:Number.prototype.toFixed"),
        ("(5n).toString", "builtin-fn:BigInt.prototype.toString"),
        (
            "(5n).toLocaleString",
            "builtin-fn:BigInt.prototype.toLocaleString",
        ),
        (
            "Number.prototype.toString",
            "builtin-fn:Number.prototype.toString",
        ),
        (
            "BigInt.prototype.valueOf",
            "builtin-fn:BigInt.prototype.valueOf",
        ),
        ("(5).constructor", "builtin:Number"),
        ("(5n).constructor", "builtin:BigInt"),
    ];
    let mut wrong = Vec::new();
    for (v, want) in cases {
        let r = receivers_of(v);
        if !r.iter().any(|t| t == want) || r.iter().any(|t| t == "builtin-fn") {
            wrong.push(format!("{v}: want {want}, got {r:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
