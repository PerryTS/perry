// #8692: a Uint32Array indexed `+=` keeps one guarded numeric representation
// through get/add/set.  The smaller sibling fixture
// `test_issue_8692_typed_array_rmw_gc.ts` runs the same parameter/global/alias
// representation under forced moving collections.

// The ticket's complete reduced wolf-ecs reproduction.  Both Node and Perry
// must report checksum 2000; the compiler-output ratchet separately asserts
// that the guarded fast arm contains no generic get/add/set helper.
class Query extends Array<number[]> {
  archetypes = this;
}

class Archetype extends Array<number> {
  entities = this;
}

const entityCount = 1_000;
const iterations = 2_000;
const query = new Query();
const archetype = new Archetype();
for (let i = 0; i < entityCount; i++) archetype.push(i);
query.push(archetype);

const components = new Uint32Array(entityCount);

function system(values: Uint32Array): void {
  for (let i = 0, length = query.length; i < length; i++) {
    const current = query[i];
    for (let j = 0, length = current.length; j < length; j++) {
      values[current[j]] += 1;
    }
  }
}

for (let i = 0; i < iterations; i++) system(components);
console.log("repro", components[0], components[entityCount - 1]);

// Dynamic-key guard success plus every important guard-failure class.  A
// string canonical index must stay semantic even though it cannot use the raw
// numeric arm; OOB/fractional/NaN/infinite/negative writes remain no-ops.
function bump(values: Uint32Array, key: any): void {
  values[key] += 1;
}

const guarded = new Uint32Array(3);
guarded[0] = 9;
const keys: any[] = [0, -1, 1.5, NaN, Infinity, 99, "0"];
for (let i = 0; i < keys.length; i++) bump(guarded, keys[i]);
console.log("guards", guarded[0], guarded[1], guarded[2]);

// Uint32 conversion is modulo 2^32, while the assignment expression itself
// yields the unwrapped numeric sum.
const wrapping = new Uint32Array(1);
wrapping[0] = 0xffffffff;
const expressionValue = (wrapping[0] += 2);
console.log("wrapping", wrapping[0], expressionValue);

const conversions = new Uint32Array(3);
conversions[0] = 5;
const negativeFraction = (conversions[0] += -6.75);
conversions[1] = 1;
const hugeFinite = (conversions[1] += 1e300);
conversions[2] = 1;
const infinite = (conversions[2] += 1e300 * 1e300);
console.log(
  "conversions",
  conversions[0],
  negativeFraction,
  conversions[1],
  hugeFinite,
  conversions[2],
  infinite,
);

// The read precedes RHS evaluation.  Mutating through an alias in the RHS
// must not change the old value used by the addition, and RHS is called once.
const aliased = new Uint32Array(1);
const same = aliased;
aliased[0] = 5;
let rhsCalls = 0;
function mutatingRhs(): number {
  rhsCalls++;
  same[0] = 40;
  const churn: object[] = [];
  for (let i = 0; i < 256; i++) churn.push({ i, text: "x".repeat(64) });
  return 2;
}
same[0] += +mutatingRhs();
console.log("alias-order", aliased[0], rhsCalls);

// Abrupt RHS completion performs no store and is never retried.
const abrupt = new Uint32Array(1);
abrupt[0] = 12;
let throws = 0;
try {
  abrupt[0] += +(() => {
    throws++;
    throw new Error("stop");
  })();
} catch (error) {
  console.log("abrupt", (error as Error).message, abrupt[0], throws);
}

// Captured/module-global representation.  The dynamic key prevents a static
// bounds proof, while the runtime kind/index guard keeps the direct arm safe.
const globalValues = new Uint32Array(2);
function capturedBump(key: any): void {
  globalValues[key] += 1;
}
const dynamicKeys: any[] = [0, 1, 0];
for (let i = 0; i < dynamicKeys.length; i++) capturedBump(dynamicKeys[i]);
console.log("captured", globalValues[0], globalValues[1]);

// ArrayBuffer views and detached stores are intentionally rejected by the
// inline-storage guard.  Detaching in the RHS also pins the get-before-RHS and
// pending-set semantics without allowing a stale direct backing-store write.
const backing = new ArrayBuffer(4);
const detached = new Uint32Array(backing);
detached[0] = 7;
let detachCalls = 0;
function detachRhs(): number {
  detachCalls++;
  backing.transfer();
  return 3;
}
const detachedResult = (detached[0] += +detachRhs());
console.log(
  "detached",
  detachedResult,
  detached[0] === undefined,
  backing.byteLength,
  detachCalls,
);
