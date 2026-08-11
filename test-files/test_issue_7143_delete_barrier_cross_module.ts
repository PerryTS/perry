// #7143: the Phase 5a `delete` shape barrier is computed per-module
// (`collect_module_dispatch_facts`), but a proven-`this` receiver is
// aliased across modules by construction. This module (B) never mentions
// `delete`, so module A (fixtures/issue_7143_pkg/shared.ts) is free to admit
// a `readC$pshape` clone for `C.readC` and route `readViaA`'s call to it.
// This module DOES `delete` a declared field off an instance A handed it,
// then calls back into A's `readViaA`, which dispatches `inst.readC()` from
// inside module A on the now-mutated object.
import {
  C,
  makeC,
  readViaA,
} from "./fixtures/issue_7143_pkg/shared.ts";

const inst = makeC();
console.log("before delete:", readViaA(inst));

delete (inst as any).b;

console.log("direct field read after delete:", (inst as any).c);
console.log("via module A after delete:", readViaA(inst));
