// Issue #554: Object destructuring with nested array pattern in
// for-of binding produces 0 instead of array elements.
//
// Pre-fix, the for-of binding pre-pass at lower.rs only pre-defined
// leaf locals for `Pat::Ident` values inside `ObjectPatProp::KeyValue`.
// The binding-emit phase had the same gap: the nested pattern fell
// through `_ => continue`, so for `{ entity, components: [a, b] }`
// neither `a` nor `b` were defined as locals — body lowering treated
// them as unknown identifiers (globals reading 0).
//
// Fix: recurse into nested `ast::Pat::Array` / `ast::Pat::Object` /
// `ast::Pat::Assign` values during both pre-pass and binding-phase,
// using a helper that mirrors `destructuring::lower_pattern_binding`.

const results = [{ entity: 1024, components: [{ x: 50, y: 30 }, { x: 1, y: 2 }] }];

// The exact pattern from the issue.
console.log("=== nested array in object KeyValue ===");
for (const { entity, components: [a, b] } of results) {
  console.log(`entity=${entity} a=${JSON.stringify(a)} b=${JSON.stringify(b)}`);
}

// Triple nesting: object → array → object.
console.log("=== triple nested ===");
for (const { entity, components: [{ x: ax, y: ay }, { x: bx }] } of results) {
  console.log(`entity=${entity} ax=${ax} ay=${ay} bx=${bx}`);
}

// Default value through nested pattern.
const withDefaults = [{ key: "k", arr: [1] }];
console.log("=== nested with default ===");
for (const { key, arr: [first, second = 99] } of withDefaults) {
  console.log(`key=${key} first=${first} second=${second}`);
}

// Rest in nested array.
const nestedWithRest = [{ id: 1, vals: [10, 20, 30, 40] }];
console.log("=== nested rest ===");
for (const { id, vals: [head, ...tail] } of nestedWithRest) {
  console.log(`id=${id} head=${head} tail=${JSON.stringify(tail)}`);
}

// Sanity: the working forms from the issue still work.
console.log("=== plain (was already working) ===");
for (const r of results) {
  console.log(`a=${JSON.stringify(r.components[0])} b=${JSON.stringify(r.components[1])}`);
}

console.log("=== two-step (was already working) ===");
for (const { entity, components } of results) {
  const [a, b] = components;
  console.log(`entity=${entity} a=${JSON.stringify(a)} b=${JSON.stringify(b)}`);
}

// Re-opened followup: the same destructure pattern inside a FUNCTION
// body lowered through `lower_decl.rs::ast::Stmt::ForOf` instead of
// `lower.rs::lower_stmt`. Both paths had the same parallel gaps; the
// initial v0.5.629 fix only covered the top-level `lower.rs` site.
function printResultsFn(items: typeof results): void {
  for (const { entity, components: [a, b] } of items) {
    console.log(`fn entity=${entity} a=${JSON.stringify(a)} b=${JSON.stringify(b)}`);
  }
}
console.log("=== nested in function body ===");
printResultsFn(results);

// Triple-nested inside a function body.
function printTripleFn(items: typeof results): void {
  for (const { entity, components: [{ x: ax, y: ay }, { x: bx }] } of items) {
    console.log(`fn3 entity=${entity} ax=${ax} ay=${ay} bx=${bx}`);
  }
}
console.log("=== triple nested in function body ===");
printTripleFn(results);

// `pos = arr[0]` shape from the issue's followup repro
// (codehz/ecs serialization.ts printWorldState).
function printSinglePos(items: typeof results): void {
  for (const { entity, components: [pos] } of items) {
    console.log(`fn1 entity=${entity} pos=${JSON.stringify(pos)} typeof=${typeof pos}`);
  }
}
console.log("=== single-element nested in function body ===");
printSinglePos(results);
