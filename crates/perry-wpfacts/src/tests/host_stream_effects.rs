//! RSTREAM PR B (skin seed R6, three.js FileLoader): `new ReadableStream(..)`
//! runs the constructor's realm declaration (node 26,
//! lib/internal/webstreams/readablestream.js): start/pull are called with
//! this = the source and the stream's controller, cancel with the stream's
//! cancel reason, size with its chunks; the stream keeps its queue, stored
//! error and cancel reason, never the source. The controller, stream and
//! default reader members run theirs. Each S-test is red without the
//! declarations; the C-tests are the named controls.
use super::*;

fn esc(pf: &ProgramFacts, class: &str) -> bool {
    tmpl(pf, class).escape.is_some()
}

fn show(pf: &ProgramFacts, keys: &[&str], classes: &[&str]) {
    for k in keys {
        for s in pf.sites.iter().filter(|s| s.key == *k) {
            eprintln!(
                "RSTREAM site .{k} @{} top={} recv={:?} verdict={:?}",
                s.byte_offset, s.top, s.receivers, s.verdict
            );
        }
    }
    for c in classes {
        eprintln!("RSTREAM tmpl {c} escape={:?}", tmpl(pf, c).escape);
    }
}

fn controller_site(pf: &ProgramFacts, key: &str) -> bool {
    let s = site(pf, key);
    !s.top
        && !s.receivers.is_empty()
        && s
            .receivers
            .iter()
            .all(|r| r.contains("ReadableStreamDefaultController.prototype"))
}

fn no_unknown_ctor(pf: &ProgramFacts) -> bool {
    !pf.dump_text().contains("unknown constructor ReadableStream")
}

const S1: &str = r#"
    class C { x = 1; }
    const s = new ReadableStream({
        start(c: any) { c.enqueue(new C()); c.close(); },
    });
    s.getReader().read().then((r: any) => { if (!r.done) console.log(r.value.x); });
"#;

