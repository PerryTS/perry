//! A builtin prototype taken as a value (`Map.prototype`,
//! `Object.getPrototypeOf(/x/)`) is that builtin's prototype object: an
//! object whose chain it is reads the builtin's members and accessors,
//! not only `Object.prototype`'s.
use super::*;

fn site_n(src: &str, proven: bool) -> Result<(), String> {
    let at = src.find("// SITE_N").expect("named site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing SITE_N\n{}", pf.dump_text()));
    }
    if sites
        .iter()
        .any(|s| !matches!(s.verdict, Verdict::No(_)) != proven)
    {
        return Err(format!(
            "SITE_N expected proven={proven}: {:?}\n{}",
            sites.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
            pf.dump_text()
        ));
    }
    Ok(())
}

fn lookup_src(setup: &str, key: &str) -> String {
    format!(
        r#"
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
const found: any = receiver.__lookupGetter__("{key}");
if (found) found.ref = payload;
console.log(payload.n); // SITE_N
"#
    )
}

/// The getter a builtin prototype holds is found through an object whose
/// chain that prototype is: the lookup returns a native function, which a
/// store hands the payload to.
#[test]
fn builtin_proto_value_accessor_lookup_finds_the_builtins_getter() {
    let mut failures = Vec::new();
    for (setup, key) in [
        (
            "const receiver: any = Object.create(Map.prototype);",
            "size",
        ),
        (
            "const receiver: any = Object.create(Set.prototype);",
            "size",
        ),
        (
            "const receiver: any = Object.create(Object.getPrototypeOf(/x/));",
            "flags",
        ),
        (
            "const receiver: any = Object.create(RegExp.prototype);",
            "source",
        ),
        (
            "const receiver: any = Object.create(Object.getPrototypeOf(new Map()));",
            "size",
        ),
        (
            "const receiver: any = {}; Object.setPrototypeOf(receiver, Set.prototype);",
            "size",
        ),
    ] {
        if let Err(e) = site_n(&lookup_src(setup, key), false) {
            failures.push(format!("{setup}: {}", e.lines().next().unwrap()));
        }
    }
    assert!(
        failures.is_empty(),
        "builtin prototype getters: {failures:?}"
    );
}

/// Controls: `Object.prototype` holds no `size` getter; a lookup through a
/// plain object or `Object.prototype` itself finds none.
#[test]
fn builtin_proto_value_object_prototype_lookup_stays_proven() {
    for setup in [
        "const receiver: any = Object.create({});",
        "const receiver: any = Object.create(Object.prototype);",
        "const receiver: any = {};",
    ] {
        site_n(&lookup_src(setup, "size"), true).unwrap_or_else(|e| panic!("{setup}: {e}"));
    }
}

/// A method read through an object whose chain is `Map.prototype` is
/// `Map.prototype.get`: called on a map it returns the map's value, whose
/// field the program then writes with a string.
#[test]
fn builtin_proto_value_method_read_is_the_builtins_method() {
    let src = r#"
class Payload { n = 1; }
const payload = new Payload();
const m = new Map<string, any>();
m.set("k", payload);
const p: any = Object.create(Map.prototype);
const v: any = p.get.call(m, "k");
v.n = "s";
console.log(payload.n);
"#;
    let dump = facts(src).dump_text();
    assert!(
        dump.contains("FACT F2 kind t.ts:Payload.n = num|str"),
        "Payload.n misses the string stored through the builtin method's result:\n{dump}"
    );
}
