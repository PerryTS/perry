"use strict";
import { countedPairAccessor } from "./numeric-refusal-control.ts";
class RefusalPair { a: number = 1; b: number = 2; }
function affectedPair(p: RefusalPair): void {
    for (let i = 0; i < 200; i++) p.a = p.a + p.b;
}
const pair = new RefusalPair();
const events = countedPairAccessor(pair);
affectedPair(pair);
console.log(events());
