// #11275: a re-exported IMPORTED binding must keep its kind (value, function,
// class, namespace) through every re-export chain.
//
// `import { x } from "./a"; export { x }` in B plus `export * from "./b"` in
// C handed importers of C a function in place of x: its properties read
// `undefined`, and a method extracted from it threw `value is not a
// function`. bson's `ByteUtils` takes exactly this path into mongodb.
//
// Root cause: the export-propagation fixpoint (`all_module_exports`) let an
// `export *` entry copy B's FIRST-iteration origin for x (B itself) and then
// never corrected it once B's own entry resolved to the real origin, A. The
// importer then looked up the var/function classification under B, where
// the value is not declared, and fell back to treating B's zero-arg getter
// as a function.
import * as B from "./fixtures/issue_11275_reexport_kinds/barrel.ts";
import {
  obj,
  plainObj,
  num,
  str,
  list,
  fn,
  arrow,
  Cls,
  counter,
  bump,
  objAlias,
  fnAlias,
  ClsAlias,
  arrowAlias,
  originNs,
  cjs,
  starNs,
} from "./fixtures/issue_11275_reexport_kinds/barrel.ts";
import { obj as obj1, fn as fn1 } from "./fixtures/issue_11275_reexport_kinds/mid2.ts";
import { obj as direct } from "./fixtures/issue_11275_reexport_kinds/mid.ts";
import { num as shadowNum, obj as shadowObj } from "./fixtures/issue_11275_reexport_kinds/shadow.ts";
import { one, two, oneAgain } from "./fixtures/issue_11275_reexport_kinds/amb.ts";
import { cycObj, CycCls } from "./fixtures/issue_11275_reexport_kinds/cyc_a.ts";
import { cycObj as cycObjB, CycCls as CycClsB } from "./fixtures/issue_11275_reexport_kinds/cyc_b.ts";

function show(label: string, f: () => unknown): void {
  try {
    console.log(label, f());
  } catch (e: any) {
    console.log(label, "THREW", e?.constructor?.name, e?.message);
  }
}

show("obj", () => [typeof obj, obj.kind, typeof obj.twice, obj.twice(4)]);
show("obj extracted method", () => {
  const twice = obj.twice;
  return twice.call(obj, 5);
});
show("plainObj", () => [typeof plainObj, plainObj.kind]);
show("num/str/list", () => [typeof num, num, typeof str, str, Array.isArray(list), list.length]);
show("fn", () => [typeof fn, fn(2, 3), fn.length]);
show("arrow", () => [typeof arrow, arrow(4)]);
show("Cls", () => [typeof Cls, new Cls(7).get(), new Cls(1) instanceof Cls]);
show("aliases", () => [objAlias.kind, objAlias === obj, fnAlias(1, 1), new ClsAlias(3).get(), arrowAlias(2)]);
show("namespaces", () => [typeof originNs, originNs.obj.kind, originNs.fn(1, 2), typeof starNs, starNs.obj.twice(3)]);
show("cjs", () => [typeof cjs, cjs.cjsObj.kind, cjs.cjsObj.hello(), cjs.cjsFn(1)]);
show("one hop", () => [typeof obj1, obj1.kind, fn1(5, 5)]);
show("direct", () => [typeof direct, direct.kind, direct === obj]);
show("shadow", () => [shadowNum, shadowObj.kind]);
show("ambiguous star", () => [one.f(), two.f(), oneAgain === one]);
show("cycle", () => [cycObj.kind, new CycCls(9).get(), cycObjB.twice(1), new CycClsB(2).get()]);
show("namespace of barrel", () => [typeof B.obj, B.obj.kind, B.fn(4, 4), typeof B.Cls, "num" in B]);
show("live binding before", () => counter);
bump();
bump();
show("live binding after", () => [counter, B.counter]);
