// Runtime stores skip barrier and layout bookkeeping for values that cannot
// name a heap object, and closure births skip the newborn barrier while no
// incremental cycle is live. The pointer-bearing stores these tests interleave
// with scalars must keep every edge: old containers (promoted by the churn
// below) receive young objects through Map/Set, array push, indexed store and
// closure captures, and are read back only after many more allocations.

class Node {
  id: number;
  label: string;
  constructor(id: number, label: string) {
    this.id = id;
    this.label = label;
  }
}

function churn(rounds: number): number {
  let total = 0;
  for (let i = 0; i < rounds; i++) {
    const tmp = { a: i, b: [i, i + 1, i + 2], s: "t" + i };
    total += tmp.b.length + tmp.s.length;
  }
  return total;
}

// Containers that survive collections before the mixed stores begin.
const oldMap = new Map<number, unknown>();
const oldSet = new Set<unknown>();
const oldArr: unknown[] = [];
const numArr: number[] = [];
const closures: Array<() => string> = [];
churn(200000);

for (let i = 0; i < 4000; i++) {
  // Alternate scalar and pointer payloads into the same containers.
  oldMap.set(i, i % 3 === 0 ? new Node(i, "m" + i) : i * 0.5);
  oldSet.add(i % 5 === 0 ? new Node(i, "s" + i) : i);
  oldArr.push(i % 4 === 0 ? new Node(i, "a" + i) : i % 4 === 1 ? true : i + 0.25);
  numArr.push(i * 1.5);
  if (i % 50 === 0) numArr[i >> 1] = -i;
  const captured = new Node(i, "c" + i);
  const scalar = i * 2;
  closures.push(() => captured.label + ":" + scalar);
  if (i % 256 === 0) churn(20000);
}

churn(300000);

let mapNodes = 0;
let mapSum = 0;
oldMap.forEach((v, k) => {
  if (v instanceof Node) {
    if (v.id !== k || v.label !== "m" + k) throw new Error("map edge lost at " + k);
    mapNodes++;
  } else {
    mapSum += v as number;
  }
});
let setNodes = 0;
for (const v of oldSet) if (v instanceof Node && v.label === "s" + v.id) setNodes++;
let arrNodes = 0;
let arrOther = 0;
for (let i = 0; i < oldArr.length; i++) {
  const v = oldArr[i];
  if (v instanceof Node) {
    if (v.label !== "a" + i) throw new Error("array edge lost at " + i);
    arrNodes++;
  } else {
    arrOther++;
  }
}
let numSum = 0;
for (const n of numArr) numSum += n;
let closureOk = 0;
for (let i = 0; i < closures.length; i++) {
  if (closures[i]() === "c" + i + ":" + i * 2) closureOk++;
}
console.log("map", oldMap.size, mapNodes, mapSum);
console.log("set", oldSet.size, setNodes);
console.log("array", oldArr.length, arrNodes, arrOther);
console.log("numbers", numArr.length, numSum);
console.log("closures", closures.length, closureOk);

// Overwrite pointer slots with scalars and back, then read everything again.
for (let i = 0; i < 4000; i += 3) {
  oldMap.set(i, i);
  oldArr[i] = new Node(-i, "late" + i);
}
churn(200000);
let late = 0;
for (let i = 0; i < 4000; i += 3) {
  const v = oldArr[i] as Node;
  if (v.id === -i && v.label === "late" + i && oldMap.get(i) === i) late++;
}
console.log("late", late);
