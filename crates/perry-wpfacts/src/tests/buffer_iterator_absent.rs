//! `ArrayBuffer.prototype`, `SharedArrayBuffer.prototype` and
//! `DataView.prototype` have no `@@iterator` (nor does `Object.prototype`):
//! `for (const b of buffer)` reads `undefined` and GetIterator throws a
//! fresh TypeError. No method runs, so neither the buffer nor what the loop
//! binds reaches unknown code. Named control: once a program writes
//! `@@iterator` onto the buffer's prototype, the iteration runs what was
//! written: the program's own function (the runtime applies the store: the
//! key is absent there, and adding a member to `ArrayBuffer.prototype` is
//! not locked, DEC 250).
use super::*;

const KINDS: [&str; 3] = [
    "new ArrayBuffer(4)",
    "new SharedArrayBuffer(4)",
    "new DataView(new ArrayBuffer(4))",
];

fn iterated(prelude: &str, buffer: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        {prelude}
        class Q {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        try {{ for (const b of ({buffer}) as any) {{ console.log(o.m(b)); }} }} catch (e) {{}}
        console.log(o.m(new Q()));
    "#
    ))
}

#[test]
fn iterating_a_buffer_runs_no_method() {
    for buffer in KINDS {
        let pf = iterated("", buffer);
        let y = site(&pf, "y");
        assert!(
            !y.top && y.receivers.iter().all(|r| r.ends_with(":Q")),
            "{buffer}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn a_written_buffer_iterator_runs_the_programs_function() {
    // owner 11-24: E20 hold dropped; a program store onto a builtin prototype is a member reads see (PSTORE): the written @@iterator is the program's function.
    let pf = iterated(
        "class R { y = 2; }\n(ArrayBuffer.prototype as any)[Symbol.iterator] = function () { return [new R()][Symbol.iterator](); };",
        "new ArrayBuffer(4)",
    );
    let y = site(&pf, "y");
    // The written function's body runs: the loop binds what its iterator
    // yields (an R), beside the direct call's Q, and nothing unknown.
    assert!(
        !y.top
            && y.receivers.iter().any(|r| r.ends_with(":R"))
            && y.receivers.iter().any(|r| r.ends_with(":Q"))
            && y.receivers
                .iter()
                .all(|r| r.ends_with(":R") || r.ends_with(":Q")),
        "{}",
        pf.dump_text()
    );
}
