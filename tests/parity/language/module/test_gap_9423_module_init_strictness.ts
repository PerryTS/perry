// #9423: ES module top-level code is ALWAYS strict (ES2024 SS11.2.2: a Module
// is strict mode code, with no directive prologue needed). Perry lowers module
// init as a synthetic function with `is_strict_fn: false`
// (codegen/entry.rs, deliberately mirrored in entry_outline.rs), so every
// codegen lane that reads `ctx.is_strict_fn` rather than a flag carried on the
// HIR node sees SLOPPY at module top level.
//
// `Expr::IndexSet` is exactly such a lane (expr/dispatch.rs passes
// `ctx.is_strict_fn`), and `for` heads and destructuring assignment targets are
// what produce an `IndexSet` at module top level. A rejected write there
// silently no-ops in code the spec says must throw.
//
// This repo's package is `"type": "module"`, so a plain `.ts` fixture is
// strict-mode ESM in BOTH runtimes -- which is the whole point of this file.
// The sloppy control for the same shapes is
// test_gap_9422_strict_object_store_strictness.cts, which is a `.cts` and
// therefore a CommonJS script in both runtimes.
//
// Every write below sits at MODULE TOP LEVEL on purpose. Wrapping any of them
// in a function would move it to a lowering that already carries the right
// strictness and would test nothing.

function report(name: string, threw: boolean, ...rest: unknown[]): void {
  console.log(name, threw ? "TypeError" : "silent", ...rest);
}

function hasOwn(value: any, key: PropertyKey): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

let threw = false;

// `this` at module top level is DELIBERATELY NOT ASSERTED here. Node gives
// `undefined` for an ES module, and Perry gives a CommonJS `module.exports`
// stand-in -- but that is not this bug. Module top-level `this` lowers to its
// own HIR node, `Expr::ModuleTopThis`, chosen in `lower_expr`'s `ast::Expr::This`
// arm and switched only by `PERRY_GLOBAL_SCRIPT_THIS` (#5579/#5346/#5511); it
// never consults strictness at all, so no `is_strict_fn` fix can change it.
// Perry compiles a standalone program as CJS on purpose, which is a separate
// module-goal decision from the strictness one and is reported, not changed.

// An assignment to an undeclared name is a ReferenceError in strict code
// instead of creating a global.
threw = false;
let undeclaredKind = "silent";
try {
  // @ts-expect-error -- assigning to an undeclared name is the point.
  moduleUndeclaredBinding = 1;
} catch (e) {
  threw = true;
  undeclaredKind = (e as Error).constructor.name;
}
console.log("module undeclared assignment:", undeclaredKind);

// --- Rejected named writes at module top level -------------------------------

const frozen: any = { x: 1 };
Object.freeze(frozen);
threw = false;
try {
  frozen.x = 9;
} catch {
  threw = true;
}
report("module frozen named:", threw, frozen.x);

const frozenComputed: any = { x: 1 };
Object.freeze(frozenComputed);
const key = "x";
threw = false;
try {
  frozenComputed[key] = 9;
} catch {
  threw = true;
}
report("module frozen computed:", threw, frozenComputed.x);

const frozenNew: any = { x: 1 };
Object.freeze(frozenNew);
threw = false;
try {
  frozenNew.y = 9;
} catch {
  threw = true;
}
report("module frozen new:", threw, hasOwn(frozenNew, "y"));

// --- The `IndexSet` lanes named in #9423 -------------------------------------
//
// A `for` head whose target is a member expression, and a destructuring
// assignment whose target is a member expression, are the two lowerings that
// produce an `Expr::IndexSet` (rather than an `Expr::PutValueSet`, which
// carries the reference's own strictness).

const forHeadProp: any = { x: 1 };
Object.freeze(forHeadProp);
threw = false;
try {
  for (forHeadProp.x of [7]) {
    // body intentionally empty
  }
} catch {
  threw = true;
}
report("module frozen for-of head named:", threw, forHeadProp.x);

const forHeadIndex: any = { x: 1 };
Object.freeze(forHeadIndex);
const forHeadKey = "x";
threw = false;
try {
  for (forHeadIndex[forHeadKey] of [7]) {
    // body intentionally empty
  }
} catch {
  threw = true;
}
report("module frozen for-of head computed:", threw, forHeadIndex.x);

const destructureProp: any = { x: 1 };
Object.freeze(destructureProp);
threw = false;
try {
  ({ x: destructureProp.x } = { x: 9 });
} catch {
  threw = true;
}
report("module frozen destructure named:", threw, destructureProp.x);

const destructureIndex: any = { x: 1 };
Object.freeze(destructureIndex);
const destructureKey = "x";
threw = false;
try {
  ({ x: destructureIndex[destructureKey] } = { x: 9 });
} catch {
  threw = true;
}
report("module frozen destructure computed:", threw, destructureIndex.x);

// --- Array element / length at module top level ------------------------------

const frozenArray: any[] = [1, 2];
Object.freeze(frozenArray);
threw = false;
try {
  frozenArray[0] = 9;
} catch {
  threw = true;
}
report("module frozen array index:", threw, frozenArray[0]);

const frozenArrayForHead: any[] = [1, 2];
Object.freeze(frozenArrayForHead);
threw = false;
try {
  for (frozenArrayForHead[0] of [7]) {
    // body intentionally empty
  }
} catch {
  threw = true;
}
report("module frozen array for-of head:", threw, frozenArrayForHead[0]);

const frozenArrayDestructure: any[] = [1, 2];
Object.freeze(frozenArrayDestructure);
threw = false;
try {
  [frozenArrayDestructure[0]] = [7];
} catch {
  threw = true;
}
report("module frozen array destructure:", threw, frozenArrayDestructure[0]);

const frozenArrayLength: any[] = [1, 2];
Object.freeze(frozenArrayLength);
threw = false;
try {
  frozenArrayLength.length = 0;
} catch {
  threw = true;
}
report("module frozen array length:", threw, frozenArrayLength.length);

// --- Over-throw controls: these succeed in strict mode too -------------------

const sealed: any = { x: 1 };
Object.seal(sealed);
threw = false;
try {
  sealed.x = 9;
} catch {
  threw = true;
}
report("module sealed own:", threw, sealed.x);

const noExtendOwn: any = { x: 1 };
Object.preventExtensions(noExtendOwn);
threw = false;
try {
  noExtendOwn.x = 9;
} catch {
  threw = true;
}
report("module preventExtensions own:", threw, noExtendOwn.x);

const plain: any = { x: 1 };
threw = false;
try {
  for (plain.x of [7]) {
    // body intentionally empty
  }
} catch {
  threw = true;
}
report("module plain for-of head:", threw, plain.x);

const plainArray: any[] = [1, 2];
threw = false;
try {
  [plainArray[0]] = [7];
} catch {
  threw = true;
}
report("module plain array destructure:", threw, plainArray[0]);

export {};
