//! Rule R1 (STR1): a store under a key the realm does not declare for a
//! built-in namespace or native function object is a field of that object
//! (`solve/builtin_fields.rs`). The value stays the program's: a receiver
//! that may be such an object (a computed read of a dictionary may yield the
//! inherited `constructor`, `Object`, or an `Object.prototype` method) no
//! longer sinks it, and its readers see it. A declared key, the anonymous
//! native as the store receiver, and a holder handed to unknown code keep
//! the opaque path. Fixtures from /root/STR1/fixtures (f1, f1t, f2, f3, f3t).
use super::*;

/// The read sites under `key` on the line holding `marker`.
fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let end = m as u32;
    let s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == key && (start..end).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn proven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    at(pf, src, marker, key)
        .iter()
        .all(|s| !matches!(s.verdict, Verdict::No(_)))
}

fn unproven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    at(pf, src, marker, key)
        .iter()
        .all(|s| matches!(s.verdict, Verdict::No(_)))
}

const SF: &str = "class SF { text: string; lineMap: number[] | undefined; \
    constructor(t: string) { this.text = t; this.lineMap = undefined; } }\n";

/// f1 (scanner.ts:503): `files[k]` may be the inherited `constructor`
/// (Object) or an `Object.prototype` method; `lines` stores the map on what
/// it read. Main: the stored value is sunk and `lineMap` on `Object` is
/// written, so `lines(sf).n` reads an opaque member (⊤). (The fixture's
/// array `.length` is judged `OwnLength` only without `undefined` beside
/// the array, which `sf.lineMap || ...` keeps: the map is a class here;
/// `f1_array_length_is_no_longer_top` runs the array.)
#[test]
fn a_field_stored_on_what_a_dictionary_read_returns_stays_the_programs() {
    let src = "class LM { n: number; constructor() { this.n = 0; } }\n\
         class SF { text: string; lineMap: LM | undefined; constructor(t: string) { this.text = t; this.lineMap = undefined; } }\n\
         const files: { [k: string]: SF } = {};\n\
         files[process.argv[2] ?? \"a\"] = new SF(\"hello\");\n\
         function lines(sf: SF): LM { return sf.lineMap || (sf.lineMap = new LM()); }\n\
         const sf = files[process.argv[3] ?? \"a\"];\n\
         if (sf) console.log(lines(sf).n); // SITE-L\n";
    let pf = facts(src);
    assert!(proven(&pf, src, "// SITE-L", "n"));
}

/// The fixture f1 itself (an array map): the stored array is no longer
/// sunk, so `.length` no longer reads ⊤ (it stays unproven for the
/// `undefined` the `||` keeps beside the array).
#[test]
fn f1_array_length_is_no_longer_top() {
    let src = format!(
        "{SF}const files: {{ [k: string]: SF }} = {{}};\n\
         files[process.argv[2] ?? \"a\"] = new SF(\"hello\\nworld\");\n\
         function lines(sf: SF): number[] {{ return sf.lineMap || (sf.lineMap = [0, sf.text.length]); }}\n\
         const sf = files[process.argv[3] ?? \"a\"];\n\
         if (sf) console.log(lines(sf).length); // SITE-A\n"
    );
    let pf = facts(&src);
    let s = at(&pf, &src, "// SITE-A", "length");
    assert!(
        s.iter().all(|s| !s.top),
        "{:?}",
        s.iter().map(|s| &s.verdict).collect::<Vec<_>>()
    );
}

