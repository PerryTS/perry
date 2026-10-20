// `class X extends ns.Error {}` extends a PROPERTY of `ns`, not the global
// Error. Effect declares `class SystemError extends Data.Error`, where
// `Data.Error` is a package class (`core.Error`, made by an IIFE). Perry kept
// the trailing name `Error` as the static parent, so once the heritage was
// proven unrelinked `super(args)` ran the built-in Error initializer: the
// package constructor never ran (no fields, message "[object Object]") and
// `Object.getPrototypeOf(SystemError)` was the global Error. opencode's TUI
// then failed decoding a NotFound PlatformError as a Ripgrep.Error.
// #11139 fixed the same shape for names like `URL`/`Map`; this covers the
// rest of the names codegen keys `super(...)` on (Error family, Event,
// Response, ...). The fixtures are under fixtures/member_heritage_error_family/.
import * as ns from "./fixtures/member_heritage_error_family/ns.mjs";
import { SystemError as PackageSystemError } from "./fixtures/member_heritage_error_family/platform.mjs";
import { notFound } from "./fixtures/member_heritage_error_family/fs.mjs";

// Constructed in a third module, as Effect's FileSystem does.
const nf: any = notFound("/missing");
console.log("package", nf._tag, nf.module, nf.path, nf.message, Object.keys(nf).join(","));
console.log(
  "package chain",
  nf instanceof PackageSystemError,
  nf instanceof ns.Error,
  Object.getPrototypeOf(PackageSystemError) === ns.Error,
  Object.getPrototypeOf(PackageSystemError) === Error,
);

class SystemError extends ns.Error {
  get message(): string {
    return `${this._tag}: ${this.module}.${this.method}`;
  }
}
const s: any = new SystemError({ _tag: "NotFound", module: "FS", method: "stat" });
console.log("system", s._tag, s.module, s.message, Object.keys(s).join(","));
console.log(
  "system chain",
  s instanceof SystemError,
  s instanceof ns.Error,
  s instanceof Error,
  Object.getPrototypeOf(SystemError) === ns.Error,
  Object.getPrototypeOf(SystemError) === Error,
);

const wrap = (cause: unknown) => (cause instanceof ns.Error ? "kept" : "wrapped");
console.log("wrap", wrap(s), wrap(new Error("plain")));

class Plain extends ns.TypeError {}
const p: any = new Plain(7);
console.log("plain", p.v, Object.keys(p).join(","), Object.getPrototypeOf(Plain) === ns.TypeError, p instanceof TypeError);

class Ev extends ns.Event {}
const e: any = new Ev("x");
console.log("event", e.kind, e.type, Object.keys(e).join(","), Object.getPrototypeOf(Ev) === ns.Event);

class Res extends ns.Response {}
const r: any = new Res("b");
console.log("response", r.payload, Object.keys(r).join(","), Object.getPrototypeOf(Res) === ns.Response);
