//! A for-of element is what the iteration yields: an element of the array
//! below its length, or the value of a `next()` result that is not done.
//! The done result's `undefined`, the loop value's `undefined` initializer
//! and release, and the literal tail past the count never reach the loop
//! body. A destructuring reads the step's output after a done step too
//! (`js_iterator_step` publishes `undefined`), and keeps it.

use super::*;

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = verdicts(pf, key);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn open(pf: &ProgramFacts, key: &str) -> bool {
    let v = verdicts(pf, key);
    !v.is_empty() && v.iter().all(|v| *v != Verdict::NativeMember)
}

#[test]
fn a_literal_loop_element_is_its_strings() {
    let pf = facts(
        r#"
        let n = 0;
        for (const s of ["ab", "cd"]) n += s.trim().length;
        console.log(n);
        "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        verdicts(&pf, "trim"),
        pf.dump_text()
    );
}

#[test]
fn a_bound_and_a_pushed_array_loop_element_is_its_strings() {
    let pf = facts(
        r#"
        const xs = ["ab", "cd"];
        const ys: string[] = [];
        ys.push("ef");
        ys.push("gh");
        let n = 0;
        for (const s of xs) n += s.trim().length;
        for (const s of ys) n += s.trimEnd().length;
        console.log(n);
        "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        verdicts(&pf, "trim"),
        pf.dump_text()
    );
    assert!(
        native(&pf, "trimEnd"),
        "{:?}\n{}",
        verdicts(&pf, "trimEnd"),
        pf.dump_text()
    );
}

/// Soundness: a program iterator that yields `undefined` (a result that is
/// not done) keeps it.
#[test]
fn a_program_iterator_yielding_undefined_keeps_it() {
    let pf = facts(
        r#"
        const it = {
          i: 0,
          [Symbol.iterator]() { return this; },
          next() {
            this.i++;
            if (this.i > 2) return { done: true, value: undefined };
            return { done: false, value: this.i === 1 ? "ab" : undefined };
          },
        };
        let n = 0;
        try { for (const s of it as any) n += s.trim().length; } catch (e) { n = -1; }
        console.log(n);
        "#,
    );
    assert!(
        open(&pf, "trim"),
        "{:?}\n{}",
        verdicts(&pf, "trim"),
        pf.dump_text()
    );
}

/// Soundness: an index store past the end leaves a hole the loop reads.
#[test]
fn a_holey_array_loop_element_keeps_undefined() {
    let pf = facts(
        r#"
        const a = ["ab"];
        a[3] = "cd";
        let n = 0;
        try { for (const s of a) n += s.trim().length; } catch (e) { n = -1; }
        console.log(n);
        "#,
    );
    assert!(
        open(&pf, "trim"),
        "{:?}\n{}",
        verdicts(&pf, "trim"),
        pf.dump_text()
    );
}

/// Soundness: the counted walk of a bound array (`for_of_guard`'s source
/// walk) yields every pushed value, a non-string one included.
#[test]
fn a_walked_array_element_keeps_a_pushed_non_string() {
    let pf = facts(
        r#"
        const ys: any[] = [];
        ys.push("ef");
        ys.push(7);
        let n = 0;
        try { for (const s of ys) n += s.trimEnd().length; } catch (e) { n = -1; }
        console.log(n);
        "#,
    );
    assert!(
        open(&pf, "trimEnd"),
        "{:?}\n{}",
        verdicts(&pf, "trimEnd"),
        pf.dump_text()
    );
}

/// Soundness: a hand-written loop that only looks like the walk (its
/// counter named like the temp, a bound past the length, or a counter the
/// body moves) reads past the end and keeps `undefined`.
#[test]
fn a_loop_shaped_like_the_walk_keeps_undefined() {
    for src in [
        r#"
        const ys = ["ab", "cd"];
        let n = 0;
        try { for (let __forof_lit_k_0 = 0; __forof_lit_k_0 < ys.length + 1; __forof_lit_k_0++) { const s = ys[__forof_lit_k_0]; n += s.trim().length; } } catch (e) { n = -1; }
        console.log(n);
        "#,
        r#"
        const ys = ["ab", "cd"];
        let n = 0;
        try { for (let __forof_lit_k_0 = 0; __forof_lit_k_0 < ys.length; __forof_lit_k_0++) { __forof_lit_k_0 += 5; const s = ys[__forof_lit_k_0]; n += s.trim().length; } } catch (e) { n = -1; }
        console.log(n);
        "#,
        r#"
        const ys: any = { length: 2 };
        let n = 0;
        try { for (let __forof_lit_k_0 = 0; __forof_lit_k_0 < ys.length; __forof_lit_k_0++) { const s = ys[__forof_lit_k_0]; n += s.trim().length; } } catch (e) { n = -1; }
        console.log(n);
        "#,
    ] {
        let pf = facts(src);
        assert!(
            open(&pf, "trim"),
            "{:?}\n{}",
            verdicts(&pf, "trim"),
            pf.dump_text()
        );
    }
}

/// PR-C1 (`Cons::ElemUndef`): a counter the body moves only after the read
/// is still tested against the length before every read, so the read is an
/// element: the key interval `[0, 1]` lies below the length 2.
#[test]
fn a_counter_moved_after_the_read_reads_an_element() {
    let pf = facts(
        r#"
        const ys = ["ab", "cd"];
        let n = 0;
        try { for (let __forof_lit_k_0 = 0; __forof_lit_k_0 < ys.length; __forof_lit_k_0++) { const s = ys[__forof_lit_k_0]; __forof_lit_k_0 += 5; n += s.trim().length; } } catch (e) { n = -1; }
        console.log(n);
        "#,
    );
    assert!(native(&pf, "trim"), "{:?}", verdicts(&pf, "trim"));
}

/// Soundness: a destructuring past the yielded values reads `undefined`.
#[test]
fn a_destructuring_past_the_end_keeps_undefined() {
    let pf = facts(
        r#"
        const [p, q] = new Set(["ab"]);
        let n = p.length;
        try { n += (q as any).trim().length; } catch (e) { n = -1; }
        console.log(n);
        "#,
    );
    assert!(
        open(&pf, "trim"),
        "{:?}\n{}",
        verdicts(&pf, "trim"),
        pf.dump_text()
    );
}

/// A for-of record over an array literal steps the literal through its
/// payload, which stands for the iterator GetIterator returns (the returned
/// Array Iterator is not a value of the loop): the loop value is the
/// literal's strings only, never the done step's `undefined`, at module
/// level as in a function (c08 elements.ts `for (const p of ["name", ...])
/// reflectString(Cls, p)`). `p ?? new Svc()` is a Svc only for `undefined`.
fn literal_loop(body: &str) -> Vec<String> {
    let pf = facts(&format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any): number {{ return a.size; }}
        {body}
        "#
    ));
    pf.sites
        .iter()
        .filter(|s| s.key == "size" && !s.is_call)
        .flat_map(|s| s.receivers.iter().cloned())
        .collect()
}

#[test]
fn a_module_level_literal_loop_value_is_no_done_value() {
    let r = literal_loop(r#"for (const p of ["a", "b"]) console.log(g(p ?? new Svc()));"#);
    assert!(!r.iter().any(|r| r.contains("Svc")), "{r:?}");
}

#[test]
fn a_function_literal_loop_value_is_no_done_value() {
    let r = literal_loop(
        r#"function init(): void { for (const p of ["a", "b"]) console.log(g(p ?? new Svc())); } init();"#,
    );
    assert!(!r.iter().any(|r| r.contains("Svc")), "{r:?}");
}
