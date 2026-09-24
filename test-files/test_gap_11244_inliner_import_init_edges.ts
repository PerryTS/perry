// #11244: the cross-module method inliner must not give a module an import
// (and so a module-init edge) for a method it never inlines.
//
// Every inlinable method harvested from an earlier-compiled module used to
// queue imports for the names its body reads, whether or not any call site in
// the destination inlined it. Each synthesized import is an ordinary static
// init dependency, so a module's init ran module bodies that Node runs later
// or never. The cycle form, which broke `import { MongoClient } from
// "mongodb"`, is test_gap_11244_inliner_import_cycle.ts.
//
// Covered here, each with its own output line:
//   1. extra init: util.ts is reachable only through a lazily imported
//      err.ts, so Node runs it at the end, after "holder". Perry ran it at
//      startup, before this module's body.
//   2. control: a method this module DOES inline (exact receiver `a`) still
//      gets the import its body reads, and computes the right value.
async function lazyErr(): Promise<number> {
  const m = await import("./fixtures/issue_11244_inliner_import_edges/err.ts");
  return new m.ParseError().code();
}
async function neverCalled(): Promise<number> {
  const m = await import("./fixtures/issue_11244_inliner_import_edges/err.ts");
  return new m.ParseError().code() + 1;
}
import { W } from "./fixtures/issue_11244_inliner_import_edges/w.ts";
import { Adder } from "./fixtures/issue_11244_inliner_import_edges/inl_cls.ts";

console.log("main", W, typeof neverCalled);

const a = new Adder();
console.log("adder", a.add(1), a.add(2));

(async () => {
  console.log("before lazy import");
  console.log("err", await lazyErr());
  console.log("done");
})();
