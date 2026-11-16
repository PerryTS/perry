//! DEC 250, a define through ⊤: `Object.defineProperty`, `Reflect.defineProperty`
//! and Annex B `__defineGetter__` / `__defineSetter__` on a ⊤ receiver (which
//! may be any built-in prototype) redefine nothing the runtime refuses to
//! redefine. Replacing an image member of a built-in prototype throws
//! (`Reflect.defineProperty` returns false) and installs nothing, so a read
//! whose whole chain is final (`members::locked_read`, on
//! `members::store_admitted`) keeps its native member. An admitted define
//! (a named add on `Array.prototype`, a program prototype) still counts.
use super::*;

fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty()
        && v
            .iter()
            .all(|v| matches!(v, Verdict::T0Mono | Verdict::T0Set | Verdict::NativeMember))
}

fn open(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

#[test]
fn defines_through_top_keep_locked_native_members() {
    let pf = facts(
        r#"
        const u: any = new Function(process.argv[2])();
        u.__defineGetter__("x", () => 1);
        Object.defineProperty(u, "replace", { get() { return 1; } });
        console.log("abc".replace("a", "b"), [1, 2].shift());
        "#,
    );
    let r = sites(&pf, "replace", true);
    assert!(proven(&r), "{r:?}\n{}", pf.dump_text());
    let s = sites(&pf, "shift", true);
    assert!(proven(&s), "{s:?}\n{}", pf.dump_text());
}

#[test]
fn computed_define_through_top_keeps_locked_native_members() {
    let pf = facts(
        r#"
        const u: any = (globalThis as any)[process.argv[2]];
        Object.defineProperty(u, process.argv[3], { value: 1 });
        console.log("abc".replace("a", "b"), [1, 2].shift());
        "#,
    );
    let r = sites(&pf, "replace", true);
    assert!(proven(&r), "{r:?}\n{}", pf.dump_text());
    let s = sites(&pf, "shift", true);
    assert!(proven(&s), "{s:?}\n{}", pf.dump_text());
}

/// Control: a named add on `Array.prototype` is admitted at run time, so
/// an absent key of an array may read it.
#[test]
fn an_admitted_add_on_array_prototype_stays_open() {
    let pf = facts(
        r#"
        const u: any = process.argv.length > 9 ? Array.prototype : (globalThis as any)[process.argv[2]];
        Object.defineProperty(u, "extra", { value: 1 });
        console.log(([] as any).extra);
        "#,
    );
    let v = sites(&pf, "extra", false);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}

/// Control: a program class prototype is not locked; a define through ⊤
/// may redefine its method.
#[test]
fn a_program_prototype_redefined_through_top_stays_open() {
    let pf = facts(
        r#"
        class C { m() { return 1; } }
        const u: any = process.argv.length > 9 ? C.prototype : (globalThis as any)[process.argv[2]];
        (globalThis as any).leak = C;
        Object.defineProperty(u, "m", { value: () => 2 });
        console.log(new C().m());
        "#,
    );
    let v = sites(&pf, "m", true);
    assert!(open(&v), "{v:?}\n{}", pf.dump_text());
}
