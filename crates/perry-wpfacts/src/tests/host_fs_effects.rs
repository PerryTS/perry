//! fs members whose effects the runtime declares at the body their dispatch
//! arm calls (HOST slice F): `statSync` / `lstatSync` return the Stats object
//! `build_stats_object` makes (own predicate closures `%FsStatsPredicate%`
//! that return a boolean, own numbers, Date values), `readdirSync` a fresh
//! array of strings (or Buffers, or Dirent objects of the same predicates).
use super::*;
use crate::model::prim;

/// Main: the call result was ⊤ (an opaque native), so every read on it and
/// every value out of it was unknown.
#[test]
fn stat_result_predicates_and_mtime_are_proven() {
    let pf = facts(
        r#"
        function g(v: any) { return v.q; }
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const st: any = _fs.statSync(p, { throwIfNoEntry: false } as any);
            const f = st.isFile();
            const d = st.isDirectory();
            const t = st.mtime;
            return [g(f), g(d), +t, st.size];
        }
        console.log(sys(process.argv[1]));
    "#,
    );
    for key in ["isFile", "isDirectory", "mtime", "size", "q"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
}

/// node's `statSync` / `lstatSync` return `undefined` for a missing entry
/// when the options' `throwIfNoEntry` is false: the result is a Stats
/// object or `undefined`, never proven to be an object only.
#[test]
fn stat_result_may_be_undefined_without_throw() {
    let pf = facts(
        r#"
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const st: any = _fs.statSync(p, { throwIfNoEntry: false } as any);
            const ls: any = _fs.lstatSync(p, { throwIfNoEntry: false } as any);
            return [st.size, ls.mode];
        }
        console.log(sys(process.argv[1]));
    "#,
    );
    for key in ["size", "mode"] {
        let s = site(&pf, key);
        assert!(
            !s.top && s.receiver_prims & prim::UNDEF != 0,
            "{key}\n{}",
            pf.dump_text()
        );
    }
}

/// `readdirSync(p)`: its elements are strings; with `withFileTypes` Dirent
/// objects whose `name` is a string and whose predicates return booleans.
#[test]
fn readdir_elements_are_strings_and_dirents() {
    let pf = facts(
        r#"
        function h(v: any) { return v.length; }
        function sys(p: string) {
            const _fs: typeof import("fs") = require("fs");
            const names: any = _fs.readdirSync(p);
            let n = 0;
            for (const s of names) n += h(s);
            const ents: any = _fs.readdirSync(p, { withFileTypes: true } as any);
            for (const e of ents) { if (e.isDirectory()) n += 1; if (e.name) n += 1; }
            return n;
        }
        console.log(sys(process.argv[1]));
    "#,
    );
    // One declaration covers every options form, so an element may also be
    // a Buffer (`encoding: "buffer"`): `name` is read on Buffer elements too
    // and what a Buffer holds is not modeled, so the value of `e.name` is not
    // checked here, only that the reads on the elements are known.
    for key in ["length", "isDirectory", "name"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
}

/// Named control: an options object's getter still runs, as node runs it
/// (`statSync` [[Get]]s `bigint`): the store it makes is seen.
#[test]
fn stat_options_getter_still_runs() {
    let pf = facts(
        r#"
        class P { x: any; constructor(x: number) { this.x = x; } }
        const p = new P(1);
        function sys() {
            const _fs: typeof import("fs") = require("fs");
            return _fs.statSync("/t", { get bigint() { p.x = "s"; return false; } } as any);
        }
        console.log(sys());
        console.log(p.x);
    "#,
    );
    let x = field(tmpl(&pf, "P"), "x");
    assert!(!x.readonly, "{}", pf.dump_text());
}
