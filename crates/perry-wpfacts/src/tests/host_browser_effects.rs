//! BROWSER seed group (skin / three.js FileLoader + ImageBitmapLoader):
//! the web host members fetch and AbortSignal run their realm
//! declarations, as the node host modules do. Each B-test is red without
//! them; the K-test is the named control.
use super::*;

fn esc(pf: &ProgramFacts, class: &str) -> bool {
    tmpl(pf, class).escape.is_some()
}

fn show(pf: &ProgramFacts, keys: &[&str], classes: &[&str]) {
    for k in keys {
        for s in pf.sites.iter().filter(|s| s.key == *k) {
            eprintln!(
                "SKINBR site .{k} @{} top={} recv={:?} verdict={:?}",
                s.byte_offset, s.top, s.receivers, s.verdict
            );
        }
    }
    for c in classes {
        eprintln!("SKINBR tmpl {c} escape={:?}", tmpl(pf, c).escape);
    }
}

/// B1 (main: `argument of builtin Other.fetch` sinks the init object and
/// `builtins.rs:296 result: argument of builtin Other.fetch`; the
/// ImageBitmapLoader shape): fetch reads `init.headers` (HeadersInit: own
/// keys + ToString of each value) and keeps nothing; its result is a host
/// promise of a fresh Response whose `blob()` is a host promise of a fresh
/// Blob. The program header object H stays unescaped.
#[test]
fn b1_fetch_reads_init_and_returns_a_host_response() {
    let pf = facts(
        r#"
        class H { a = "b"; }
        class Loader {
            requestHeader = new H();
            crossOrigin = "anonymous";
            load(url: string, onLoad: (n: number) => void) {
                const fetchOptions: any = {};
                fetchOptions.credentials = (this.crossOrigin === "anonymous") ? "same-origin" : "include";
                fetchOptions.headers = this.requestHeader;
                fetch(url, fetchOptions)
                    .then(function (res) { return res.blob(); })
                    .then(function (blob) { onLoad(blob.size); });
                return this.requestHeader.a;
            }
        }
        console.log(new Loader().load(process.argv[2], (n) => console.log(n)));
        export const keep = 1;
    "#,
    );
    show(&pf, &["a", "blob", "size", "then"], &["H"]);
    assert!(!esc(&pf, "H"), "{}", pf.dump_text());
}

/// B3 (main: `unresolved class value AbortSignal` in a class method): the
/// realm global constructor `AbortSignal` is its realm value (main:
/// builtin-fn:AbortSignal AND ⊤), and `AbortSignal.any` iterates its
/// argument and returns a fresh signal (the program array and its holder
/// stay unescaped).
#[test]
fn b3_abort_signal_any_is_a_realm_value() {
    let pf = facts(
        r#"
        class Mgr { abortController = new AbortController(); list: any[] = []; }
        class Loader {
            _abortController = new AbortController();
            manager = new Mgr();
            signal() {
                const l = this.manager.list;
                l.push(this._abortController.signal, this.manager.abortController.signal);
                const s = (typeof AbortSignal.any === "function") ? AbortSignal.any(l) : this._abortController.signal;
                return [s, l.length];
            }
        }
        console.log(new Loader().signal());
        export const keep = 1;
    "#,
    );
    show(&pf, &["any", "list", "length"], &["Mgr", "Loader"]);
    let s = site(&pf, "any");
    assert!(
        !s.top && !s.receivers.is_empty(),
        "any {:?}\n{}",
        s.receivers,
        pf.dump_text()
    );
    assert!(!esc(&pf, "Mgr"), "{}", pf.dump_text());
}

/// K2 (control, must stay modelled): the abort reason the program gives
/// abort(reason) is the rejection value of a fetch on that signal (node)
/// and the reason of the signal: the rejection handler read sees R, which
/// no opaque call holds (red without abort's store of its reason).
#[test]
fn k2_abort_reason_is_the_fetch_rejection() {
    let pf = facts(
        r#"
        class R { x = 1; }
        function go(url: string) {
            const ac = new AbortController();
            const p = fetch(url, { signal: ac.signal }).catch((e: any) => console.log(e.x));
            ac.abort(new R());
            return p;
        }
        go(process.argv[2]);
        export const keep = 1;
    "#,
    );
    show(&pf, &["x"], &["R"]);
    let x = site(&pf, "x");
    assert!(
        x.receivers.iter().any(|r| r.ends_with(":R")),
        "{:?}\n{}",
        x.receivers,
        pf.dump_text()
    );
    assert!(!esc(&pf, "R"), "{}", pf.dump_text());
}

