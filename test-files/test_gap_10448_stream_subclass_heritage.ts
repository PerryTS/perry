// #10448: node:stream subclass overrides (`_transform`/`_write`/`_read`)
// were ignored whenever the heritage reaching `class X extends <base>` was
// anything OTHER than a shape `is_genuine_node_stream_parent` recognizes
// statically at HIR-lowering time
// (`crates/perry-hir/src/lower_decl/class_decl.rs`) — a local alias
// (`const Alias = Transform`), an indirect subclass, a class expression, or
// a CJS destructured `require('stream')` (the shape nodemailer uses in
// every stream class it defines). `write()`/`push()` then threw
// `ERR_METHOD_NOT_IMPLEMENTED` because the override was never installed on
// `this`.
//
// (`PassThrough` is a separate, deeper gap — HIR never recognizes it
// statically even via a bare import, unlike Readable/Writable/Duplex/
// Transform, so it needs its own follow-up; not covered by this test, see
// the PR body.)
//
// Each check awaits its own stream before starting the next so output order
// is deterministic regardless of engine event-loop/nextTick scheduling
// differences — only the per-check content is the thing under test. State
// is captured via public class fields (not a `constructor(...args) {
// super(...args); ... }` rest-spread pattern, which hits an unrelated
// pre-existing gap independent of heritage shape).
import { Transform, Writable, Readable, Duplex } from "stream";
import * as streamNs from "stream";
import {
  CjsTransform,
  CjsViaMember,
  CjsWritable,
  CjsReadable,
  CjsDuplex,
} from "./gap_10448_stream_subclass_heritage_helper.cjs";

const AliasTransform = Transform;

class ViaImport extends Transform {
  _transform(chunk: any, _enc: string, cb: any) {
    cb(null, String(chunk).toUpperCase());
  }
}
class ViaAlias extends AliasTransform {
  _transform(chunk: any, _enc: string, cb: any) {
    cb(null, String(chunk).toUpperCase());
  }
}
class ViaNamespaceMember extends streamNs.Transform {
  _transform(chunk: any, _enc: string, cb: any) {
    cb(null, String(chunk).toUpperCase());
  }
}
class Mid extends AliasTransform {}
class ViaIndirect extends Mid {
  _transform(chunk: any, _enc: string, cb: any) {
    cb(null, String(chunk).toUpperCase());
  }
}
const ViaClassExpr = class extends AliasTransform {
  _transform(chunk: any, _enc: string, cb: any) {
    cb(null, String(chunk).toUpperCase());
  }
};

function runTransform(name: string, T: any): Promise<void> {
  return new Promise((resolve) => {
    const t = new T();
    let out = "";
    t.on("data", (c: any) => (out += c));
    t.on("end", () => {
      console.log(name, JSON.stringify(out));
      resolve();
    });
    try {
      t.write("ab");
      t.end("c");
    } catch (e: any) {
      console.log(name, "threw", e.code);
      resolve();
    }
  });
}

function runWritable(name: string, W: any): Promise<void> {
  return new Promise((resolve) => {
    let w: any;
    try {
      w = new W();
    } catch (e: any) {
      console.log(name, "threw (construct)", e.message);
      resolve();
      return;
    }
    w.on("finish", () => {
      console.log(name, JSON.stringify(w.captured));
      resolve();
    });
    try {
      w.write("ab");
      w.end("c");
    } catch (e: any) {
      console.log(name, "threw", e.code);
      resolve();
    }
  });
}

function runReadable(name: string, R: any): Promise<void> {
  return new Promise((resolve) => {
    let r: any;
    try {
      r = new R();
    } catch (e: any) {
      console.log(name, "threw (construct)", e.message);
      resolve();
      return;
    }
    let out = "";
    r.on("data", (c: any) => (out += c));
    r.on("end", () => {
      console.log(name, JSON.stringify(out));
      resolve();
    });
  });
}

class WViaWritable extends Writable {
  captured = "";
  _write(chunk: any, _enc: string, cb: any) {
    this.captured += String(chunk).toUpperCase();
    cb();
  }
}

class RViaReadable extends Readable {
  private _done = false;
  _read() {
    if (this._done) return;
    this._done = true;
    this.push("m");
    this.push("n");
    this.push(null);
  }
}

class DViaDuplex extends Duplex {
  captured = "";
  _write(chunk: any, _enc: string, cb: any) {
    this.captured += String(chunk).toUpperCase();
    cb();
  }
}

async function main() {
  await runTransform("Transform    via import          ", ViaImport);
  await runTransform("Transform    via alias           ", ViaAlias);
  await runTransform("Transform    via namespace member", ViaNamespaceMember);
  await runTransform("Transform    via indirect subclas", ViaIndirect);
  await runTransform("Transform    via class expression", ViaClassExpr);
  await runTransform("Transform    CJS destructured    ", CjsTransform);
  await runTransform("Transform    CJS namespace member", CjsViaMember);
  await runWritable("Writable     CJS destructured    ", CjsWritable);
  await runWritable("Duplex       CJS destructured    ", CjsDuplex);
  await runReadable("Readable     CJS destructured    ", CjsReadable);
  await runWritable("Writable     via import          ", WViaWritable);
  await runReadable("Readable     via import          ", RViaReadable);
  await runWritable("Duplex       via import          ", DViaDuplex);
}

main();
