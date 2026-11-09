//! `process.argv`: one array the runtime makes before the program runs,
//! its elements strings. `process.argv` and a load of `argv` (or a computed
//! key) from `process` denote it; what the program pushes or stores there
//! is in it, and unknown code given `process` or the global object may
//! write it.

use super::*;

/// The verdicts of the method-call sites of `key`.
fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// chess main.ts:35 / chess.ts:477: a string read from `process.argv`,
/// passed to a constructor and trimmed.
#[test]
fn an_argv_element_is_a_string_receiver() {
    let pf = facts(
        r#"
        class G { constructor(fen: string) { this.read(fen); } read(fen: string): void { const f = fen.trim().split(/\s+/); console.log(f.length); } }
        let fen = "a b";
        for (let i = 2; i < process.argv.length; i++) { const a = process.argv[i] ?? ""; if (a === "--fen") fen = process.argv[i + 1] ?? ""; }
        new G(fen);
    "#,
    );
    for key in ["trim"] {
        assert!(
            native(&pf, key),
            "{key}: {:?}\n{}",
            calls(&pf, key),
            pf.dump_text()
        );
    }
}

/// `<write>` then `process.argv[n].trim()`: the control (no write) is a
/// native member; each write makes it unproven.
#[test]
fn a_write_to_argv_reaches_its_reads() {
    let use_ = "console.log((process.argv[process.argv.length - 1] ?? \"\").trim());";
    let control = facts(use_);
    assert!(
        native(&control, "trim"),
        "{:?}\n{}",
        calls(&control, "trim"),
        control.dump_text()
    );
    for write in [
        // The program's own push.
        "process.argv.push(5 as any);",
        // Through `process` held as a value, by name and by a computed key.
        "const p: any = process; p.argv.push(1);",
        "const p: any = process; p[\"ar\" + String(process.argv.length).slice(9)].push(1);",
        "const g: any = globalThis; g.process.argv.push(1);",
        // A store of another array.
        "(process as any).argv = [5];",
        // `process`, or the global object, handed to unknown code.
        "const h: any = (globalThis as any)[\"hook\" + process.argv.length]; if (h) h(process);",
        "const h: any = (globalThis as any)[\"hook\" + process.argv.length]; if (h) h(globalThis);",
    ] {
        let pf = facts(&format!("{write}\n{use_}"));
        assert!(
            unproven(&pf, "trim"),
            "{write}: {:?}\n{}",
            calls(&pf, "trim"),
            pf.dump_text()
        );
    }
}

/// An object pushed onto `process.argv`: the field read from an element is
/// not a number receiver.
#[test]
fn an_object_pushed_onto_argv_is_an_element() {
    let pf = facts(
        r#"
        process.argv.push({ x: 1 } as any);
        console.log((process.argv[process.argv.length - 1] as any).x.toFixed());
    "#,
    );
    assert!(
        unproven(&pf, "toFixed"),
        "{:?}\n{}",
        calls(&pf, "toFixed"),
        pf.dump_text()
    );
}
