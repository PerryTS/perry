"use strict";
import { refuseNumericRoutes } from "./numeric-control.ts";

class Pair {
    a: number = 1;
    b: number = 2;
    m = () => this.a;
}
function callM(receiver: any) { return receiver.m(); }
// Multiplication defeats the ordinary Add fact-tree spelling. P7 must
// derive exact R reads and scoped Number locals for this arithmetic loop.
function regionRead(p: Pair): number {
    let sum = 0;
    // Stay above the transform's full-unroll limit: exercise an actual loop.
    for (let i = 0; i < 200; i++) sum += p.a * 2 + p.b * 2;
    return sum;
}
function regionStore(p: Pair, n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) { p.a = i * 0.5; sum += p.b; }
    return sum;
}
class Reader {
    m = () => 1;
    read(a: number[], index: number): number {
        // Exercise the checked-reader family that supplies versioned indexing.
        if (a === undefined) throw new Error("missing array");
        const value = a[index];
        if (value === -1) throw new Error("missing value");
        return value;
    }
    visit(entities: number[], values: number[], callback: (e: number, v: number) => void): void {
        const n = entities.length;
        for (let i = 0; i < n; i++) {
            const entity = entities[i];
            callback(entity, this.read(values, i));
        }
    }
}
const reader = new Reader();
const entities: number[] = [], values: number[] = [];
for (let i = 0; i < 128; i++) { entities.push(i); values.push(i * 2); }
// A contained numeric receiver supplies the classic loop tier. Pair remains
// the mixed numeric/ConstFn receiver for independent arithmetic region tests.
class LoopCell { a: number = 0; }
const cell = new LoopCell();
const pair = new Pair();
if (process.argv[2] === "refuse") refuseNumericRoutes(pair, entities);
for (let i = 0; i < 2048; i++) pair.a = pair.a + pair.b;
console.log(pair.a, pair.b, regionRead(pair), regionStore(pair, 128), pair.a);
let methodSum = 0;
for (let i = 0; i < 1024; i++) methodSum += callM(pair);
console.log(methodSum);
for (let i = 0; i < 200; i++) cell.a = 42;
console.log(cell.a);
let visited = 0;
reader.visit(entities, values, (e, v) => { visited += e + v; });
console.log(visited);
