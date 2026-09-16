// Instances of a pointer-bearing class allocated in an importing module must
// behave exactly like instances allocated by the defining module, for stores
// compiled on either side, subclasses, fieldless classes and class expressions.
import {
  Node6,
  Sub6,
  Empty6,
  Expr6,
  makeNode,
  setNext,
  setValue,
  describe,
} from "./_helpers/cross_module_class_shape_identity.ts";
import { builtAtInit, buildChain, buildPair } from "./_helpers/cross_module_class_shape_identity_builder.ts";
import { relink } from "./_helpers/cross_module_class_shape_identity_store.ts";
import { CycNode, cycleReport, linkCyc } from "./_helpers/cross_module_shape_cycle_a.ts";
import { buildFromB } from "./_helpers/cross_module_shape_cycle_b.ts";

// Built here, stored by the defining module.
const a = new Node6(1);
const b = new Node6(2);
setNext(a, b);
setValue(a, 10);
console.log("importer-built:", describe(a), describe(b));

// Built by the defining module, stored here.
const c = makeNode(3);
c.next = a;
c.value = 30;
c.link(a);
c.bump(0.5);
console.log("producer-built:", describe(c), c.total());

// A long chain of alternating allocation sites and store sites.
let head: Node6 = new Node6(0);
for (let i = 1; i < 3000; i++) {
  const n = i % 2 === 0 ? new Node6(i) : makeNode(i);
  if (i % 3 === 0) {
    setNext(n, head);
  } else {
    n.next = head;
  }
  if (i % 7 === 0) {
    setValue(n, i * 1.5);
  }
  if (i % 11 === 0) {
    n.value = n.value - 1;
  }
  head = n;
}
console.log("chain total:", head.total());

// A store that contradicts the declared field type must still read back.
const d = new Node6(4);
(d as any).value = "not a number";
setNext(d, b);
console.log("contradicting store:", describe(d), typeof d.value);
(d as any).next = { value: 99 };
console.log("foreign next:", (d.next as any).value);

// Subclass defined next to the base, allocated here.
const s = new Sub6(5);
setNext(s, a);
setValue(s, 50);
console.log("subclass:", describe(s), s.extra, s instanceof Node6, s instanceof Sub6);

// Fieldless class and class expression.
const e = new Empty6();
(e as any).x = 1;
console.log("empty:", JSON.stringify(e), Object.keys(e).join(","));
const x = new Expr6(6);
x.b = b;
x.a = 7;
console.log("expr:", x.a, x.b === null ? "null" : x.b.value);

// Reflection sees the same layout for both allocation sites.
console.log("keys:", Object.keys(a).join(","), Object.keys(makeNode(8)).join(","));
console.log("json:", JSON.stringify({ v: b.value, l: b.label, n: b.next }));

// Four modules: the class is declared in A, instances are built by non-entry
// module B (at its init and later), stores happen in non-entry module C, and
// this entry module mixes in its own instances.
const mixed: Node6[] = [...builtAtInit, ...buildPair(200), new Node6(300), makeNode(400)];
console.log("four-module relink:", relink(mixed, 50), mixed.map((n) => describe(n)).join(" "));
const chain = buildChain(1000);
console.log("four-module chain:", chain.total(), chain instanceof Node6);
setNext(builtAtInit[0], chain);
console.log("producer store on builder object:", describe(builtAtInit[0]));

// An import cycle: B's string pool ran before A (which declares CycNode)
// initialized; B builds and stores, A stores, and so does this entry module.
console.log("cycle:", cycleReport());
const cyc = buildFromB(3);
cyc[0].next = cyc[1];
linkCyc(cyc[1], new CycNode(99));
console.log("cycle entry:", cyc.map((n) => n.tag + ">" + (n.next === null ? "null" : n.next.tag)).join(","), cyc[0] instanceof CycNode);

// A class declared in a module reached only through `await import()`, built and
// stored by another lazily imported module that imports it statically, and by
// this entry module through the namespace.
const lazyBuilder = await import("./_helpers/cross_module_shape_lazy_builder.ts");
const lazyClass = await import("./_helpers/cross_module_shape_lazy_class.ts");
const lazyNodes = [...lazyBuilder.lazyAtInit, ...lazyBuilder.buildLazy(4), new lazyClass.LazyNode(50)];
console.log("lazy relink:", lazyBuilder.relinkLazy(lazyNodes, 30));
lazyClass.setLazyNext(lazyNodes[6], lazyNodes[0]);
lazyNodes[0].next = lazyNodes[6];
console.log("lazy:", lazyNodes.map((n) => lazyClass.describeLazy(n)).join(" "), lazyNodes[6] instanceof lazyClass.LazyNode);
