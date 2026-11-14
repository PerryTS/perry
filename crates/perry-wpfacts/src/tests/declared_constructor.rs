//! `new Name(...)` of a realm constructor that declares its effects
//! (`#[realm(Name, kind = constructor, effects = { ... })]`) does what its
//! declaration says to its arguments, while the global `Name` holds the
//! realm's constructor: `new CustomEvent(type, { detail: this })` keeps
//! `this` in the event's `detail` field, runs the options' getters and the
//! type's ToString, and gives `this` to no one else. A constructor the
//! program (or unmodeled code) replaced under its global name is unknown
//! code again: its arguments escape.
use super::*;

fn at<'a>(pf: &'a ProgramFacts, src: &str) -> Vec<&'a SiteFacts> {
    let m = src.find("// SITE").expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "x" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `x` site on the SITE line");
    s
}

fn verdicts(pf: &ProgramFacts, src: &str) -> String {
    format!(
        "{:?}",
        at(pf, src)
            .iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str) -> bool {
    at(pf, src)
        .iter()
        .all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// The site's facts admit a `W` receiver: unproven, or proven with `W`.
fn admits_w(pf: &ProgramFacts, src: &str) -> bool {
    at(pf, src).iter().all(|s| {
        matches!(s.verdict, Verdict::No(_)) || s.receivers.iter().any(|r| r.ends_with(":W"))
    })
}

/// `body` runs in `S`'s constructor after `this.v = new V(1)`; `before`
/// runs first at top level. The SITE reads `s.v.x`.
fn program(before: &str, body: &str) -> String {
    format!(
        "class V {{ x: number; constructor(x: number) {{ this.x = x; }} }}\n\
         class W {{ x: number; constructor(x: number) {{ this.x = x * 10; }} }}\n\
         {before}\n\
         class S {{ v: any; constructor() {{ this.v = new V(1); const self: any = this; {body} }} }}\n\
         const s = new S();\n\
         console.log(s.v.x); // SITE\n"
    )
}

/// Red on main: every argument of `new CustomEvent` escaped (the unknown
/// constructor), and `this` with it (three.core.js:15325's shape).
#[test]
fn custom_event_detail_keeps_this() {
    let src = program(
        "",
        "if (typeof (globalThis as any).__devtools !== \"undefined\") { (globalThis as any).__devtools.dispatchEvent(new CustomEvent(\"observe\", { detail: this })); }",
    );
    let pf = facts(&src);
    assert!(proven(&pf, &src), "{}", verdicts(&pf, &src));
    assert!(!admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Red on main: `Event` reads `bubbles` as a boolean only.
#[test]
fn event_options_keep_this() {
    let src = program("", "new Event(\"observe\", { bubbles: this } as any);");
    let pf = facts(&src);
    assert!(proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: a write through the event's `detail` is a write to `this`.
#[test]
fn a_write_through_detail_reaches_this() {
    let src = program(
        "",
        "const e: any = new CustomEvent(\"observe\", { detail: this }); e.detail.v = new W(2);",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the constructor runs the options' `detail` getter.
#[test]
fn the_options_getter_runs() {
    let src = program(
        "",
        "new CustomEvent(\"observe\", { get detail() { self.v = new W(2); return 0; } });",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the `bubbles` getter of `Event`'s options runs too.
#[test]
fn the_event_options_getter_runs() {
    let src = program(
        "",
        "new Event(\"observe\", { get bubbles() { self.v = new W(2); return false; } });",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the type's ToString runs the program's `toString`.
#[test]
fn the_type_to_string_runs() {
    let src = program(
        "",
        "new CustomEvent({ toString() { self.v = new W(2); return \"t\"; } } as any);",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: an event that escapes gives its `detail` to unknown code.
#[test]
fn an_escaped_event_escapes_detail() {
    let src = program(
        "",
        "(globalThis as any)[\"h\" + String(process.argv.length)](new CustomEvent(\"observe\", { detail: this }));",
    );
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: a program store replaces the constructor under its global
/// name; `new CustomEvent` is unknown code again.
#[test]
fn a_replaced_constructor_is_unknown() {
    let src = program(
        "(globalThis as any).CustomEvent = class { constructor(t: any, o: any) { o.detail.v = new W(2); } };",
        "new CustomEvent(\"observe\", { detail: this });",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the same replacement written later in the source (a function
/// that runs before `S`): the construct is unknown code once the write is
/// seen, whatever the solve order (the replay).
#[test]
fn a_later_replacement_is_unknown() {
    let src = format!(
        "{}\nfunction replace() {{ (globalThis as any).CustomEvent = function (t: any, o: any) {{ o.detail.v = new W(2); }}; }}\n",
        program("replace();", "new CustomEvent(\"observe\", { detail: this });")
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: an undeclared method of the event (unknown code run on it)
/// sees its `detail`.
#[test]
fn an_undeclared_event_method_sees_detail() {
    let src = program(
        "",
        "const e = new CustomEvent(\"observe\", { detail: this }); e.preventDefault();",
    );
    let pf = facts(&src);
    assert!(!proven(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Red on main: every argument of `new Blob` escaped (the unknown
/// constructor), `this` with it (GLTFLoader's `new Blob([bufferView])`
/// shape). The constructor reads its Array of parts by index and converts
/// each one that is not bytes to a string; it reads the options' `type` and `endings`.
#[test]
fn blob_parts_keep_this() {
    let src = program("", "new Blob([this, \"x\"], { type: \"text/plain\" });");
    let pf = facts(&src);
    assert!(proven(&pf, &src), "{}", verdicts(&pf, &src));
    assert!(!admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: a part's `toString` runs.
#[test]
fn the_blob_part_to_string_runs() {
    let src = program(
        "",
        "new Blob([{ toString() { self.v = new W(2); return \"\"; } }] as any);",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// The parts' iterator never runs: the runtime reads an Array part list by
/// index and takes any other value as one part (perry-stdlib fetch_blob.rs
/// `append_blob_parts`), converted by ToString: `Object.prototype.toString`
/// here, which calls nothing of the program.
#[test]
fn the_blob_parts_iterator_never_runs() {
    let src = program(
        "",
        "new Blob({ [Symbol.iterator]() { self.v = new W(2); return [][Symbol.iterator](); } } as any);",
    );
    let pf = facts(&src);
    assert!(!admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}

/// Control: the options' `type` getter runs.
#[test]
fn the_blob_options_getter_runs() {
    let src = program(
        "",
        "new Blob([], { get type() { self.v = new W(2); return \"\"; } } as any);",
    );
    let pf = facts(&src);
    assert!(admits_w(&pf, &src), "{}", verdicts(&pf, &src));
}
