//! The file contents `fs.readFileSync(path)` returns are a Buffer (HOST
//! slice F part 2): its `toString` is the method call on the buffer
//! (`perry_hir::folded_call::fs_receiver_dispatch`), whose effects the
//! runtime declares at `Buffer.prototype.toString`: each argument goes
//! through ToPrimitive (node's `encoding += ""`, the start/end compares),
//! and the result is a string.
use super::*;
use crate::model::prim;

/// Main: the result of `buffer.toString(..)` was ⊤ (an opaque native), so
/// every read on it was unknown (TypeScript `sys.ts` `readFile`).
#[test]
fn read_file_text_is_a_string() {
    let pf = facts(
        r#"
        function h1(v: any) { return v.q1; }
        function h2(v: any) { return v.q2; }
        function readFile(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: Buffer;
            try { buffer = _fs.readFileSync(f); } catch { return undefined; }
            let len = buffer.length;
            if (len >= 2 && buffer[0] === 0xfe && buffer[1] === 0xff) {
                return h1(buffer.toString("utf16le", 2));
            }
            return h2(buffer.toString("utf8"));
        }
        console.log(readFile(process.argv[1]));
    "#,
    );
    for key in ["q1", "q2"] {
        assert!(!site(&pf, key).top, "{key}\n{}", pf.dump_text());
    }
}

/// Main: a program object passed as the encoding escaped ("argument of
/// native module fs.toString"). It is coerced (ToPrimitive runs its
/// `toString`, whose store is seen) and handed to no one.
#[test]
fn encoding_object_is_coerced_not_escaped() {
    let pf = facts(
        r#"
        class P {
            x: any;
            constructor(x: number) { this.x = x; }
            toString() { this.x = "s"; return "utf8"; }
        }
        const p = new P(1);
        function readFile(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: Buffer;
            try { buffer = _fs.readFileSync(f); } catch { return undefined; }
            return buffer.toString(p as any);
        }
        console.log(readFile(process.argv[1]));
        console.log(p.x);
    "#,
    );
    let t = tmpl(&pf, "P");
    assert!(t.escape.is_none(), "{}", pf.dump_text());
    assert!(!field(t, "x").readonly, "{}", pf.dump_text());
}

/// Named control: an own `toString` of the buffer is what the call runs
/// (as the runtime's dispatch does): it receives the argument.
#[test]
fn own_to_string_of_the_buffer_runs() {
    let pf = facts(
        r#"
        class H { v: any; constructor() { this.v = 0; } }
        class O { k: number; constructor() { this.k = 1; } }
        const holder = new H();
        function readFile(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: any;
            try { buffer = _fs.readFileSync(f); } catch { return undefined; }
            buffer.toString = (e: any) => { holder.v = e; return "x"; };
            return buffer.toString(new O());
        }
        console.log(readFile(process.argv[1]));
        console.log(holder.v);
    "#,
    );
    let v = field(tmpl(&pf, "H"), "v");
    assert!(
        v.kind.top || v.kind.objs.iter().any(|o| o.ends_with(":O")),
        "{}",
        pf.dump_text()
    );
}

/// Named control: a program store to `Buffer.prototype.toString` is never
/// proven away: the call's result is not a string only.
#[test]
fn stored_prototype_to_string_is_seen() {
    let pf = facts(
        r#"
        function g(v: any) { return v.k; }
        (Buffer.prototype as any).toString = function () { return { k: 1 }; };
        function readFile(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: Buffer;
            try { buffer = _fs.readFileSync(f); } catch { return undefined; }
            return g(buffer.toString("utf8"));
        }
        console.log(readFile(process.argv[1]));
    "#,
    );
    let s = site(&pf, "k");
    assert!(s.top || !s.receivers.is_empty(), "{}", pf.dump_text());
}

/// Named control: the runtime dispatch returns 0 for a nullish receiver
/// (node throws a TypeError): the result may be a number.
#[test]
fn nullish_receiver_result_may_be_a_number() {
    let pf = facts(
        r#"
        function g1(v: any) { return v.q1; }
        function g2(v: any) { return v.q2; }
        function readA(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: Buffer;
            try { buffer = _fs.readFileSync(f); } catch { return undefined; }
            if (f.length > 3) buffer = undefined as any;
            return g1(buffer.toString("utf8"));
        }
        function readB(f: string) {
            const _fs: typeof import("fs") = require("fs");
            let buffer: Buffer = undefined as any;
            if (f.length > 3) buffer = _fs.readFileSync(f);
            return g2(buffer.toString("utf8"));
        }
        console.log(readA(process.argv[1]), readB(process.argv[1]));
    "#,
    );
    for k in ["q1", "q2"] {
        let s = site(&pf, k);
        assert!(
            s.top || s.receiver_prims & prim::NUM != 0,
            "{k}\n{}",
            pf.dump_text()
        );
    }
}
