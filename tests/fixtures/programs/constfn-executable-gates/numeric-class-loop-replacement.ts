"use strict";
// Separate executable: no other loop can contribute R/bare route counts.
// This supplements the unchanged original class fixtures; it cannot replace
// their Infinity/subclass/frozen/accessor or retained-tier cost obligations.
class NumericCell { a: number = 0; }
function classLoopReplacement(p: NumericCell): void {
    for (let i = 0; i < 200; i++) p.a = p.a + 1;
}
const cell = new NumericCell();
classLoopReplacement(cell);
console.log(cell.a);
