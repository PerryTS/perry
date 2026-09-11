declare function gc(): void;

function values(a: any[]): string {
  return a.map((value) => value === undefined ? "undefined" : String(value)).join(",");
}

const chunked: number[] = [];
const chunkedAlias = chunked;
let returnedLength = chunked.push(...[]);
for (let start = 0; start < 101; start += 32) {
  const chunk: number[] = [];
  for (let i = start; i < Math.min(101, start + 32); i++) chunk.push(i);
  returnedLength = chunked.push(...chunk);
}
console.log("chunked", chunked === chunkedAlias, returnedLength, chunked.length,
  chunked[0], chunked[31], chunked[32], chunked[100]);

const self = Array.from({ length: 20 }, (_, i) => i + 1);
const selfAlias = self;
const selfLength = self.push(...self);
console.log("self", self === selfAlias, selfLength, self.length,
  self[0], self[19], self[20], self[39]);

const own: any[] = [1, 2, 3];
own[Symbol.iterator] = function*(): Generator<number> {
  yield 7;
  yield 8;
};
const ownOut: number[] = [6];
console.log("own", ownOut.push(...own), values(ownOut));

const savedIterator = Array.prototype[Symbol.iterator];
Array.prototype[Symbol.iterator] = function*(): Generator<number> {
  yield 41;
};
const prototypeOut: number[] = [40];
const prototypeLength = prototypeOut.push(...[1, 2, 3]);
Array.prototype[Symbol.iterator] = savedIterator;
console.log("prototype", prototypeLength, values(prototypeOut));

const holey = [1, , 3];
const holeyOut: any[] = [];
holeyOut.push(...holey);
console.log("holes", holeyOut.length, 1 in holeyOut, values(holeyOut));

const accessor: any[] = [1, 2, 3];
Object.defineProperty(accessor, "0", {
  configurable: true,
  get(): number {
    accessor[1] = 20;
    accessor.length = 2;
    return 10;
  },
});
const accessorOut: any[] = [];
console.log("accessor", accessorOut.push(...accessor), values(accessorOut));

const mutatedDestination: number[] = [1];
const mutatingSource: any[] = [0];
mutatingSource[Symbol.iterator] = function*(): Generator<number> {
  mutatedDestination.push(2);
  yield 3;
};
console.log("mutation", mutatedDestination.push(...mutatingSource), values(mutatedDestination));

const generic: any = [];
const mixedChunk: any[] = [1, { id: 2 }, "three"];
console.log("generic", generic.push(...mixedChunk), generic[0], generic[1].id, generic[2]);

const concatSource = [1, 2];
const concatResult = concatSource.concat([3, 4]);
console.log("concat", values(concatSource), values(concatResult), concatSource !== concatResult);

const throwing: any[] = [1];
throwing[Symbol.iterator] = function(): any {
  return {
    next(): any {
      throw new Error("iterator-boom");
    },
  };
};
try {
  const never: any[] = [];
  never.push(...throwing);
  console.log("throw", "missing");
} catch (error: any) {
  console.log("throw", error.message);
}

// Promote a reused destination, then append freshly allocated pointer-bearing
// chunks. Forced-moving runs verify that every new old-to-young edge survives.
const oldDestination: any[] = [];
for (let i = 0; i < 2300; i++) oldDestination.push({ id: -i });
gc();
for (let round = 0; round < 40; round++) {
  const youngChunk: any[] = [];
  for (let i = 0; i < 7; i++) youngChunk.push({ id: round * 7 + i });
  oldDestination.push(...youngChunk);
  const churn: any[] = [];
  for (let i = 0; i < 2000; i++) churn.push({ i, pad: "x" + i });
  gc();
}
let live = 0;
for (let i = 2300; i < oldDestination.length; i++) {
  if (oldDestination[i].id === i - 2300) live++;
}
console.log("old-young", oldDestination.length, live);
