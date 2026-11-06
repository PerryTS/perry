//! `console` and `process.stdout` / `stderr` / `stdin` are host objects
//! perry's runtime creates: one token each, whose described members are
//! the runtime's natives until a store reaches the key (DECISIONS 255:
//! `builtins::host_written`, read by the solver's host calls and loads and
//! by the site verdict).

use super::*;

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

fn open(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

const USES: &str = r#"
    console.log("ok");
    process.stdout.write("line\n");
    process.stderr.write("");
    const input = process.stdin;
    input.setEncoding("utf8");
    "#;

#[test]
fn host_members_are_the_runtimes_natives() {
    let pf = facts(USES);
    for k in ["log", "write", "setEncoding"] {
        assert!(native(&pf, k), "{k}: {:?}\n{}", calls(&pf, k), pf.dump_text());
    }
}

/// A key the runtime's layouts do not both hold is not described.
#[test]
fn an_undescribed_host_member_stays_open() {
    let pf = facts("const o = process.stdout; o.cork(); const i = process.stdin; i.setRawMode(true);");
    assert!(open(&pf, "cork"), "{:?}\n{}", calls(&pf, "cork"), pf.dump_text());
    assert!(open(&pf, "setRawMode"), "{:?}\n{}", calls(&pf, "setRawMode"), pf.dump_text());
}

/// Soundness: each of these may give the member another value.
#[test]
fn a_written_host_member_stays_open() {
    for (what, key, extra) in [
        ("a store of the member", "write", "process.stdout.write = function (s: any) { return true; } as any;"),
        ("a store into console", "log", "console.log = function (...a: any[]) {};"),
        ("a computed store", "write", "(process.stdout as any)[\"wr\" + String(Date.now() % 2)] = 1;"),
        ("a store into ⊤", "write", "import \"unseen-host\"; const t: any = (globalThis as any).someHost; t.write = 3;"),
        ("unknown code holding it", "write", "const g = new Function(\"o\", \"o.write = 1\"); g(process.stdout);"),
    ] {
        let pf = facts(&format!("console.log(\"ok\"); process.stdout.write(\"x\"); {extra}"));
        assert!(open(&pf, key), "{what}: {:?}\n{}", calls(&pf, key), pf.dump_text());
    }
}

/// A store into a host object hands the stored value to the runtime (node's
/// `console.log` calls a replaced `process.stdout.write`), so the parameter
/// of the stored function is unknown code's argument: its reads stay open.
#[test]
fn a_function_stored_on_a_host_object_is_called_by_the_runtime() {
    let pf = facts(
        "const w = function (s: any) { return s.zq; }; (process.stdout as any).write = w; console.log(\"x\");",
    );
    let v: Vec<Verdict> = pf
        .sites
        .iter()
        .filter(|s| s.key == "zq")
        .map(|s| s.verdict.clone())
        .collect();
    assert!(
        !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_))),
        "{v:?}\n{}",
        pf.dump_text()
    );
}
