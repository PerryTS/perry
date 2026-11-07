//! `Array.prototype` taken as a value is the intrinsic prototype, whose
//! image methods the runtime refuses to replace (DEC 250). A method found
//! on it through an object's chain (`Object.create(Array.prototype)`, an
//! array given such a prototype) is that intrinsic method run with the
//! object as `this`: its declared effect, not an opaque native that hands
//! its arguments and receiver to unknown code.
use super::*;

/// Is the read of `n` on the line marked `// SITE_N` proven?
fn site_n(src: &str) -> Result<bool, String> {
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
    Ok(sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))))
}

fn src(setup: &str, read: &str) -> String {
    format!(
        r#"
class Payload {{ n = 1; }}
const payload = new Payload();
{setup}
console.log({read}.n); // SITE_N
"#
    )
}

/// An array method found on `Array.prototype` through the chain runs its
/// effect on the receiver: what `push` stores, `slice` copies into a fresh
/// array, and the argument goes nowhere else.
#[test]
fn array_prototype_method_through_the_chain_is_the_intrinsic_method() {
    let mut failures = Vec::new();
    for (setup, read) in [
        (
            "const p: any = Object.create(Array.prototype); p.push(payload); const s = p.slice();",
            "s[0]",
        ),
        (
            "const p: any = Object.create(Array.prototype); p.push(payload);",
            "payload",
        ),
        (
            "const q: any = Object.create(Array.prototype); const p: any = Object.create(q);
             p.push(payload); const s = p.slice();",
            "s[0]",
        ),
        (
            "const p: any = Object.create(Array.prototype); p.push(payload);
             const s = p.filter((x: any) => x !== undefined);",
            "s[0]",
        ),
        (
            "const o: any = { length: 0 }; Array.prototype.push.call(o, payload);
             const s = Array.prototype.slice.call(o);",
            "s[0]",
        ),
    ] {
        match site_n(&src(setup, read)) {
            Ok(true) => {}
            Ok(false) => failures.push(format!("{setup}: unproven")),
            Err(e) => failures.push(format!("{setup}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "Array.prototype methods: {failures:?}");
}

/// Control: a key `Array.prototype` does not own may be added there (it
/// takes named adds) by a store into ⊤; the call runs what was stored,
/// opaque, and the argument escapes.
#[test]
fn absent_key_on_array_prototype_stays_opaque_under_a_top_store() {
    let s = unseen(&src(
        "const u: any = (globalThis as any).unseenValue;
         u[String(Math.random())] = (globalThis as any).unseenFn;
         const p: any = Object.create(Array.prototype); p.notAnArrayMethod(payload);",
        "payload",
    ));
    assert_eq!(site_n(&s), Ok(false));
}

/// Control: `concat` on an object that is no array (not spreadable) makes
/// the object itself an element (ECMA-262 23.1.3.2.1 IsConcatSpreadable):
/// `s[0]` may be `p`, which has no `n`.
#[test]
fn concat_of_a_non_array_receiver_holds_the_receiver() {
    let pf = facts(&src(
        "const p: any = Object.create(Array.prototype); p.push(payload); const s = p.concat();",
        "s[0]",
    ));
    let at = pf
        .sites
        .iter()
        .find(|s| !s.is_call && s.key == "n")
        .expect("site n");
    assert!(
        at.top || at.receivers.iter().any(|r| r.contains("Object.create")),
        "{}",
        pf.dump_text()
    );
}

/// Control: an object that is no array reads its indices through its chain:
/// `slice` of `p` copies the element its prototype `q` holds at index 0.
#[test]
fn inherited_index_of_a_non_array_receiver_is_an_element() {
    let pf = facts(&src(
        "const q: any = Object.create(Array.prototype); q[0] = payload;
         const p: any = Object.create(q); p.length = 1; const s = p.slice();",
        "s[0]",
    ));
    let at = pf
        .sites
        .iter()
        .find(|s| !s.is_call && s.key == "n")
        .expect("site n");
    assert!(
        at.top || at.receivers.iter().any(|r| r.ends_with(":Payload")),
        "{}",
        pf.dump_text()
    );
}

/// The marker functions of `markers` whose bodies may run in `src`.
fn runs(src: &str, markers: &[&str]) -> Vec<String> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let list = [("t.ts".to_string(), &hir)];
    let pf = crate::compute(&list, &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let on = &pf.body_reach.bodies;
    markers
        .iter()
        .filter(|g| {
            hir.functions
                .iter()
                .find(|f| f.name == **g)
                .is_some_and(|f| on.iter().any(|(_, i, _)| *i == f.id))
        })
        .map(|g| g.to_string())
        .collect()
}

const MARKS: &str = r#"
function gLen() { return 0; }
function sLen() { return 0; }
function vOf() { return 0; }
function gIdx() { return 0; }
function ownJoin() { return 0; }
"#;

/// A generic Array method touches a receiver that is no array only through
/// Get and Set (ECMA-262 23.1.3): `length` is read (and, by the mutators,
/// written) through the program's accessors, own or inherited, and ToLength
/// runs a `valueOf` of what the getter returns. Each marker must be reached.
#[test]
fn length_accessors_of_a_non_array_receiver_run() {
    let mut failures = Vec::new();
    for (prog, want) in [
        (
            "class C { n = 0; get length() { gLen(); return this.n; } set length(v: number) { sLen(); this.n = v; } }
             const c: any = new C(); console.log(Array.prototype.push.call(c, 7));",
            &["gLen", "sLen"][..],
        ),
        (
            "class C { n = 1; get length() { gLen(); return this.n; } set length(v: number) { sLen(); this.n = v; } }
             const c: any = new C(); console.log(Array.prototype.pop.call(c));",
            &["gLen", "sLen"][..],
        ),
        (
            "class C { n = 0; get length() { gLen(); return this.n; } set length(v: number) { sLen(); this.n = v; } }
             const c: any = new C(); const p: any = Object.create(Array.prototype); Object.setPrototypeOf(C.prototype, p);
             console.log(c.push(7));",
            &["gLen", "sLen"][..],
        ),
        (
            "const proto: any = { get length() { gLen(); return 1; }, set length(v: number) { sLen(); } };
             const o: any = Object.create(proto); console.log(Array.prototype.push.call(o, 1));",
            &["gLen", "sLen"][..],
        ),
        (
            "const o: any = { get length() { return { valueOf() { vOf(); return 0; } }; } };
             console.log(Array.prototype.slice.call(o));",
            &["vOf"][..],
        ),
        (
            "class J { length = 0; join() { ownJoin(); return \"j\"; } }
             console.log(Array.prototype.toString.call(new J()));",
            &["ownJoin"][..],
        ),
    ] {
        let got = runs(&format!("{MARKS}\n{prog}"), want);
        if got != want {
            failures.push(format!("{prog}: reached {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// An index accessor of a receiver that is no array runs under `slice` and
/// `map`, and its result is the element they copy.
#[test]
fn index_accessor_of_a_non_array_receiver_runs_and_is_an_element() {
    for (call, read) in [
        ("const s = Array.prototype.slice.call(o);", "s[0]"),
        (
            "const s = Array.prototype.map.call(o, (x: any) => x);",
            "s[0]",
        ),
    ] {
        let prog = format!(
            "class I {{ length = 1; get 0() {{ gIdx(); return payload; }} }}
             const o: any = new I(); {call}"
        );
        let full = format!("{MARKS}\n{}", src(&prog, read));
        assert_eq!(runs(&full, &["gIdx"]), ["gIdx"], "{call}");
        assert_eq!(site_n(&full), Ok(true), "{call}");
    }
}
