"use strict";
import { countedIndexedAccessor } from "./numeric-refusal-control.ts";
class RefusalReader {
    m = () => 1;
    read(a: number[], index: number): number {
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
const reader = new RefusalReader();
const entities: number[] = [], values: number[] = [];
for (let i = 0; i < 128; i++) { entities.push(i); values.push(i * 2); }
const events = countedIndexedAccessor(entities);
let visited = 0;
reader.visit(entities, values, (e, v) => { visited += e + v; });
console.log(visited, events());
