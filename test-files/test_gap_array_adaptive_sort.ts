// Generic comparator-sort coverage: stability, arbitrary values, callbacks,
// holes, exotic receivers, copying methods, coercion, and exception transport.
function check(ok: boolean, message: string) {
  if (!ok) throw new Error(message);
}

for (const size of [0, 1, 2, 31, 32, 33, 64, 65, 127, 513]) {
  for (const pattern of [0, 1, 2, 3]) {
    const records: { key: number; id: number; text: string }[] = [];
    for (let i = 0; i < size; i++) {
      let key = (i * 17) % 19;
      if (pattern === 1) key = Math.floor((size - i) / 3);
      if (pattern === 2) key = Math.floor(i / 3);
      if (pattern === 3) key = 0;
      records.push({ key, id: i, text: "item:" + i });
    }
    const original = records.slice();
    const result = records.sort((a, b) => a.key - b.key);
    check(result === records && result.length === size, "receiver identity/length");
    const seen: boolean[] = [];
    for (let i = 0; i < size; i++) {
      const item = result[i];
      check(item === original[item.id], "element identity");
      check(!seen[item.id], "permutation duplicate");
      seen[item.id] = true;
      check(item.text === "item:" + item.id, "object identity");
      if (i > 0) {
        const prev = result[i - 1];
        check(prev.key <= item.key, "sorted keys");
        check(prev.key !== item.key || prev.id < item.id, "stable equal keys");
      }
    }
  }
}
console.log("stable objects and run boundaries");

const mixed: any[] = [];
for (let i = 256; i >= 0; i--) {
  mixed.push(i % 3 === 0 ? i : i % 3 === 1 ? String(i) : { key: i });
}
function mixedKey(value: any): number {
  return typeof value === "object" ? value.key : Number(value);
}
mixed.sort((a, b) => mixedKey(a) - mixedKey(b));
for (let i = 0; i < mixed.length; i++) {
  check(mixedKey(mixed[i]) === i, "mixed value permutation");
  check(typeof mixed[i] === (i % 3 === 0 ? "number" : i % 3 === 1 ? "string" : "object"), "mixed value type");
}
console.log("mixed pointers and numbers");

const words = ["z", "a", "\u{1f600}", "aa", "a", "\uffff", ""];
words.sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
console.log("strings", JSON.stringify(words));

const equal = [4, 1, 3, 2];
equal.sort(() => NaN);
check(equal.join(",") === "4,1,3,2", "NaN means equal");
const coerced = [9, 3, 7, 1, 5];
coerced.sort((a, b) => String(a - b) as any);
check(coerced.join(",") === "1,3,5,7,9", "comparator ToNumber");
const coercionObjects = [8, 2, 6, 4];
coercionObjects.sort((a, b) => ({ valueOf() { return a - b; } }) as any);
check(coercionObjects.join(",") === "2,4,6,8", "allocating result coercion");
const infiniteResults = [3, 1, 2, 1];
infiniteResults.sort((a, b) => a === b ? -0 : a < b ? -Infinity : Infinity);
check(infiniteResults.join(",") === "1,1,2,3", "signed zero and infinite comparator results");
for (const result of [undefined, false, -0, NaN]) {
  const ties = [3, 1, 2];
  ties.sort(() => result as any);
  check(ties.join(",") === "3,1,2", "coerced equality preserves order");
}
for (const result of [1n, Object(1n), { valueOf() { return 1n; } },
  { [Symbol.toPrimitive]() { return 1n; } }, Symbol("not-a-number")]) {
  const input = [2, 1];
  let threw = false;
  try { input.sort(() => result as any); } catch (error) { threw = error instanceof TypeError; }
  check(threw && input.join(",") === "2,1", "invalid numeric result throws before publication");
}
console.log("comparator coercion");

const sparse = [3, , undefined, 1, , 2];
sparse.sort((a, b) => {
  check(a !== undefined && b !== undefined, "undefined reached comparator");
  return (a as number) - (b as number);
});
check(sparse.length === 6 && sparse.slice(0, 3).join(",") === "1,2,3", "sparse values");
check(3 in sparse && sparse[3] === undefined && !(4 in sparse) && !(5 in sparse), "sparse suffix");
const generic: any = { 0: 5, 2: 1, 3: 3, length: 4 };
Array.prototype.sort.call(generic, (a: number, b: number) => a - b);
check(generic[0] === 1 && generic[1] === 3 && generic[2] === 5 && !(3 in generic), "array-like receiver");
const source = [7, 1, 3];
check(source.toSorted((a, b) => a - b).join(",") === "1,3,7", "toSorted result");
check(source.join(",") === "7,1,3", "toSorted receiver");
console.log("sparse and copying sorts");

