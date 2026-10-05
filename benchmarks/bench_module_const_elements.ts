// A module-level const array of class instances, read element by element
// inside a function. The nonintegral bound keeps this in the generic function.
class Cell {
    value: number;
    constructor(value: number) { this.value = value; }
}
function makeCells(): Cell[] {
    const xs: Cell[] = [];
    for (let i = 0; i < 256; i++) xs.push(new Cell(i + 1));
    return xs;
}
const CELLS: Cell[] = makeCells();
function sumCells(n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        const cell: Cell = CELLS[i];
        sum += cell.value;
    }
    return sum;
}
let checksum = 0;
for (let repeat = 0; repeat < 40000; repeat++) checksum += sumCells(255.5);
console.log(checksum);