/// f1t: the program really stores a value on `Object` under `lineMap` (one
/// from unknown code here; a primitive would not change the verdict), so
/// `files[k]` (`k` = "constructor") may return `Object` whose `lineMap` is
/// that value: the read stays unproven.
#[test]
fn a_program_field_really_stored_on_a_namespace_is_read_back() {
    let src = "class LM { n: number; constructor() { this.n = 0; } }\n\
         class SF { text: string; lineMap: LM | undefined; constructor(t: string) { this.text = t; this.lineMap = undefined; } }\n\
         const files: { [k: string]: SF } = {};\n\
         files[process.argv[2] ?? \"a\"] = new SF(\"hello\");\n\
         (Object as any).lineMap = (globalThis as any)[process.argv[4] ?? \"y\"];\n\
         function lines(sf: SF): LM { return sf.lineMap || (sf.lineMap = new LM()); }\n\
         const sf = files[process.argv[3] ?? \"a\"];\n\
         if (sf) console.log(lines(sf).n); // SITE-B\n";
    let pf = facts(src);
    assert!(unproven(&pf, src, "// SITE-B", "n"));
}

/// f2: TypeScript core.ts `getProperty` (guarded by `hasOwnProperty.call`,
/// which the solver does not follow), then scanner.ts's field store.
#[test]
fn a_maplike_get_property_value_keeps_its_stored_field() {
    let src = "interface MapLike<T> { [k: string]: T }\n\
         const hasOwn = Object.prototype.hasOwnProperty;\n\
         function getProperty<T>(map: MapLike<T>, key: string): T | undefined { return hasOwn.call(map, key) ? map[key] : undefined; }\n\
         class LM { n: number; constructor() { this.n = 0; } }\n\
         class SF { text: string; lineMap: LM | undefined; constructor(t: string) { this.text = t; this.lineMap = undefined; } }\n\
         const files: MapLike<SF> = {};\n\
         files[process.argv[2] ?? \"a\"] = new SF(\"hello\");\n\
         function getLineStarts(sf: SF): LM { return sf.lineMap || (sf.lineMap = new LM()); }\n\
         const sf = getProperty(files, process.argv[3] ?? \"a\");\n\
         if (sf) console.log(getLineStarts(sf).n); // SITE-C\n";
    let pf = facts(src);
    assert!(proven(&pf, src, "// SITE-C", "n"));
}

const K: &str = "class In { n: number; constructor(n: number) { this.n = n; } }\n\
    class K { inner: In; constructor(n: number) { this.inner = new In(n); } }\n";

/// f3 with identified natives (`Array.prototype` methods read off an
/// array): a store on what may be one of them is a field every native
/// function value shares; the stored object is not sunk.
#[test]
fn a_store_on_an_identified_native_is_a_field() {
    let src = format!(
        "{K}const table: {{ [s: string]: any }} = {{ push: [].push, pop: [].pop }};\n\
         const f = table[process.argv[2] ?? \"push\"];\n\
         const k1 = new K(1);\n\
         (f as any).tag = k1;\n\
         console.log(k1.inner.n.toFixed(0)); // SITE-M\n"
    );
    let pf = facts(&src);
    assert!(proven(&pf, &src, "// SITE-M", "inner"));
    assert!(proven(&pf, &src, "// SITE-M", "n"));
}

/// Its twin: the field is read back through ANOTHER native function value
/// (an alias may be the same object) and changed: the shared field holds
/// `k1`, so `k1.inner` may be a value from unknown code.
#[test]
fn a_native_field_is_read_back_through_any_native() {
    let src = format!(
        "{K}const table: {{ [s: string]: any }} = {{ push: [].push, pop: [].pop }};\n\
         const f = table[process.argv[2] ?? \"push\"];\n\
         const k1 = new K(1);\n\
         (f as any).tag = k1;\n\
         (([] as any).slice.tag as any).inner = (globalThis as any)[process.argv[3] ?? \"y\"];\n\
         console.log(k1.inner.n); // SITE-MT\n"
    );
    let pf = facts(&src);
    assert!(unproven(&pf, &src, "// SITE-MT", "n"));
}