const mutated = [6, 4, 2, 5, 3, 1];
let mutationCalls = 0;
mutated.sort((a, b) => {
  if (mutationCalls++ === 0) {
    mutated[0] = 999;
    const inner = ["d", "b", "c", "a"];
    inner.sort((x, y) => x < y ? -1 : x > y ? 1 : 0);
    check(inner.join("") === "abcd", "reentrant sort");
  }
  return a - b;
});
check(mutated.join(",") === "1,2,3,4,5,6", "snapshot across mutation");
console.log("mutating and reentrant comparators");

for (const action of ["shrink", "grow", "freeze", "getter"]) {
  const receiver = [5, 4, 3, 2, 1];
  let changed = false;
  let threw = false;
  try {
    receiver.sort((a, b) => {
      if (!changed) {
        changed = true;
        if (action === "shrink") receiver.length = 0;
        if (action === "grow") receiver.push(8, 9);
        if (action === "freeze") Object.freeze(receiver);
        if (action === "getter") Object.defineProperty(receiver, "0", {
          get() { return 7; }, configurable: true,
        });
      }
      return a - b;
    });
  } catch (error) {
    check(error instanceof TypeError, "write-back must throw TypeError");
    threw = true;
  }
  check(threw === (action === "freeze" || action === "getter"), "write-back descriptor recheck");
  const expected = action === "shrink" ? "1,2,3,4,5"
    : action === "grow" ? "1,2,3,4,5,8,9"
    : action === "freeze" ? "5,4,3,2,1" : "7,4,3,2,1";
  check(receiver.join(",") === expected, "write-back after " + action);
}
console.log("receiver changes during sort");

const sealedDuringSort = [3, , 1];
let sealedOnce = false;
let deletionThrew = false;
try {
  sealedDuringSort.sort((a, b) => {
    if (!sealedOnce) {
      sealedOnce = true;
      sealedDuringSort[1] = 7;
      Object.seal(sealedDuringSort);
    }
    return a - b;
  });
} catch (error) {
  deletionThrew = error instanceof TypeError;
}
check(deletionThrew && sealedDuringSort.join(",") === "1,3,1", "strict deletion after callback sealing");
console.log("strict sorted suffix deletion");

// The final write-back can call setters after the comparator has finished.
// Keep the private source and index workspace valid across those allocations.
const setterRows: any[] = [];
for (let i = 0; i < 128; i++) setterRows.push({key: 127 - i});
const originalSetterRows = setterRows.slice();
let setterSeen: any;
let setterInstalled = false;
setterRows.sort((a, b) => {
  if (!setterInstalled) {
    setterInstalled = true;
    Object.defineProperty(setterRows, "0", {
      configurable: true,
      set(value) {
        setterSeen = value;
        const pressure: any[] = [];
        for (let j = 0; j < 256; j++) pressure.push({j, text: "write-back-" + j});
        check(pressure[255].j === 255, "setter allocations");
      }
    });
  }
  return a.key - b.key;
});
check(setterSeen === originalSetterRows[127], "setter receives sorted identity");
for (let i = 1; i < 128; i++) {
  check(setterRows[i] === originalSetterRows[127 - i], "sorted identities after allocating setter");
}
console.log("allocating setter write-back");

for (const define of [false, true]) {
  const proto: any = Object.prototype;
  if (define) Object.defineProperty(proto, "1", {value: 2, writable: true, configurable: true});
  else proto[1] = 2;
  const inherited = [3, , 1];
  try {
    inherited.sort((a, b) => a - b);
  } finally {
    delete proto[1];
  }
  check(inherited.join(",") === "1,2,3", "indexed Object.prototype admission");
}
console.log("indexed prototype sorting");

for (const throwAfter of [1, 17, 100]) {
  const throwing: number[] = [];
  for (let i = 0; i < 257; i++) throwing.push((i * 73) % 257);
  const before = throwing.join(",");
  const marker = { name: "sort exception", throwAfter };
  let calls = 0;
  let caught = false;
  try {
    throwing.sort((a, b) => {
      if (++calls === throwAfter) throw marker;
      return a - b;
    });
  } catch (error) {
    caught = error === marker;
  }
  check(caught && throwing.join(",") === before, "throw identity and unpublished result");
  throwing.sort((a, b) => a - b);
  for (let i = 0; i < throwing.length; i++) check(throwing[i] === i, "sort after exception");
}
console.log("throwing comparators and recovery");
