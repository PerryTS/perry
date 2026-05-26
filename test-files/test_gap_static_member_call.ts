// #1787 / #321: calling a STATIC FIELD that holds a callable —
// `Class.make(args)` where `static make = (x) => ...` (an arrow field, not a
// static method) — mis-dispatched. The static-method vtable walk missed it
// (it's a field), so the call fell through to `js_class_static_method_call`,
// which returns the receiver class ref on a method miss → `Class.make(5)` came
// back as `1` (the INT32 class id). Cross-module it was worse: even static
// METHODS on an imported class returned `1`, because the importing module's
// class stub has no compile-time static methods/fields and the receiver
// (`ExternFuncRef` / `namespace.Class`) never reached the static-dispatch tower.
//
// This is effect's `SchemaAST.Union` shape: `static make = (...) => ...` called
// as `AST.Union.make([...])` from `ParseResult.ts` — which returned undefined
// and crashed Schema decode reading `_tag`.
//
// Fix: a static FIELD holding a callable is read and invoked as a closure
// (compile-time for same-module `ClassRef`, runtime via the class_id
// CLASS_DYNAMIC_PROPS registry for imported `ExternFuncRef` / `namespace.Class`
// receivers). Compared byte-for-byte against `node --experimental-strip-types`.

import * as NS from "./_helpers/static_members_mod.ts";
import { Factory, SubFactory } from "./_helpers/static_members_mod.ts";

// (1) static arrow field — direct import.
console.log("Factory.make(5):", (Factory as any).make(5)); // 10
// (2) static function-expression field.
console.log("Factory.makeFn(5):", (Factory as any).makeFn(5)); // 105
// (3) static method (regression guard).
console.log("Factory.label(5):", Factory.label(5)); // F:5
// (4) static arrow field — via namespace import (effect's `AST.Union.make`).
console.log("NS.Factory.make(7):", (NS.Factory as any).make(7)); // 14
console.log("NS.Factory.label(7):", NS.Factory.label(7)); // F:7
// (5) inherited static field through a subclass.
console.log("SubFactory.make(8):", (SubFactory as any).make(8)); // 16 (inherited)
console.log("SubFactory.extra(8):", (SubFactory as any).extra(8)); // 5 (own)
console.log("NS.SubFactory.make(9):", (NS.SubFactory as any).make(9)); // 18

// Same-module class with a static arrow field.
class Local {
  static twice = (n: number): number => n + n;
}
console.log("Local.twice(6):", (Local as any).twice(6)); // 12