/// S1: the start callback is called with the stream's default controller;
/// what it enqueues is what the default reader's read() fulfills with.
#[test]
#[ignore = "HELD on rstream-a2: the byte-stream families replace `unknown` in the constructor's controller and getReader's reader unions (PROGRESS-B.md)"]
fn s1_start_controller_is_declared() {
    let pf = facts(&format!("{S1}\nexport const keep = 1;\n"));
    show(&pf, &["enqueue", "close", "x"], &["C"]);
    assert!(controller_site(&pf, "enqueue") && controller_site(&pf, "close"), "{}", pf.dump_text());
    let x = site(&pf, "x");
    assert!(!x.top && x.receivers.len() == 1 && x.receivers[0].contains('C'), "{:?}\n{}", x.receivers, pf.dump_text());
    assert!(!esc(&pf, "C") && no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// S2: three.js FileLoader's stream (three.core.js FileLoader.load), handed
/// back through a plain function instead of `new Response(..)`.
#[test]
#[ignore = "HELD on rstream-a2: the byte-stream families replace `unknown` in the constructor's controller and getReader's reader unions (PROGRESS-B.md)"]
fn s2_fileloader_shape() {
    let pf = facts(
        r#"
        class Ev { loaded = 0; }
        class Src { getReader() { return new ReadableStream({ start(c: any) { c.enqueue(new Uint8Array(1)); c.close(); } }).getReader(); } }
        function keepIt(s: any) { return s; }
        function load(body: Src, onProgress: (e: Ev) => void) {
            const reader = body.getReader();
            let loaded = 0;
            const stream = new ReadableStream({
                start(controller: any) {
                    readData();
                    function readData() {
                        reader.read().then(({ done, value }: any) => {
                            if (done) {
                                controller.close();
                            } else {
                                loaded += value.byteLength;
                                onProgress(new Ev());
                                controller.enqueue(value);
                                readData();
                            }
                        }, (e: any) => {
                            controller.error(e);
                        });
                    }
                }
            });
            return keepIt(stream);
        }
        console.log(load(new Src(), (e) => console.log(e.loaded)));
        export const keep = 1;
    "#,
    );
    show(&pf, &["close", "enqueue", "error", "loaded"], &["Ev", "Src"]);
    for k in ["close", "enqueue", "error"] {
        assert!(controller_site(&pf, k), "{k}\n{}", pf.dump_text());
    }
    assert!(!esc(&pf, "Ev") && no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// S3: the cancel callback is called with the reason the program cancels
/// the stream with.
#[test]
fn s3_cancel_reason_reaches_cancel() {
    let pf = facts(
        r#"
        class R { y = 2; }
        const s = new ReadableStream({ cancel(r: any) { console.log(r.y); } });
        s.cancel(new R());
        export const keep = 1;
    "#,
    );
    show(&pf, &["y"], &["R"]);
    let y = site(&pf, "y");
    assert!(!y.top && y.receivers.iter().any(|r| r.contains('R')), "{:?}\n{}", y.receivers, pf.dump_text());
    assert!(!esc(&pf, "R") && no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// S4: start's result is adopted as a promise (node's PromiseResolve): a
/// program thenable's then is called.
#[test]
fn s4_start_thenable() {
    let pf = facts(
        r#"
        class T { z = 3; }
        const t = new T();
        new ReadableStream({
            start(c: any) { return { then(res: any) { console.log(t.z); res(1); } }; },
        });
        export const keep = 1;
    "#,
    );
    show(&pf, &["z"], &["T"]);
    let z = site(&pf, "z");
    assert!(!z.top && z.receivers.iter().any(|r| r.contains('T')), "{:?}\n{}", z.receivers, pf.dump_text());
    assert!(!esc(&pf, "T") && no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// C1 (control): a stream handed to unknown code. Its queue is reachable
/// (the chunk class escapes), and unknown code may cancel it with any
/// reason, so the cancel callback's parameter is ⊤. The start callback and
/// its controller stay the program's. The callee is read off globalThis
/// first: a call with globalThis as its receiver would let unknown code
/// write the global constructor (c2).
#[test]
#[ignore = "HELD: c1, a false proof: unknown code can neither read nor cancel an escaped stream (PROGRESS-B.md); a separate design phase writes the rule"]
fn c1_stream_given_to_unknown_code() {
    let pf = facts(
        r#"
        class C { x = 1; }
        class R { y = 2; }
        const s = new ReadableStream({
            start(c: any) { c.enqueue(new C()); c.close(); },
            cancel(r: any) { console.log(r.y); },
        });
        s.cancel(new R());
        const sink = (globalThis as any).sink;
        sink(s);
        export const keep = 1;
    "#,
    );
    show(&pf, &["enqueue", "close", "y"], &["C", "R"]);
    assert!(esc(&pf, "C"), "the queue of an escaped stream is reachable\n{}", pf.dump_text());
    assert!(site(&pf, "y").top, "unknown code may cancel with any reason\n{}", pf.dump_text());
    assert!(controller_site(&pf, "enqueue") && controller_site(&pf, "close"), "{}", pf.dump_text());
}

/// C2 (control): a written global constructor is not the realm's.
#[test]
fn c2_written_global() {
    let pf = facts(
        r#"
        class X { constructor(o: any) {} }
        (globalThis as any).ReadableStream = X;
        const s = new ReadableStream({ start(c: any) { c.enqueue(1); } });
        export const keep = 1;
    "#,
    );
    assert!(pf.dump_text().contains("argument of written constructor"), "{}", pf.dump_text());
}

/// C3 (control): `type: "bytes"` makes a ReadableByteStreamController, so
/// the start callback's controller is never claimed to be only the
/// default controller.
#[test]
fn c3_bytes() {
    let pf = facts(
        r#"
        class C { x = 1; }
        new ReadableStream({
            type: "bytes",
            start(c: any) { c.enqueue(new C()); },
        });
        export const keep = 1;
    "#,
    );
    show(&pf, &["enqueue"], &["C"]);
    assert!(!controller_site(&pf, "enqueue"), "{}", pf.dump_text());
}
