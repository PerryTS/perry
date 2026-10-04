// RegExp spec-observability rows (REGEXP_ONE_SHAPE_DESIGN S0) that are KNOWN
// WRONG on main: perry's output differs from node for every row below, so this
// file is a recorded `parity_fail` entry in test-parity/gap_snapshot.json.
// Each row is fixed by a later slice and the snapshot entry must be deleted
// when the last one flips (the snapshot gate fails when a listed test passes):
//
//   proto_test_patched_local  perry true, node "patched"          fixed by S5
//   flags_reads_getters       perry "g", node ""                  fixed by S3
//   subclass_exec             r instanceof R false, node true     fixed by S2
//   expando                   perry prints an empty line          fixed by S2
//   setPrototypeOf            perry true, node "p"                fixed by S2
//   delete_lastIndex          perry true, node throws TypeError   fixed by S2
// One output line per row; a throw is printed as `throw <ErrorName>`.
function t(name: string, f: () => any): void {
  let line: string;
  try {
    line = name + ": " + JSON.stringify(f());
  } catch (e: any) {
    line = name + ": throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(line);
}
const mk = (): any => /a/;

t("proto_test_patched_local", () => { const r: any = /a/; const o = RegExp.prototype.test; RegExp.prototype.test = function () { return "patched"; }; try { return r.test("a"); } finally { RegExp.prototype.test = o; } });
t("flags_reads_getters", () => { const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "global"); Object.defineProperty(RegExp.prototype, "global", { get() { return false; }, configurable: true }); try { return /a/g.flags; } finally { Object.defineProperty(RegExp.prototype, "global", d); } });
t("subclass_exec", () => { class R extends RegExp { exec(s: any) { return null; } } const r = new R("a"); return [r.test("a"), r instanceof RegExp, r instanceof R, Object.prototype.toString.call(r)]; });
t("expando", () => { const r: any = /a/; r.foo = 1; return [r.foo, Object.keys(r), JSON.stringify(r)]; });
t("setPrototypeOf", () => { const r: any = mk(); Object.setPrototypeOf(r, { test() { return "p"; } }); return r.test("a"); });
t("delete_lastIndex", () => { "use strict"; const r: any = /a/; return delete r.lastIndex; });