/// K3 (control): `abort()` calls the signal's `onabort` with an Event
/// (node): the handler's parameter is the Event the host makes, never
/// proven to be a program value it is not (here the program object E the
/// handler is also called with directly). `s.onabort = f` runs the declared
/// setter: f is kept in the signal's handler slot, which the declared
/// abort calls; f does not escape, and neither does the reason R (red
/// without abort's call of the handler).
#[test]
fn k3_onabort_is_called_with_an_event() {
    let pf = facts(
        r#"
        class E { type = "x"; }
        class R { x = 1; }
        function f(ev: any) { console.log(ev.type); }
        function go() {
            const c = new AbortController();
            const s = c.signal;
            s.onabort = f;
            f(new E());
            c.abort(new R());
            return s;
        }
        go();
        export const keep = 1;
    "#,
    );
    show(&pf, &["type"], &["E", "R"]);
    let t = site(&pf, "type");
    assert!(
        t.receivers.iter().any(|r| r.contains("Event.prototype")),
        "{:?}\n{}",
        t.receivers,
        pf.dump_text()
    );
    assert!(!esc(&pf, "R") && !esc(&pf, "E"), "{}", pf.dump_text());
}

/// G1: a load of a key whose realm declaration is a getter reads the
/// getter's declared result. `AbortSignal.prototype.reason`'s getter
/// returns the reason slot or a fresh DOMException (node: `abort()` with no
/// reason); the `.y` receivers are those values, never the getter function
/// (red while the load reads `builtin-fn`).
#[test]
fn g1_reason_getter_reads_the_declared_result() {
    let pf = facts(
        r#"
        class R { y = 2; }
        const c = new AbortController();
        c.abort(new R());
        const s = c.signal;
        const r: any = s.reason;
        console.log(r.y);
        export const keep = 1;
    "#,
    );
    show(&pf, &["y", "reason"], &["R"]);
    let y = site(&pf, "y");
    assert!(
        y.receivers.iter().any(|r| r.ends_with(":R"))
            && y.receivers
                .iter()
                .any(|r| r.contains("DOMException.prototype"))
            && !y.receivers.iter().any(|r| r.contains("builtin-fn")),
        "{:?}\n{}",
        y.receivers,
        pf.dump_text()
    );
}

/// G2 (the false proof): the program writes a string into R.y through the
/// reason getter's result, so R.y is written after construction and `q.y`
/// is not proven a number (red while the getter's result is its function:
/// R.y readonly, kind num, `q.y` T0).
#[test]
fn g2_write_through_the_reason_getter_reaches_r() {
    let pf = facts(
        r#"
        class R { y = 2; }
        const c = new AbortController();
        const q = new R();
        c.abort(q);
        const r: any = c.signal.reason;
        r.y = "s";
        console.log(q.y + 1);
        export const keep = 1;
    "#,
    );
    show(&pf, &["y"], &["R"]);
    let y = field(tmpl(&pf, "R"), "y");
    eprintln!(
        "SKINBR R.y readonly={} writers_post={} kind={:?}",
        y.readonly, y.writers_post, y.kind
    );
    assert!(!y.readonly && y.writers_post > 0, "{}", pf.dump_text());
    assert!(
        y.kind.prims & crate::model::prim::STR != 0,
        "{:?}\n{}",
        y.kind,
        pf.dump_text()
    );
}

/// Q1: an accessor the realm declares with a getter the model does not
/// (`Response.prototype.body`) reads the getter's unknown result (⊤ and
/// any primitive), never a builtin function (red while it reads
/// `builtin-fn`: a value the program gave the Response, its body stream,
/// would be missing).
#[test]
fn q1_undeclared_accessor_reads_its_result_not_a_function() {
    let pf = facts(
        r#"
        function go(url: string) {
            return fetch(url).then((r: any) => {
                const b: any = r.body;
                console.log(b.locked);
            });
        }
        go(process.argv[2]);
        export const keep = 1;
    "#,
    );
    show(&pf, &["locked", "body"], &[]);
    let l = site(&pf, "locked");
    assert!(
        l.top && !l.receivers.iter().any(|r| r.contains("builtin-fn")),
        "{} {:?}\n{}",
        l.top,
        l.receivers,
        pf.dump_text()
    );
}

/// B4 (skin: `Headers coerces its elems(arg(0))` read every key of the init
/// through its chain, so Object.prototype's and String.prototype's methods
/// were coerced): Fetch "fill a Headers object" converts a HeadersInit as
/// WebIDL `sequence<sequence<ByteString>>` (iterate the outer, iterate each
/// pair, ToString each item) or `record<ByteString, ByteString>` (own
/// enumerable keys, ToString each value). The record's inherited method
/// and the pair item objects stay unescaped; only their `toString` runs.
#[test]
fn b4_headers_init_reads_pairs_and_own_values_only() {
    let pf = facts(
        r#"
        class V { s = "v"; toString() { return this.s; } }
        class R { a = new V(); m() { return this.a; } }
        const r = new R();
        const pairs = [["x-a", new V()]];
        const h1 = new Headers(r as any);
        const h2 = new Headers(pairs as any);
        console.log(h1.get("a"), h2.get("x-a"), r.m().s);
        export const keep = 1;
    "#,
    );
    show(&pf, &["s", "a", "get"], &["R", "V"]);
    assert!(!esc(&pf, "R"), "{}", pf.dump_text());
    assert!(!esc(&pf, "V"), "{}", pf.dump_text());
}
