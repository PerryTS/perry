"use strict";
import { countedCellAccessor } from "./numeric-refusal-control.ts";
class LoopCell { a: number = 0; }
function untouchedCell(cell: LoopCell): void {
    for (let i = 0; i < 200; i++) cell.a = 42;
}
const cell = new LoopCell();
if (process.argv[2] === "refuse") {
    const events = countedCellAccessor(cell);
    untouchedCell(cell);
    console.log(events());
} else {
    untouchedCell(cell);
    console.log(cell.a);
}
