//! `ArrayBuffer.prototype`, `SharedArrayBuffer.prototype` and
//! `DataView.prototype` have no `@@iterator` (nor does `Object.prototype`):
//! `for (const b of buffer)` reads `undefined` and GetIterator throws a
//! fresh TypeError. No method runs, so neither the buffer nor what the loop
//! binds reaches unknown code. Named control: once a program writes
//! `@@iterator` onto the buffer's prototype, the iteration runs what was
//! written, as an unknown method.
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
fn a_written_buffer_iterator_runs_as_unknown_code() {
    let pf = iterated(
        "(ArrayBuffer.prototype as any)[Symbol.iterator] = function () { return [][Symbol.iterator](); };",
        "new ArrayBuffer(4)",
    );
    let y = site(&pf, "y");
    assert!(y.top, "{}", pf.dump_text());
}
