// On wasm32 a Date cell has four bytes of padding between its pointer-sized
// meta word and its timestamp. Every property read loads the word at payload +4
// as the receiver's ShapeId, and the arena does not clear reused memory, so a
// Date allocated over a dead object kept that object's ShapeId and was read as
// the object: a property that only the dead object had came back from the
// Date's neighbouring memory. Dead objects of three sizes are laid down and
// collected, repeatedly and in bulk so the collector recycles their memory,
// then Dates are allocated over it; none may read like an object.
import { collect } from "perry/gc";

const COUNT = 20000;
const ROUNDS = 8;

// Each reader's property read is cached for the shape its objects have, so a
// Date carrying that shape would match the cache and read slot 0.
function readA1(x: any) { return x.a; }
function readA3(x: any) { return x.a; }
function readA6(x: any) { return x.a; }

// The dead objects are held in an array while they are read, so they are real
// heap objects (a literal that never escapes is not allocated at all), and the
// array is dropped before the collection.
let bag: any[] = [];
function populate(which: number): number {
  let n = 0;
  bag = [];
  for (let i = 0; i < COUNT; i++) {
    if (which === 1) bag.push({ a: i });
    else if (which === 3) bag.push({ a: i, b: i, c: i });
    else bag.push({ a: i, b: i, c: i, d: i, e: i, f: i });
  }
  for (let i = 0; i < COUNT; i += 50) {
    n += which === 1 ? readA1(bag[i]) : which === 3 ? readA3(bag[i]) : readA6(bag[i]);
  }
  bag = [];
  return n;
}

let checked = 0;
// Reads are direct calls: a function-valued parameter would add an indirect
// call, which is a different wasm32 bug.
function misreads(which: number): number {
  populate(which);
  collect();
  const dates: Date[] = [];
  for (let i = 0; i < COUNT; i++) dates.push(new Date(i));
  let wrong = 0;
  for (let i = 0; i < COUNT; i++) {
    const date = dates[i];
    const seen = which === 1 ? readA1(date) : which === 3 ? readA3(date) : readA6(date);
    if (seen !== undefined || "a" in date) wrong++;
    else if (i % 100 === 0 && Object.keys(date).length !== 0) wrong++;
    checked++;
  }
  return wrong;
}
let w1 = 0, w3 = 0, w6 = 0;
for (let round = 0; round < ROUNDS; round++) {
  w1 += misreads(1);
  w3 += misreads(3);
  w6 += misreads(6);
}
console.log(w1, w3, w6);
console.log(checked);
