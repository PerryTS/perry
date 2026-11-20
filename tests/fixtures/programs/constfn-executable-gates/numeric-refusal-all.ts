"use strict";
import { countedPairAccessor, countedCellAccessor } from "./numeric-refusal-control.ts";
class RefusalPair { a: number = 1; b: number = 2; }
class LoopCell { a: number = 0; }
const pair = new RefusalPair(), cell = new LoopCell();
const pairEvents = countedPairAccessor(pair), cellEvents = countedCellAccessor(cell);
for (let i = 0; i < 200; i++) pair.a = pair.a + pair.b;
for (let i = 0; i < 200; i++) cell.a = 42;
console.log(pairEvents());
console.log(cellEvents());