/// f3 itself: `Math.max` is a namespace static, the anonymous native
/// (`Native::Any`), which may be a native the description does not cover:
/// a store on it keeps the opaque path (rule R1 case c).
#[test]
fn a_store_on_the_anonymous_native_keeps_the_opaque_path() {
    let src = format!(
        "{K}const table: {{ [s: string]: (a: number, b: number) => number }} = {{ max: Math.max, min: Math.min }};\n\
         const f = table[process.argv[2] ?? \"max\"];\n\
         const k1 = new K(1);\n\
         (f as any).tag = k1;\n\
         console.log(k1.inner.n.toFixed(0)); // SITE-D\n"
    );
    let pf = facts(&src);
    assert!(unproven(&pf, &src, "// SITE-D", "inner"));
}

/// f3t: the program reads the native's field back and replaces what it
/// holds: stays unproven.
#[test]
fn f3_twin_stays_unproven() {
    let src = format!(
        "{K}const table: {{ [s: string]: (a: number, b: number) => number }} = {{ max: Math.max, min: Math.min }};\n\
         const f = table[process.argv[2] ?? \"max\"];\n\
         const k1 = new K(1);\n\
         (f as any).tag = k1;\n\
         ((Math.max as any).tag as any).inner = \"str\";\n\
         console.log(k1.inner.n); // SITE-D2\n"
    );
    let pf = facts(&src);
    assert!(unproven(&pf, &src, "// SITE-D2", "n"));
}

/// A declared key keeps today's path: the runtime reads it. A function
/// stored as `Error.prepareStackTrace` is called by the runtime with its
/// call sites; one stored as `Object.keys` by any caller; an object stored
/// as `Error.stackTraceLimit` is converted by the runtime.
#[test]
fn a_store_under_a_declared_key_keeps_todays_path() {
    let src = "class L { n: number; constructor() { this.n = 3; } }\n\
         (Error as any).prepareStackTrace = (e: any, cs: any) => cs.length; // SITE-P\n\
         (Object as any).keys = (o: any) => o.size; // SITE-K\n\
         const lim = new L();\n\
         (Error as any).stackTraceLimit = lim;\n\
         console.log(lim.n.toFixed(0)); // SITE-S\n";
    let pf = facts(src);
    assert!(unproven(&pf, src, "// SITE-P", "length"));
    assert!(unproven(&pf, src, "// SITE-K", "size"));
    assert!(unproven(&pf, src, "// SITE-S", "n"));
}

/// A field the program stores on a namespace and on a native function value
/// is what it reads back there.
#[test]
fn a_stored_builtin_field_is_read_back() {
    let src = "class V { n: number; constructor() { this.n = 1; } }\n\
         const v = new V();\n\
         (Object as any).myTag = v;\n\
         const w = (Object as any).myTag;\n\
         if (w) console.log(w.n); // SITE-R\n\
         ([] as any).push.myTag = v;\n\
         const w2 = ([] as any).pop.myTag;\n\
         if (w2) console.log(w2.n); // SITE-R2\n\
         console.log(v.n.toFixed(0)); // SITE-V\n";
    let pf = facts(src);
    for m in ["// SITE-R", "// SITE-R2"] {
        let s = at(&pf, src, m, "n");
        assert!(
            s.iter()
                .all(|s| s.receivers.iter().any(|r| r.contains('V'))),
            "{m}: {:?}",
            s.iter().map(|s| &s.receivers).collect::<Vec<_>>()
        );
    }
    assert!(proven(&pf, src, "// SITE-V", "n"));
}

/// A namespace handed to unknown code: what it holds under a program key
/// may be read there, so it is sunk as an escaped object's field.
#[test]
fn a_builtin_handed_to_unknown_code_sinks_its_fields() {
    let src = "class V { n: number; constructor() { this.n = 1; } }\n\
         const v = new V();\n\
         (Object as any).myTag = v;\n\
         const g: any = (globalThis as any)[process.argv[2] ?? \"x\"];\n\
         g(Object);\n\
         console.log(v.n.toFixed(0)); // SITE-E\n";
    let pf = facts(src);
    assert!(unproven(&pf, src, "// SITE-E", "n"));
}
