//! node:fs watchers and sync members whose effects the runtime declares at
//! their bodies (batch W): `fs.watch` keeps its listener on the watcher it
//! returns and runs every listener the watcher holds later with the watcher
//! as `this` ('change': two strings or a Buffer, 'error': an fs Error,
//! 'close': none); the watcher's own closures (`on`, `close`, ...) act on
//! the watcher they capture (`islot(callee, self)`), never on `this`.
//! `fs.watchFile` runs its listeners with two Stats objects; a second call
//! on a watched path returns the watcher an earlier call made. On main
//! these members were undeclared: every argument was handed to unknown
//! code, so a listener escaped and its parameters and `this` were ⊤.
use super::*;

/// The receivers of `.key` are exactly the one watcher object a call made.
fn on_the_watcher(pf: &ProgramFacts, key: &str) -> bool {
    let s = site(pf, key);
    !s.top && s.receivers.len() == 1 && s.receivers[0].starts_with("obj:hostrecord@")
}

/// The listener's parameters and `this`, a listener `on` adds, and the
/// watcher's own members; a program object the listener stores a
/// parameter into keeps its facts.
#[test]
fn watch_listener_params_this_and_members_are_proven() {
    let pf = facts(
        r#"
        class O { k: any = 0; }
        const o = new O();
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const w: any = _fs.watch(p, { persistent: false } as any, function (this: any, ev: any, name: any) {
                o.k = name;
                console.log(ev.aa, name.bb, this.cc);
            });
            w.on("error", function (this: any, e: any) { console.log(e.dd, this.ee); });
            const r: any = w.unref();
            console.log(r.ff, w.gg, o.k.mm);
            w.close();
        }
        sys(process.argv[1]);
    "#,
    );
    for key in ["aa", "bb", "dd", "mm"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
    for key in ["cc", "ee", "ff", "gg"] {
        assert!(on_the_watcher(&pf, key), "{key}\n{}", pf.dump_text());
    }
}

/// `fs.watch(path, listener)`: the listener is the second argument.
#[test]
fn watch_two_argument_listener_is_proven() {
    let pf = facts(
        r#"
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            _fs.watch(p, function (this: any, ev: any) { console.log(ev.nn, this.oo); });
        }
        sys(process.argv[1]);
    "#,
    );
    assert!(!site(&pf, "nn").top, "{}", pf.dump_text());
    assert!(on_the_watcher(&pf, "oo"), "{}", pf.dump_text());
}

/// The watchFile listener gets two Stats objects (a Date `mtime`) and the
/// StatWatcher as `this`; `unwatchFile` never calls the listener it is
/// given (its body never runs: no site of it is made).
#[test]
fn watch_file_listener_stats_and_this_are_proven() {
    let pf = facts(
        r#"
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            _fs.watchFile(p, { persistent: true, interval: 250 } as any, function (this: any, curr: any, prev: any) {
                console.log(curr.mtime.getTime(), +prev.mtime, curr.size, this.kk);
            });
            _fs.unwatchFile(p, function (a: any) { console.log(a.ll); });
        }
        sys(process.argv[1]);
    "#,
    );
    for key in ["mtime", "getTime", "size"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
    assert!(on_the_watcher(&pf, "kk"), "{}", pf.dump_text());
    assert!(
        pf.sites.iter().filter(|s| s.key == "ll").all(|s| !s.top),
        "{}",
        pf.dump_text()
    );
}

/// A second `watchFile` on a watched path returns the watcher the first
/// call made (the runtime keeps one per path): what the program stored on
/// it is read back through the second result.
#[test]
fn watch_file_again_returns_the_earlier_watcher() {
    let pf = facts(
        r#"
        class Q { qv = 1; }
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const a: any = _fs.watchFile(p, (c: any) => {});
            a.x = new Q();
            const b: any = _fs.watchFile(p, (c: any) => {});
            console.log(b.x.qq);
        }
        sys(process.argv[1]);
    "#,
    );
    let s = site(&pf, "qq");
    assert!(
        !s.top && s.receivers.iter().any(|r| r.ends_with(":Q")),
        "{}",
        pf.dump_text()
    );
}

