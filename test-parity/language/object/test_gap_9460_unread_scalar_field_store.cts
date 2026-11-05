// #9460: SIGSEGV on `o.<field> = v` when `o` is a scalar-replaced object
// literal and that field is never READ.
//
// `stmt/let_stmt.rs`'s scalar-replacement arm elides the heap allocation for a
// non-escaping `new` and gives each field its own stack alloca -- but for a
// synthetic `__AnonShape_*` class (what an object literal lowers to) it only
// creates slots for the fields in `non_escaping_new_used_fields`, which
// deliberately tracks READS:
//
//     "writes still need their RHS evaluated for JS side effects, but the
//      scalar slot/store can be elided when the field is never observed"
//                            -- collectors/escape_news.rs
//
// It then registers `ctx.locals[id]` = a DUMMY entry-block alloca that is never
// initialized, because the binding is no longer an object at all.
//
// `expr/property_get.rs` implements the read half of that contract: a property
// of a scalar-replaced local with no slot reads `undefined`, with a comment
// naming the hazard -- "the generic runtime helper that crashes on the dummy
// slot". `expr/property_set.rs` never implemented the write half. A store to a
// slotless field fell past the scalar arm into the class-field / `Ptr<Shape>`
// lowering, which loads the dummy slot as an `ObjectHeader*` and stores through
// `null + 16`.
//
// The crash needs the field to be written and NEVER read, which is why the
// original report's isolated attempt did not reproduce: it printed `o.x`
// afterwards, and that read is what creates the slot.
//
// It is NOT specific to sloppy mode, to `for (o.x of ...)`, or to a preceding
// throw -- `"use strict"; const o: any = {x:1}; o.x = 7;` segfaults too. This
// file is `.cts` so both modes live in one program.
//
// Every case here prints a sentinel INSTEAD of reading the field: reading it
// is what hides the bug.

function sloppyForOfHead(): void {
  const o: any = { x: 1 };
  for (o.x of [7]) {
  }
  console.log("sloppy for-of head: survived");
}

function strictForOfHead(): void {
  "use strict";
  const o: any = { x: 1 };
  for (o.x of [7]) {
  }
  console.log("strict for-of head: survived");
}

function sloppyPlainAssign(): void {
  const o: any = { x: 1 };
  o.x = 7;
  console.log("sloppy plain assign: survived");
}

function strictPlainAssign(): void {
  "use strict";
  const o: any = { x: 1 };
  o.x = 7;
  console.log("strict plain assign: survived");
}

function strictCompoundAssign(): void {
  "use strict";
  const o: any = { x: 1 };
  o.x += 1;
  console.log("strict compound assign: survived");
}

function strictComputedAssign(): void {
  "use strict";
  const o: any = { x: 1 };
  const k = "x";
  o[k] = 7;
  console.log("strict computed assign: survived");
}

function strictUpdateNewField(): void {
  "use strict";
  const o: any = { x: 1 };
  o.y++;
  console.log("strict update new field: survived");
}

function strictUpdate(): void {
  "use strict";
  const o: any = { x: 1 };
  o.x++;
  console.log("strict update: survived");
}

function strictDestructure(): void {
  "use strict";
  const o: any = { x: 1 };
  [o.x] = [7];
  console.log("strict destructure: survived");
}

// A store to a field the literal never declared, still never read.
function strictNewField(): void {
  "use strict";
  const o: any = { x: 1 };
  o.y = 7;
  console.log("strict new field: survived");
}

// The RHS must still be evaluated for its side effects even when the store
// itself is elided -- that is the half of the contract `escape_news.rs`
// promises, and eliding the whole statement would silently drop this call.
let sideEffects = 0;
function bump(): number {
  sideEffects += 1;
  return 7;
}

function strictStoreEvaluatesRhs(): void {
  "use strict";
  const o: any = { x: 1 };
  o.x = bump();
  o.x = bump();
  console.log("strict rhs side effects:", sideEffects);
}

// CONTROL: the same shape WITH a read. This is the version that already
// worked, kept so a fix that broke it would show up here rather than in a
// benchmark. The field must still hold what was stored.
function strictReadAfterStore(): void {
  "use strict";
  const o: any = { x: 1 };
  o.x = 7;
  console.log("strict read after store:", o.x);
}

function strictReadAfterForOfHead(): void {
  "use strict";
  const o: any = { x: 1 };
  for (o.x of [7]) {
  }
  console.log("strict read after for-of head:", o.x);
}

// CONTROL: an ESCAPING receiver keeps its heap object, so the store is a real
// one. `Object.freeze` is a call, which is what makes it escape.
function strictEscapingReceiver(): void {
  "use strict";
  const o: any = { x: 1 };
  const seen: any[] = [];
  seen.push(o);
  o.x = 7;
  console.log("strict escaping receiver:", seen[0].x);
}

sloppyForOfHead();
strictForOfHead();
sloppyPlainAssign();
strictPlainAssign();
strictCompoundAssign();
strictComputedAssign();
strictUpdate();
strictUpdateNewField();
strictDestructure();
strictNewField();
strictStoreEvaluatesRhs();
strictReadAfterStore();
strictReadAfterForOfHead();
strictEscapingReceiver();
