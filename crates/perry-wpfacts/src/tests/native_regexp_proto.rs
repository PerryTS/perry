//! `RegExp.prototype` and `Date.prototype` hold image methods the runtime
//! refuses to replace, delete or redefine (DEC 250: `seal_realm` seals every
//! `BuiltinProto`). A method call on a RegExp or Date receiver under such a
//! key finds the intrinsic method past the receiver's own keys: it runs the
//! model's description of that method, and is not parked for a replay of
//! whatever a store into ⊤ may have put on a builtin prototype. A key the
//! image does not declare may still be added there (both prototypes take
//! adds): that call stays opaque.
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

/// The program around `setup`; with `top`, a store of an unseen function
/// under an unknown key of an unseen value runs first (any builtin
/// prototype may gain any key it takes).
fn src(top: bool, setup: &str, read: &str) -> String {
    let opener = if top {
        "const u: any = (globalThis as any).unseenValue;\nu[String(Math.random())] = (globalThis as any).unseenFn;\n"
    } else {
        ""
    };
    let s = format!(
        "class Payload {{ n = 1; }}\nconst payload = new Payload();\n{opener}{setup}\nconsole.log({read}.n); // SITE_N\n"
    );
    if top { unseen(&s) } else { s }
}

const SUBJECT: &str = "const o: any = { p: payload, toString() { return \"a\"; } };";

/// The image-method calls: under the ⊤ store (F1-F4) and without it (their
/// twins), each runs the described method and the argument (the receiver)
/// goes only where the description says.
fn image_calls() -> Vec<(String, &'static str)> {
    vec![
        // F1
        (format!("const r = /a/; {SUBJECT} r.test(o);"), "o.p"),
        // F2
        (format!("const r = /a/g; {SUBJECT} r.exec(o);"), "o.p"),
        // F3
        (format!("const r: any = /a/; {SUBJECT} r[Symbol.replace](o, \"b\");"), "o.p"),
        (format!("const r: any = /a/; {SUBJECT} r[Symbol.match](o);"), "o.p"),
        // F4 (an own field of a Date reads as an unknown member today, so
        // the Date call takes the argument: `setTime` ToNumbers it)
        (
            "const dt: any = new Date(0); const o: any = { p: payload, valueOf() { return 1; } }; dt.setTime(o);"
                .to_string(),
            "o.p",
        ),
    ]
}

#[test]
fn regexp_and_date_image_methods_are_final_under_a_top_store() {
    let mut failures = Vec::new();
    for (setup, read) in image_calls() {
        match site_n(&src(true, &setup, read)) {
            Ok(true) => {}
            Ok(false) => failures.push(format!("{setup}: unproven")),
            Err(e) => failures.push(format!("{setup}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "RegExp/Date image methods: {failures:?}");
}

#[test]
fn regexp_and_date_image_methods_without_a_top_store() {
    let mut failures = Vec::new();
    let split = (format!("const r: any = /a/; {SUBJECT} r[Symbol.split](o);"), "o.p");
    for (setup, read) in image_calls().into_iter().chain([split]) {
        match site_n(&src(false, &setup, read)) {
            Ok(true) => {}
            Ok(false) => failures.push(format!("{setup}: unproven")),
            Err(e) => failures.push(format!("{setup}: {}", e.lines().next().unwrap())),
        }
    }
    assert!(failures.is_empty(), "RegExp/Date image methods: {failures:?}");
}

/// Control (the named one): a key `RegExp.prototype` does not declare may
/// be added there by the store into ⊤; the call runs what was stored,
/// opaque, and the argument escapes.
#[test]
fn absent_key_on_regexp_prototype_stays_opaque_under_a_top_store() {
    let s = src(
        true,
        &format!("const r: any = /a/; {SUBJECT} try {{ r.notAMethod(o); }} catch (e) {{}}"),
        "o.p",
    );
    assert_eq!(site_n(&s), Ok(false));
}

/// Control: an escaped regexp may gain an own `exec` from unknown code; the
/// lookup finds it before the prototype's.
#[test]
fn own_key_of_an_escaped_regexp_is_not_the_prototype_read() {
    let s = src(
        true,
        &format!("const r: any = /a/; (globalThis as any).keep = r; {SUBJECT} r.exec(o);"),
        "o.p",
    );
    assert_eq!(site_n(&s), Ok(false));
}

/// Control: the program adds a method under a computed key to
/// `RegExp.prototype`; a call under a key the image does not declare runs
/// it, and it overwrites `o.p`.
#[test]
fn program_add_to_regexp_prototype_runs() {
    let s = src(
        false,
        &format!(
            "function key(): string {{ return [\"my\", \"M\"].join(\"\"); }}
             (RegExp.prototype as any)[key()] = function (q: any) {{ q.p = {{ m: 1 }}; }};
             const r: any = /a/; {SUBJECT} r.myM(o);"
        ),
        "o.p",
    );
    assert_eq!(site_n(&s), Ok(false));
}


/// Control: `@@split` (and `String.prototype.split` through it) constructs
/// the splitter with SpeciesConstructor(rx, %RegExp%), and `constructor`
/// stays patchable on `RegExp.prototype` (DEC 250 exempts it): unknown code
/// may receive the regexp and give it own keys, so a later call on it is
/// not the prototype read.
#[test]
fn split_hands_the_regexp_to_its_species_constructor() {
    for setup in [
        format!("const r: any = /a/; {SUBJECT} r[Symbol.split](\"x\"); r.exec(o);"),
        format!("const r: any = /a/; {SUBJECT} r[Symbol.split](o);"),
    ] {
        assert_eq!(site_n(&src(true, &setup, "o.p")), Ok(false), "{setup}");
    }
}