/// The sync members: openSync's fd, writeSync's byte count, realpathSync's
/// string (or Buffer), utimesSync's undefined; an object given to
/// utimesSync as a time is read as it is: its methods are not called back.
#[test]
fn sync_members_results_are_known_and_arguments_kept_by_no_one() {
    let pf = facts(
        r#"
        class Q { y = 1; }
        function g(v: any) { return v.q1; }
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const fd: any = _fs.openSync(p, "r");
            const n: any = _fs.writeSync(fd, "x", { position: 0 } as any);
            const r: any = _fs.realpathSync(p, { encoding: "utf8" } as any);
            const t: any = { m(a: any) { return a.y; } };
            const u: any = _fs.utimesSync(p, t, t);
            console.log(g(fd), g(n), r.length, g(u));
            console.log(t.m(new Q()));
        }
        sys(process.argv[1]);
    "#,
    );
    for key in ["q1", "length"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
    let y = site(&pf, "y");
    assert!(
        !y.top && y.receivers.iter().all(|r| r.ends_with(":Q")),
        "{}",
        pf.dump_text()
    );
}

/// Named control: writeSync [[Get]]s its options' `position` (an accessor
/// runs, as node's destructuring runs it): the store it makes is seen.
#[test]
fn write_sync_options_getter_still_runs() {
    let pf = facts(
        r#"
        class P { x: any; constructor(x: number) { this.x = x; } }
        const p = new P(1);
        function sys() {
            const _fs: typeof import("fs") = require("fs");
            return _fs.writeSync(1, "x", { get position() { p.x = "s"; return null; } } as any);
        }
        console.log(sys());
        console.log(p.x);
    "#,
    );
    assert!(!field(tmpl(&pf, "P"), "x").readonly, "{}", pf.dump_text());
}

/// Named control: the listener runs (the declared call): what it stores
/// into a global that unknown code reads escapes.
#[test]
fn a_listener_storing_into_a_global_unknown_code_reads_escapes_it() {
    let pf = facts(
        r#"
        class P { x: any; constructor(x: number) { this.x = x; } }
        const p = new P(1);
        function sys(path: string) {
            const _fs: typeof import("fs") = require("fs");
            _fs.watch(path, { persistent: false } as any, (ev: any) => { (globalThis as any).leak = p; });
            (globalThis as any).structuredClone(globalThis);
        }
        sys(process.argv[1]);
        console.log(p.x);
    "#,
    );
    assert!(!field(tmpl(&pf, "P"), "x").readonly, "{}", pf.dump_text());
}

/// Named control: `watch` called through an alias of fs that is ⊤ (what an
/// unmodeled native returned) is an unknown call: its listener escapes.
#[test]
fn watch_through_an_unknown_alias_of_fs_stays_unknown() {
    let pf = facts(
        r#"
        function sys(p: string) {
            const f: any = (globalThis as any).structuredClone(require("fs"));
            f.watch(p, {} as any, function (ev: any) { console.log(ev.tt); });
        }
        sys(process.argv[1]);
    "#,
    );
    assert!(site(&pf, "tt").top, "{}", pf.dump_text());
}

/// Named control: a watcher member taken off the watcher and called on
/// another receiver acts on its own watcher, never on the receiver (the
/// runtime's closure reads its captured watcher, not `this`): the
/// listener's `this` and the result are the watcher, never `other`.
#[test]
fn a_watcher_method_called_on_another_receiver_acts_on_its_watcher() {
    let pf = facts(
        r#"
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const w: any = _fs.watch(p);
            const on = w.on;
            const other: any = { hh: 1 };
            const r: any = on.call(other, "change", function (this: any) { console.log(this.ii); });
            other.on = on;
            const r2: any = other.on("error", function (this: any) { console.log(this.i2); });
            console.log(r.jj, r2.j2, other.hh);
        }
        sys(process.argv[1]);
    "#,
    );
    for key in ["ii", "i2", "jj", "j2"] {
        assert!(on_the_watcher(&pf, key), "{key}\n{}", pf.dump_text());
    }
}
