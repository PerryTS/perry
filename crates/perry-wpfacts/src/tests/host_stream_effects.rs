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
            .all(|r| {
                r.contains("ReadableStreamDefaultController.prototype")
                    || r.contains("ReadableByteStreamController.prototype")
            })
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
/// what it enqueues is what the default reader's read() fulfills with. The
/// stream may be a byte stream (no value-conditional form), whose queue also
/// holds the Uint8Array remainders the runtime makes, and getReader() may
/// be a BYOB reader whose read fulfills with a view of the caller's class
/// (any typed array, DataView or Buffer): `x` sees C and those views,
/// nothing else.
#[test]
fn s1_start_controller_is_declared() {
    let pf = facts(&format!("{S1}\nexport const keep = 1;\n"));
    show(&pf, &["enqueue", "close", "x"], &["C"]);
    assert!(controller_site(&pf, "enqueue") && controller_site(&pf, "close"), "{}", pf.dump_text());
    let x = site(&pf, "x");
    assert!(
        !x.top
            && x.receivers.iter().any(|r| r.contains('C'))
            && x.receivers.iter().all(|r| r.contains('C')
                    || r.contains("Array.prototype")
                    || r.contains("DataView.prototype")
                    || r.contains("Buffer.prototype")),
        "{:?}\n{}",
        x.receivers,
        pf.dump_text()
    );
    assert!(!esc(&pf, "C") && no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// S2: three.js FileLoader's stream (three.core.js FileLoader.load), handed
/// back through a plain function instead of `new Response(..)`.
#[test]
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

const C1: &str = r#"
    class C { x = 1; }
    class R { y = 2; }
    const s = new ReadableStream({
        start(c: any) { c.enqueue(new C()); c.close(); },
        cancel(r: any) { console.log(r.y); },
    });
    s.cancel(new R());
"#;

/// C1 (control): a stream handed to unknown code (`new Function`): that code
/// may read the queue and cancel with any reason, so C escapes and `r.y` is
/// not proven; the controller the constructor makes stays the program's.
#[test]
fn c1_stream_given_to_unknown_code() {
    let pf = facts(&format!("{C1}\nnew Function(\"v\", process.argv[2])(s);\nexport const keep = 1;\n"));
    show(&pf, &["enqueue", "close", "y"], &["C", "R"]);
    assert!(esc(&pf, "C"), "the queue of an escaped stream is reachable\n{}", pf.dump_text());
    assert!(site(&pf, "y").top, "unknown code may cancel with any reason\n{}", pf.dump_text());
    assert!(controller_site(&pf, "enqueue") && controller_site(&pf, "close"), "{}", pf.dump_text());
}

/// C1 closed: `globalThis.sink` is never written in a closed program, so
/// `f(s)` has no callee (it throws) and the stream reaches no other code:
/// the only cancel reason is R.
#[test]
fn c1_closed_world_sink() {
    let pf = facts(&format!("{C1}\nconst f = (globalThis as any).sink;\nf(s);\nexport const keep = 1;\n"));
    show(&pf, &["y"], &["C", "R"]);
    let y = site(&pf, "y");
    assert!(
        !y.top && !y.receivers.is_empty() && y.receivers.iter().all(|r| r.contains('R')),
        "{:?}\n{}",
        y.receivers,
        pf.dump_text()
    );
    assert!(!esc(&pf, "R") && no_unknown_ctor(&pf), "{}", pf.dump_text());
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

const S5: &str = r#"
    const s = new ReadableStream({
        type: "bytes",
        start(c: any) { c.enqueue(new Uint8Array(4)); },
        pull(c: any) {
            const q = c.byobRequest;
            if (q) { console.log(q.view.byteLength); q.respond(0); }
        },
    });
    const r = s.getReader({ mode: "byob" });
"#;
const S5_READ: &str = "r.read(new Uint16Array(2)).then((res: any) => { if (!res.done) console.log(res.value.length); });\nexport const keep = 1;\n";

/// S3b (bytes): a byte stream's controller, BYOB request and BYOB reader run
/// their declarations: byobRequest.view is the pending read's view, and
/// read(view) fulfills with a typed array (or undefined).
#[test]
fn s3b_bytes() {
    let pf = facts(&format!("{S5}\n{S5_READ}"));
    show(&pf, &["enqueue", "respond", "byteLength", "length"], &[]);
    assert!(controller_site(&pf, "enqueue"), "{}", pf.dump_text());
    let q = site(&pf, "respond");
    assert!(
        !q.top && q.receivers.iter().any(|r| r.contains("ReadableStreamBYOBRequest.prototype")),
        "{:?}\n{}",
        q.receivers,
        pf.dump_text()
    );
    for k in ["byteLength", "length"] {
        let v = site(&pf, k);
        assert!(
            !v.top && !v.receivers.is_empty(),
            "{k} {:?}\n{}",
            v.receivers,
            pf.dump_text()
        );
    }
    assert!(no_unknown_ctor(&pf), "{}", pf.dump_text());
}

/// C3b (control): a view unknown code made, given to respondWithNewView, is
/// what the pending BYOB read fulfills with, so `res.value.length` is not
/// proven.
#[test]
fn c3b_bytes_unknown_view() {
    let src = S5.replace("q.respond(0);", "q.respondWithNewView(new Function(process.argv[2])());");
    assert_ne!(src, S5);
    let pf = facts(&format!("{src}\n{S5_READ}"));
    show(&pf, &["length"], &[]);
    assert!(site(&pf, "length").top, "{}", pf.dump_text());
}

/// C3 (control): `type: "bytes"` makes a ReadableByteStreamController: the
/// start callback's controller is never claimed to be only the default one.
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
    let e = site(&pf, "enqueue");
    assert!(controller_site(&pf, "enqueue"), "{}", pf.dump_text());
    assert!(
        e.receivers.iter().any(|r| r.contains("ReadableByteStreamController.prototype")),
        "{:?}
{}",
        e.receivers,
        pf.dump_text()
    );
}
