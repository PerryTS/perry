// An explicitly imported class must not lose its identity to a same-named
// class that is only reachable transitively.
//
// The import of `helper` reaches a module that also exports an unrelated
// `class Shared`. Registering that one under the bare name `Shared` made
// `new Shared(...)` stamp its class id, whose method table is empty, so
// `s.defineThing` threw "defineThing is not a function" inside the
// constructor. Swapping the two import lines below used to decide whether the
// program worked, because the winner was whichever entry was pushed first.
import { helper } from "./fixtures/import_name_collision_decoy.ts";
import { Shared } from "./fixtures/import_name_collision_real.ts";

const made = new Shared((s) => {
  console.log("[name-collision]", s.defineThing("x"));
});

console.log("[name-collision] ok", typeof made, helper());
